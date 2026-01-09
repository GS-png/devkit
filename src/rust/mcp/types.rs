use chrono;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ZhiRequest {
    #[schemars(description = "The content to display")]
    pub message: String,
    
    // Support both old name (predefined_options) and new name (choices) for compatibility
    #[schemars(description = "Optional list of response templates")]
    #[serde(default, alias = "predefined_options")]
    pub choices: Vec<String>,
    
    #[schemars(description = "Enable rich text formatting, defaults to true")]
    #[serde(default = "default_is_markdown", alias = "is_markdown")]
    pub format: bool,
    
    #[schemars(description = "Project root path for context")]
    #[serde(default)]
    pub project_root_path: Option<String>,
}

impl ZhiRequest {
    /// Get choices (for backward compatibility with predefined_options)
    pub fn get_choices(&self) -> &Vec<String> {
        &self.choices
    }
    
    /// Get format setting (for backward compatibility with is_markdown)
    pub fn get_format(&self) -> bool {
        self.format
    }
}

fn default_is_markdown() -> bool {
    true
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct JiyiRequest {
    #[schemars(description = "Operation type: store (add entry), recall (get project info)")]
    pub action: String,
    #[schemars(description = "Project path (required)")]
    pub project_path: String,
    #[schemars(description = "Entry content (required for store operation)")]
    #[serde(default)]
    pub content: String,
    #[schemars(
        description = "Category: rule, preference, pattern, context"
    )]
    #[serde(default = "default_category")]
    pub category: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct AcemcpRequest {
    #[schemars(description = "Absolute path to project root directory using forward slashes")]
    pub project_root_path: String,
    #[schemars(description = "Natural language search query to find relevant code context")]
    pub query: String,
}

fn default_category() -> String {
    "context".to_string()
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PopupRequest {
    pub id: String,
    pub message: String,
    pub predefined_options: Option<Vec<String>>,
    pub is_markdown: bool,
    pub project_root_path: Option<String>,
}

/// Structured response data format
#[derive(Debug, Deserialize)]
pub struct McpResponse {
    pub user_input: Option<String>,
    pub selected_options: Vec<String>,
    pub images: Vec<ImageAttachment>,
    pub metadata: ResponseMetadata,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ImageAttachment {
    pub data: String,
    pub media_type: String,
    pub filename: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ResponseMetadata {
    pub timestamp: Option<String>,
    pub request_id: Option<String>,
    pub source: Option<String>,
}

/// Legacy format compatibility
#[derive(Debug, Deserialize)]
pub struct McpResponseContent {
    #[serde(rename = "type")]
    pub content_type: String,
    pub text: Option<String>,
    pub source: Option<ImageSource>,
}

#[derive(Debug, Deserialize)]
pub struct ImageSource {
    #[serde(rename = "type")]
    pub source_type: String,
    pub media_type: String,
    pub data: String,
}

/// Build MCP response
pub fn build_mcp_response(
    user_input: Option<String>,
    selected_options: Vec<String>,
    images: Vec<ImageAttachment>,
    request_id: Option<String>,
    source: &str,
) -> serde_json::Value {
    serde_json::json!({
        "user_input": user_input,
        "selected_options": selected_options,
        "images": images,
        "metadata": {
            "timestamp": chrono::Utc::now().to_rfc3339(),
            "request_id": request_id,
            "source": source
        }
    })
}

/// Build send response
pub fn build_send_response(
    user_input: Option<String>,
    selected_options: Vec<String>,
    images: Vec<ImageAttachment>,
    request_id: Option<String>,
    source: &str,
) -> String {
    let response = build_mcp_response(user_input, selected_options, images, request_id, source);
    response.to_string()
}

/// Build continue response
pub fn build_continue_response(request_id: Option<String>, source: &str) -> String {
    let continue_prompt = if let Ok(config) = crate::config::load_standalone_config() {
        config.reply_config.continue_prompt
    } else {
        "Please continue following best practices".to_string()
    };

    let response = build_mcp_response(Some(continue_prompt), vec![], vec![], request_id, source);
    response.to_string()
}
