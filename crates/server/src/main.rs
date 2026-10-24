use std::net::SocketAddr;

use clap::{Parser, Subcommand};
use openpim_store::Store;
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(name = "openpim", version)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// HTTP API for definitions and objects.
    Api,
    /// Reads the change log. Not implemented yet.
    Relay,
    /// Updates the search index. Not implemented yet.
    Indexer,
    /// Starts the API. Relay and indexer arrive in later phases.
    All,
    /// Print the OpenAPI document as JSON.
    Openapi {
        /// Write the document to this file instead of stdout.
        #[arg(long)]
        out: Option<std::path::PathBuf>,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    if let Command::Openapi { out } = &cli.command {
        let json = openpim_api::openapi_spec()
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
        Command::Api | Command::All => serve_api().await,
        Command::Relay => anyhow::bail!("relay is not implemented yet"),
        Command::Indexer => anyhow::bail!("indexer is not implemented yet"),
        Command::Openapi { .. } => unreachable!("handled above"),
    }
}

async fn serve_api() -> anyhow::Result<()> {
    let database_url =
        std::env::var("DATABASE_URL").map_err(|_| anyhow::anyhow!("DATABASE_URL is not set"))?;
    let tokens = std::env::var("OPENPIM_DEV_TOKENS")
        .map_err(|_| anyhow::anyhow!("OPENPIM_DEV_TOKENS is not set"))?;
    let addr: SocketAddr = std::env::var("OPENPIM_API_ADDR")
        .unwrap_or_else(|_| "127.0.0.1:8080".to_owned())
        .parse()?;

    let store = Store::connect(&database_url).await?;
    let identities = openpim_api::identities(&store, &tokens).await?;
    let app = openpim_api::router(store, identities);
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!(%addr, "api listening");
    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}
