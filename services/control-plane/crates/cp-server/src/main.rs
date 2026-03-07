use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    if std::env::args_os()
        .nth(1)
        .is_some_and(|arg| arg == "--healthcheck")
    {
        return cp_server::run_healthcheck().await;
    }

    cp_server::run().await
}
