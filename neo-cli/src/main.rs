mod app;
mod render;
mod rpc;

use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    app::run().await
}
