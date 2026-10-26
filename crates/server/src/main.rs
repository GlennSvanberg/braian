#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

use std::net::SocketAddr;

use braian_api::AppState;
use braian_events::ChangeEvent;
use braian_search::Typesense;
use braian_store::Store;
use clap::{Parser, Subcommand};
use tokio::sync::watch;
use tracing_subscriber::EnvFilter;

mod seed;

#[derive(Parser)]
#[command(name = "braian", version)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// HTTP API.
    Api,
    /// Search indexer.
    Indexer,
    /// Rebuild the search alias from Postgres.
    Reindex,
    /// Demo user, catalog and workspaces. Idempotent.
    Seed,
    /// API, indexer and seed.
    All,
    /// Print the OpenAPI document as JSON.
    Openapi {
        #[arg(long)]
        out: Option<std::path::PathBuf>,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    if let Command::Openapi { out } = &cli.command {
        let json = braian_api::openapi_spec()
            .to_pretty_json()
            .map_err(|error| anyhow::anyhow!("openapi: {error}"))?;
        match out {
            Some(path) => {
                if let Some(parent) = path.parent()
                    && !parent.as_os_str().is_empty()
                {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::write(path, format!("{json}\n"))?;
            }
            None => println!("{json}"),
        }
        return Ok(());
    }

    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();
    ignore_removed_broker();

    match cli.command {
        Command::Api => {
            let (state, addr) = prepare(false).await?;
            serve(state, addr).await
        }
        Command::Indexer => indexer().await,
        Command::Reindex => {
            let store = connect().await?;
            let search = typesense().ok_or_else(|| anyhow::anyhow!("TYPESENSE_URL is not set"))?;
            let seq = braian_indexer::reindex(&store.on_bulk(), &search).await?;
            println!("reindex seq {seq}");
            Ok(())
        }
        Command::Seed => {
            let store = connect().await?;
            seed::run(&store, typesense().as_ref()).await
        }
        Command::All => {
            let (state, addr) = prepare(true).await?;
            let store = state.store.clone();
            if typesense().is_some() {
                let index_store = store.clone();
                tokio::spawn(async move {
                    if let Err(error) = run_indexer(index_store).await {
                        tracing::error!(%error, "indexer stopped");
                    }
                });
            } else {
                tracing::warn!("TYPESENSE_URL is not set; indexer is off");
            }
            serve(state, addr).await
        }
        Command::Openapi { .. } => unreachable!("handled above"),
    }
}

async fn prepare(seed_first: bool) -> anyhow::Result<(AppState, SocketAddr)> {
    let store = connect().await?;
    let search = typesense();
    if seed_first {
        seed::run(&store, search.as_ref()).await?;
    }
    let identities = match std::env::var("BRAIAN_DEV_TOKENS") {
        Ok(raw) if !raw.trim().is_empty() => braian_api::identities(&store, &raw).await?,
        _ => Vec::new(),
    };
    let addr = std::env::var("BRAIAN_API_ADDR")
        .unwrap_or_else(|_| "127.0.0.1:8090".into())
        .parse()?;
    let state = AppState::new(store, identities, search);
    let live = state.clone();
    tokio::spawn(async move {
        if let Err(error) = run_live(live).await {
            tracing::error!(%error, "live stream stopped");
        }
    });
    Ok((state, addr))
}

/// `KAFKA_BROKERS` used to start the relay. Existing units may still set it.
fn ignore_removed_broker() {
    if std::env::var("KAFKA_BROKERS")
        .ok()
        .is_some_and(|value| !value.trim().is_empty())
    {
        tracing::warn!("KAFKA_BROKERS is set and ignored; the change log is in Postgres");
    }
}

async fn serve(state: AppState, addr: SocketAddr) -> anyhow::Result<()> {
    let app = braian_api::into_router(state);
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!(%addr, "api listening");
    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}

async fn connect() -> anyhow::Result<Store> {
    let database_url =
        std::env::var("DATABASE_URL").map_err(|_| anyhow::anyhow!("DATABASE_URL is not set"))?;
    Ok(Store::connect(&database_url).await?)
}

fn typesense() -> Option<Typesense> {
    match (
        std::env::var("TYPESENSE_URL"),
        std::env::var("TYPESENSE_API_KEY"),
    ) {
        (Ok(url), Ok(key)) if !url.is_empty() => {
            let client = Typesense::new(url, key);
            match std::env::var("BRAIAN_SEARCH_ALIAS") {
                Ok(alias) if !alias.is_empty() && alias != braian_search::ALIAS => {
                    Some(client.with_alias(alias))
                }
                _ => Some(client),
            }
        }
        _ => None,
    }
}

async fn indexer() -> anyhow::Result<()> {
    run_indexer(connect().await?).await
}

async fn run_indexer(store: Store) -> anyhow::Result<()> {
    let search = typesense().ok_or_else(|| anyhow::anyhow!("TYPESENSE_URL is not set"))?;
    // The indexer puts its interactive lane on `BRAIAN_INDEXER_POOL` and bulk
    // reads on the bulk pool. Forcing `on_bulk` here would share one pool.
    let indexer = braian_indexer::Indexer::connect(store, search).await?;
    let (_tx, rx) = watch::channel(false);
    indexer.run(rx).await
}

async fn run_live(state: AppState) -> anyhow::Result<()> {
    loop {
        if let Err(error) = live_once(&state).await {
            tracing::warn!(%error, "live tail reconnecting");
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        }
    }
}

async fn live_once(state: &AppState) -> anyhow::Result<()> {
    let mut cursor = state.store.max_change_seq().await?;
    let listener = state.store.listen_changes().await?;
    loop {
        let rows = state.store.changes_after(cursor, 200).await?;
        if rows.is_empty() {
            listener.wait(std::time::Duration::from_millis(250)).await;
            continue;
        }
        for row in rows {
            cursor = row.seq;
            let event = ChangeEvent {
                seq: row.seq,
                op: row.op,
                object_id: row.object_id,
                version: row.version,
                batch_id: row.batch_id,
                kind: row.kind,
                at: row.at.to_rfc3339(),
            };
            state.emit(serde_json::to_value(&event)?);
        }
    }
}
