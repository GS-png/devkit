/// MCP error handling utilities

use rmcp::model::ErrorData as McpError;

/// MCP tool error types
#[derive(Debug, thiserror::Error)]
pub enum McpToolError {
    #[error("Project path error: {0}")]
    ProjectPath(String),
    
    #[error("UI creation failed: {0}")]
    PopupCreation(String),
    
    #[error("Response parsing failed: {0}")]
    ResponseParsing(String),
    
    #[error("Memory error: {0}")]
    Memory(String),
    
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    
    #[error("Error: {0}")]
    Generic(#[from] anyhow::Error),
}

impl From<McpToolError> for McpError {
    fn from(error: McpToolError) -> Self {
        match error {
            McpToolError::ProjectPath(msg) => {
                McpError::invalid_params(msg, None)
            }
            McpToolError::PopupCreation(msg) | 
            McpToolError::ResponseParsing(msg) | 
            McpToolError::Memory(msg) => {
                McpError::internal_error(msg, None)
            }
            McpToolError::Io(e) => {
                McpError::internal_error(format!("IO error: {}", e), None)
            }
            McpToolError::Json(e) => {
                McpError::internal_error(format!("JSON error: {}", e), None)
            }
            McpToolError::Generic(e) => {
                McpError::internal_error(e.to_string(), None)
            }
        }
    }
}

pub fn project_path_error(msg: impl Into<String>) -> McpToolError {
    McpToolError::ProjectPath(msg.into())
}

pub fn popup_error(msg: impl Into<String>) -> McpToolError {
    McpToolError::PopupCreation(msg.into())
}

pub fn response_error(msg: impl Into<String>) -> McpToolError {
    McpToolError::ResponseParsing(msg.into())
}

pub fn memory_error(msg: impl Into<String>) -> McpToolError {
    McpToolError::Memory(msg.into())
}
