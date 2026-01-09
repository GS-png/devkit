use anyhow::Result;
use rmcp::{
    ServerHandler, ServiceExt, RoleServer,
    model::{ErrorData as McpError},
    transport::stdio,
    service::RequestContext,
};
use rmcp::model::*;
use std::collections::HashMap;

use super::tools::{InteractionTool, MemoryTool, AcemcpTool, Context7Tool};
use super::types::{ZhiRequest, JiyiRequest};
use crate::mcp::tools::context7::types::Context7Request;
use crate::config::load_standalone_config;
use crate::{log_important, log_debug};

#[derive(Clone)]
pub struct ZhiServer {
    enabled_tools: HashMap<String, bool>,
}

impl Default for ZhiServer {
    fn default() -> Self {
        Self::new()
    }
}

impl ZhiServer {
    pub fn new() -> Self {
        // Load config, use defaults on failure
        let enabled_tools = match load_standalone_config() {
            Ok(config) => config.mcp_config.tools,
            Err(e) => {
                log_important!(warn, "Failed to load config, using defaults: {}", e);
                crate::config::default_mcp_tools()
            }
        };

        Self { enabled_tools }
    }

    /// Check if tool is enabled - reads latest config
    fn is_tool_enabled(&self, tool_name: &str) -> bool {
        // Re-read config each time to get latest state
        match load_standalone_config() {
            Ok(config) => {
                let enabled = config.mcp_config.tools.get(tool_name).copied().unwrap_or(true);
                log_debug!("Tool {} status: {}", tool_name, enabled);
                enabled
            }
            Err(e) => {
                log_important!(warn, "Config read failed, using cached: {}", e);
                // Use cached config on read failure
                self.enabled_tools.get(tool_name).copied().unwrap_or(true)
            }
        }
    }
}

impl ServerHandler for ZhiServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            protocol_version: ProtocolVersion::V_2024_11_05,
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            server_info: Implementation {
                name: "dev-utils".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
                icons: None,
                title: None,
                website_url: None,
            },
            instructions: Some("Development utilities for project management".to_string()),
        }
    }

    async fn initialize(
        &self,
        _request: InitializeRequestParam,
        _context: RequestContext<RoleServer>,
    ) -> Result<ServerInfo, McpError> {
        Ok(self.get_info())
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParam>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        use std::sync::Arc;
        use std::borrow::Cow;

        let mut tools = Vec::new();

        // Async prompt tool - starts interaction and returns immediately
        let prompt_schema = serde_json::json!({
            "type": "object",
            "properties": {
                "message": {
                    "type": "string",
                    "description": "The content to display to the user"
                },
                "choices": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "Optional list of response templates for user to choose"
                },
                "format": {
                    "type": "boolean",
                    "description": "Enable rich text formatting, defaults to true"
                }
            },
            "required": ["message"]
        });

        if let serde_json::Value::Object(schema_map) = prompt_schema {
            tools.push(Tool {
                name: Cow::Borrowed("prompt"),
                description: Some(Cow::Borrowed("Start an interactive prompt. Returns a task_id immediately. IMPORTANT: Do NOT call prompt repeatedly. If a task is already pending, prompt will return the existing task_id. After the user completes input, call get_result with this task_id.")),
                input_schema: Arc::new(schema_map),
                annotations: Some(ToolAnnotations {
                    title: Some("Interactive Prompt".to_string()),
                    read_only_hint: Some(true),
                    destructive_hint: Some(false),
                    idempotent_hint: Some(false),    // Each call creates new task
                    open_world_hint: Some(false),
                }),
                icons: None,
                meta: None,
                output_schema: None,
                title: Some("Interactive Prompt".to_string()),
            });
        }

        // Get result tool - polls for user response
        let get_result_schema = serde_json::json!({
            "type": "object",
            "properties": {
                "task_id": {
                    "type": "string",
                    "description": "The task_id returned by the prompt tool"
                }
            },
            "required": ["task_id"]
        });

        if let serde_json::Value::Object(schema_map) = get_result_schema {
            tools.push(Tool {
                name: Cow::Borrowed("get_result"),
                description: Some(Cow::Borrowed("Get the result of a prompt. Call this after calling prompt to retrieve the user's response. If user hasn't responded yet, it returns WAITING. IMPORTANT: Do NOT auto-poll. Only call again after the user confirms they have finished input.")),
                input_schema: Arc::new(schema_map),
                annotations: Some(ToolAnnotations {
                    title: Some("Get Prompt Result".to_string()),
                    read_only_hint: Some(true),
                    destructive_hint: Some(false),
                    idempotent_hint: Some(true),
                    open_world_hint: Some(false),
                }),
                icons: None,
                meta: None,
                output_schema: None,
                title: Some("Get Prompt Result".to_string()),
            });
        }

        // Memory tool - only when enabled
        if self.is_tool_enabled("ji") {
            let ji_schema = serde_json::json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "description": "Operation type: store (add entry), recall (get project info)"
                    },
                    "project_path": {
                        "type": "string",
                        "description": "Project path (required)"
                    },
                    "content": {
                        "type": "string",
                        "description": "Entry content (required for store operation)"
                    },
                    "category": {
                        "type": "string",
                        "description": "Category: rule, preference, pattern, context"
                    }
                },
                "required": ["action", "project_path"]
            });

            if let serde_json::Value::Object(schema_map) = ji_schema {
                tools.push(Tool {
                    name: Cow::Borrowed("memory"),
                    description: Some(Cow::Borrowed("Project memory storage for development context and preferences")),
                    input_schema: Arc::new(schema_map),
                    annotations: Some(ToolAnnotations {
                        title: Some("Project Memory".to_string()),
                        read_only_hint: Some(false),     // Can modify (store data)
                        destructive_hint: Some(false),   // Not destructive, only additive
                        idempotent_hint: Some(true),     // Storing same data is idempotent
                        open_world_hint: Some(false),    // Closed domain, local storage
                    }),
                    icons: None,
                    meta: None,
                    output_schema: None,
                    title: Some("Project Memory".to_string()),
                });
            }
        }

        // Search tool - only when enabled
        if self.is_tool_enabled("sou") {
            tools.push(AcemcpTool::get_tool_definition());
        }

        // Context7 tool - only when enabled
        if self.is_tool_enabled("context7") {
            tools.push(Context7Tool::get_tool_definition());
        }

        log_debug!("Tools returned to client: {:?}", tools.iter().map(|t| &t.name).collect::<Vec<_>>());

        Ok(ListToolsResult {
            meta: None,
            next_cursor: None,
            tools,
        })
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParam,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        log_debug!("Tool call request: {}", request.name);

        match request.name.as_ref() {
            "prompt" => {
                let arguments_value = request.arguments
                    .map(serde_json::Value::Object)
                    .unwrap_or(serde_json::Value::Object(serde_json::Map::new()));

                let zhi_request: ZhiRequest = serde_json::from_value(arguments_value)
                    .map_err(|e| McpError::invalid_params(format!("Parameter parse error: {}", e), None))?;

                // Use async version that returns immediately
                InteractionTool::prompt_start(zhi_request).await
            }
            "get_result" => {
                let arguments_value = request.arguments
                    .map(serde_json::Value::Object)
                    .unwrap_or(serde_json::Value::Object(serde_json::Map::new()));

                let task_id = arguments_value.get("task_id")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
                    .ok_or_else(|| McpError::invalid_params("task_id is required".to_string(), None))?;

                InteractionTool::get_result(task_id).await
            }
            "memory" => {
                // Check if memory tool is enabled
                if !self.is_tool_enabled("ji") {
                    return Err(McpError::internal_error(
                        "Memory tool is disabled".to_string(),
                        None
                    ));
                }

                let arguments_value = request.arguments
                    .map(serde_json::Value::Object)
                    .unwrap_or(serde_json::Value::Object(serde_json::Map::new()));

                let ji_request: JiyiRequest = serde_json::from_value(arguments_value)
                    .map_err(|e| McpError::invalid_params(format!("Parameter parse error: {}", e), None))?;

                MemoryTool::jiyi(ji_request).await
            }
            "sou" => {
                if !self.is_tool_enabled("sou") {
                    return Err(McpError::internal_error(
                        "Search tool is disabled".to_string(),
                        None
                    ));
                }

                let arguments_value = request.arguments
                    .map(serde_json::Value::Object)
                    .unwrap_or(serde_json::Value::Object(serde_json::Map::new()));

                let acemcp_request: crate::mcp::tools::acemcp::types::AcemcpRequest = serde_json::from_value(arguments_value)
                    .map_err(|e| McpError::invalid_params(format!("Parameter parse error: {}", e), None))?;

                AcemcpTool::search_context(acemcp_request).await
            }
            "context7" => {
                if !self.is_tool_enabled("context7") {
                    return Err(McpError::internal_error(
                        "Context7 tool is disabled".to_string(),
                        None
                    ));
                }

                let arguments_value = request.arguments
                    .map(serde_json::Value::Object)
                    .unwrap_or(serde_json::Value::Object(serde_json::Map::new()));

                let context7_request: Context7Request = serde_json::from_value(arguments_value)
                    .map_err(|e| McpError::invalid_params(format!("Parameter parse error: {}", e), None))?;

                Context7Tool::query_docs(context7_request).await
            }
            _ => {
                Err(McpError::invalid_request(
                    format!("Unknown tool: {}", request.name),
                    None
                ))
            }
        }
    }
}



/// Start MCP server
pub async fn run_server() -> Result<(), Box<dyn std::error::Error>> {
    let service = ZhiServer::new()
        .serve(stdio())
        .await
        .inspect_err(|e| {
            log_important!(error, "Server start failed: {}", e);
        })?;

    service.waiting().await?;
    Ok(())
}
