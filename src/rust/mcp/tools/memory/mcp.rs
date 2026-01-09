use anyhow::Result;
use rmcp::model::{ErrorData as McpError, CallToolResult, Content};

use super::{MemoryManager, MemoryCategory};
use crate::mcp::{JiyiRequest, utils::{validate_project_path, project_path_error}};
use crate::log_debug;

/// Project memory management tool
#[derive(Clone)]
pub struct MemoryTool;

impl MemoryTool {
    pub async fn jiyi(
        request: JiyiRequest,
    ) -> Result<CallToolResult, McpError> {
        if let Err(e) = validate_project_path(&request.project_path) {
            return Err(project_path_error(format!(
                "Path validation failed: {}\nOriginal path: {}",
                e,
                request.project_path
            )).into());
        }

        let manager = MemoryManager::new(&request.project_path)
            .map_err(|e| McpError::internal_error(format!("Failed to create memory manager: {}", e), None))?;

        let mut index_hint = String::new();
        if is_sou_enabled() {
            if let Err(e) = try_trigger_background_index(&request.project_path).await {
                log_debug!("Background index trigger failed (not affecting memory): {}", e);
            } else {
                index_hint = "\n\nBackground code indexing started for this project.".to_string();
            }
        }

        let result = match request.action.as_str() {
            "store" | "记忆" => {
                if request.content.trim().is_empty() {
                    return Err(McpError::invalid_params("Missing content".to_string(), None));
                }

                let category = match request.category.as_str() {
                    "rule" => MemoryCategory::Rule,
                    "preference" => MemoryCategory::Preference,
                    "pattern" => MemoryCategory::Pattern,
                    "context" => MemoryCategory::Context,
                    _ => MemoryCategory::Context,
                };

                let id = manager.add_memory(&request.content, category)
                    .map_err(|e| McpError::internal_error(format!("Failed to add memory: {}", e), None))?;

                format!("Memory added, ID: {}\nContent: {}\nCategory: {:?}{}", id, request.content, category, index_hint)
            }
            "recall" | "回忆" => {
                let info = manager.get_project_info()
                    .map_err(|e| McpError::internal_error(format!("Failed to get project info: {}", e), None))?;
                format!("{}{}", info, index_hint)
            }
            _ => {
                return Err(McpError::invalid_params(
                    format!("Unknown action: {}", request.action),
                    None
                ));
            }
        };

        Ok(CallToolResult::success(vec![Content::text(result)]))
    }
}

/// Check if sou tool is enabled
fn is_sou_enabled() -> bool {
    match crate::config::load_standalone_config() {
        Ok(config) => config.mcp_config.tools.get("sou").copied().unwrap_or(false),
        Err(_) => false,
    }
}

/// Try to trigger background index
async fn try_trigger_background_index(project_root: &str) -> Result<()> {
    use super::super::acemcp::mcp::{get_initial_index_state, ensure_initial_index_background, InitialIndexState};

    let acemcp_config = super::super::acemcp::mcp::AcemcpTool::get_acemcp_config().await?;
    let initial_state = get_initial_index_state(project_root);

    if matches!(initial_state, InitialIndexState::Missing | InitialIndexState::Idle | InitialIndexState::Failed) {
        ensure_initial_index_background(&acemcp_config, project_root).await?;
        Ok(())
    } else {
        Ok(())
    }
}
