// MCP HTTP server entry point - runs as independent process
// This mode may bypass Windsurf's subprocess detection
//
// Usage:
// 1. Start this server: sanshu-mcp-http
// 2. Configure Windsurf mcp_config.json:
//    {
//      "mcpServers": {
//        "sanshu": {
//          "serverUrl": "http://127.0.0.1:8808/sse"
//        }
//      }
//    }

use sanshu::{mcp::ZhiServer, utils::auto_init_logger, log_important};
use rmcp::transport::sse_server::{SseServer, SseServerConfig};
use std::net::SocketAddr;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    auto_init_logger()?;
    
    let port: u16 = std::env::var("MCP_HTTP_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(8808);
    
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    
    log_important!(info, "Starting MCP HTTP/SSE server on port {}", port);
    
    // Create SSE server configuration
    let sse_config = SseServerConfig {
        bind: addr,
        sse_path: "/sse".to_string(),
        post_path: "/message".to_string(),
        ct: CancellationToken::new(),
        sse_keep_alive: Some(Duration::from_secs(30)),
    };
    
    // Create SSE server
    let (sse_server, sse_router) = SseServer::new(sse_config);
    
    // Register our MCP service
    sse_server.with_service(|| ZhiServer::new());
    
    log_important!(info, "MCP HTTP server ready at http://{}", addr);
    log_important!(info, "");
    log_important!(info, "=== Windsurf Configuration ===");
    log_important!(info, r#"Add to ~/.codeium/windsurf/mcp_config.json:"#);
    log_important!(info, r#"{{"mcpServers": {{"sanshu": {{"serverUrl": "http://127.0.0.1:{}/sse"}}}}}}"#, port);
    log_important!(info, "");
    
    // Start server
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, sse_router).await?;
    
    Ok(())
}
