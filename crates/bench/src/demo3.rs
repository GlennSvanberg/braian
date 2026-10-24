//! `just demo-3` loads a fresh database, serves the API, and runs the Hurl files.

use std::path::PathBuf;
use std::process::Stdio;

use anyhow::Context;
use openpim_store::admin::{self, PgConn};
use openpim_store::{STORAGE_FORM, Store};
use tokio::net::TcpListener;

const DATABASE: &str = "openpim_demo3";
const TOKENS: &str = "ada:human:demo3-ada;bot:agent@ada:demo3-bot";

pub async fn run(database_url: &str) -> anyhow::Result<()> {
    let hurl_dir = hurl_dir();
    if !hurl_dir.is_dir() {
        anyhow::bail!("hurl directory {} was not found", hurl_dir.display());
    }
    // Hurl joins `file,` paths with --file-root. The Hurl files use paths from
    // the repository root, and `just demo-3` is run from there.
    let file_root = std::env::current_dir().context("current directory")?;
    if !file_root.join("testdata/rules/definitions.json").is_file() {
        anyhow::bail!("run demo-3 from the repository root so Hurl can read testdata/");
    }

    let mut admin_config = admin::parse_config(database_url)?;
    admin_config.dbname("postgres");
    let admin = PgConn::connect(admin_config).await?;
    admin::drop_database(&admin.client, DATABASE)
        .await
        .with_context(|| format!("drop {DATABASE}"))?;
    admin::create_database(&admin.client, DATABASE).await?;
    drop(admin);

    let mut config = admin::parse_config(database_url)?;
    config.dbname(DATABASE);
    let store = Store::connect_config(config, STORAGE_FORM).await?;
    let identities = openpim_api::identities(&store, TOKENS).await?;
    let app = openpim_api::router(store, identities);
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let port = listener.local_addr()?.port();
    let server = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });

    let status = tokio::process::Command::new("hurl")
        .arg("--test")
        .arg("--file-root")
        .arg(file_root)
        .arg("--variable")
        .arg(format!("host=http://127.0.0.1:{port}"))
        .arg("--variable")
        .arg("token=demo3-ada")
        .arg("--variable")
        .arg("agent=demo3-bot")
        .arg(&hurl_dir)
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .await
        .context("could not run hurl. Install it with: winget install Orange-OpenSource.Hurl")?;
    server.abort();
    if !status.success() {
        anyhow::bail!("hurl exited with {status}");
    }
    println!("demo-3: hurl passed against {DATABASE} on 127.0.0.1:{port}");
    Ok(())
}

fn hurl_dir() -> PathBuf {
    let from_cwd = PathBuf::from("tests/hurl/phase-3");
    if from_cwd.is_dir() {
        return from_cwd;
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/hurl/phase-3")
}
