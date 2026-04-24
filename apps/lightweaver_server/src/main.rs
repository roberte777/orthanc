use lightweaver_server::sqlite::{DbConfig, db_init};
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = DbConfig::from_env();
    db_init(&config).await?;

    // set up the background tasks
    let _cancel = CancellationToken::new();
    let _join: JoinSet<()> = JoinSet::new();
    Ok(())
}
