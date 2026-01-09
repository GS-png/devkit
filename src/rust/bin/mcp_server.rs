// MCP server entry point
use sanshu::{mcp::run_server, utils::init_mcp_logger, log_important};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    init_mcp_logger()?;
    log_important!(info, "Starting MCP server");
    run_server().await
}
