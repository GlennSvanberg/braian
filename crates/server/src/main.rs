#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

use std::net::SocketAddr;

use braian_api::AppState;
use braian_events::{BULK_TOPIC, INTERACTIVE_TOPIC, Subscription};
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
    /// Logical replication into Redpanda or Kafka.
    Relay,
    /// Search indexer.
    Indexer,
    /// Rebuild the search alias from Postgres.
    Reindex,
    /// Demo user, catalog and workspaces. Idempotent.
    Seed,
    /// API, relay, indexer and seed.
    All,
    /// Usage consumer. Reads braian.telemetry.ui only.
    Telemetry,
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

    match cli.command {
        Command::Api => {
            let (mut state, addr) = prepare(false).await?;
            attach_usage(&mut state);
            serve(state, addr).await
        }
        Command::Relay => relay().await,
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
            let (mut state, addr) = prepare(true).await?;
            attach_usage(&mut state);
            let store = state.store.clone();
            let brokers = std::env::var("KAFKA_BROKERS").ok();
            if let Some(brokers) = brokers.clone() {
                let relay_store = store.clone();
                tokio::spawn(async move {
                    if let Err(error) = run_relay(relay_store).await {
                        tracing::error!(%error, "relay stopped");
                    }
                });
                let index_store = store.clone();
                let index_brokers = brokers.clone();
                tokio::spawn(async move {
                    if let Err(error) = run_indexer(index_store, &index_brokers).await {
                        tracing::error!(%error, "indexer stopped");
                    }
                });
                let live = state.clone();
                let live_brokers = brokers.clone();
                tokio::spawn(async move {
                    if let Err(error) = run_live(live, &live_brokers).await {
                        tracing::error!(%error, "live stream stopped");
                    }
                });
                let telemetry_store = store.clone();
                let telemetry_brokers = brokers.clone();
                tokio::spawn(async move {
                    let classifier = braian_telemetry::ClassifierMode::from_env();
                    if let Err(error) =
                        braian_telemetry::run(telemetry_store, &telemetry_brokers, classifier).await
                    {
                        tracing::error!(%error, "telemetry stopped");
                    }
                });
            } else {
                tracing::warn!(
                    "KAFKA_BROKERS is not set; relay, indexer and the kafka stream are off"
                );
            }
            serve(state, addr).await
        }
        Command::Telemetry => telemetry().await,
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
    Ok((AppState::new(store, identities, search), addr))
}

fn attach_usage(state: &mut AppState) {
    if let Ok(brokers) = std::env::var("KAFKA_BROKERS")
        && !brokers.is_empty()
    {
        state.usage_tx = Some(braian_telemetry::spawn_publisher(&brokers));
    }
    let state = state.clone();
    tokio::spawn(async move {
        if let Ok(settings) = state.store.usage_settings().await
            && let Ok(mut assigner) = state.usage_assign.lock()
        {
            assigner.set_gap(i64::from(settings.inactivity_gap_seconds) * 1000);
        }
    });
}

async fn telemetry() -> anyhow::Result<()> {
    let brokers = std::env::var("KAFKA_BROKERS")?;
    braian_telemetry::run(
        connect().await?,
        &brokers,
        braian_telemetry::ClassifierMode::from_env(),
    )
    .await
    .map_err(|error| anyhow::anyhow!(error))
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

async fn relay() -> anyhow::Result<()> {
    run_relay(connect().await?).await
}

async fn run_relay(store: Store) -> anyhow::Result<()> {
    let database_url = std::env::var("DATABASE_URL")?;
    let brokers = std::env::var("KAFKA_BROKERS")?;
    let listen = std::env::var("BRAIAN_RELAY_ADDR")
        .unwrap_or_else(|_| "127.0.0.1:8091".into())
        .parse()?;
    let lag_limit_bytes = std::env::var("BRAIAN_RELAY_LAG_LIMIT_BYTES")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(64 * 1024 * 1024);
    braian_relay::run(
        braian_relay::Config {
            database_url,
            brokers,
            slot: std::env::var("BRAIAN_RELAY_SLOT").unwrap_or_else(|_| "braian_relay".into()),
            listen,
            lag_limit_bytes,
            heartbeat_interval: std::env::var("BRAIAN_RELAY_HEARTBEAT_MS")
                .ok()
                .and_then(|value| value.parse().ok())
                .map(std::time::Duration::from_millis)
                .unwrap_or(braian_relay::HEARTBEAT_INTERVAL),
        },
        store,
    )
    .await
}

async fn indexer() -> anyhow::Result<()> {
    let brokers = std::env::var("KAFKA_BROKERS")?;
    run_indexer(connect().await?, &brokers).await
}

async fn run_indexer(store: Store, brokers: &str) -> anyhow::Result<()> {
    let store = store.on_bulk();
    let search = typesense().ok_or_else(|| anyhow::anyhow!("TYPESENSE_URL is not set"))?;
    let indexer = braian_indexer::Indexer::new(store, search, brokers, "braian-indexer")?;
    let (_tx, rx) = watch::channel(false);
    indexer.run(rx).await
}

async fn run_live(state: AppState, brokers: &str) -> anyhow::Result<()> {
    let subscription =
        Subscription::subscribe(brokers, "braian-live", &[INTERACTIVE_TOPIC, BULK_TOPIC])?;
    loop {
        let event = subscription.next().await?;
        state.emit(serde_json::to_value(&event)?);
        subscription.commit()?;
    }
}
