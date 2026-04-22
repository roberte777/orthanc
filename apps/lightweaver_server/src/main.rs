use lightweaver_server::sqlite::{DbConfig, db_init};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = DbConfig::from_env();
    db_init(&config).await?;
    Ok(())
}
