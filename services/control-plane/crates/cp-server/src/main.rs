use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    if let Some(arg) = std::env::args_os().nth(1) {
        if arg == "--healthcheck" {
            return cp_server::run_healthcheck().await;
        }
        if arg == "--version" {
            println!("cp-server {}", env!("CARGO_PKG_VERSION"));
            return Ok(());
        }
    }

    cp_server::run().await
}
