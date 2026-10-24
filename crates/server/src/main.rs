use clap::{Parser, ValueEnum};
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(name = "openpim", version)]
struct Cli {
    #[arg(value_enum)]
    role: Role,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum Role {
    Api,
    Relay,
    Indexer,
    All,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    let cli = Cli::parse();
    tracing::info!(role = ?cli.role, "startar");
    anyhow::bail!("rollen {:?} är inte implementerad än", cli.role)
}
