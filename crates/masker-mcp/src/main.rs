mod executor;
mod masker_path;
mod server;

use rmcp::ServiceExt;
use server::MaskerMcp;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let service = MaskerMcp.serve(rmcp::transport::stdio()).await?;
    service.waiting().await?;
    Ok(())
}
