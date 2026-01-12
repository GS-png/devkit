use anyhow::Result;
use rmcp::model::{ErrorData as McpError, CallToolResult, Content};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, LazyLock};
use std::process::{Command, Stdio};
use std::fs;
use tokio::time::{sleep, Duration, Instant};

use crate::config::load_standalone_config;
use crate::mcp::{ZhiRequest, PopupRequest};
use crate::mcp::save_history_entry;
use crate::mcp::handlers::{find_ui_command, parse_mcp_response};
use crate::mcp::utils::{generate_request_id, popup_error};

fn should_skip_history_save(response_str: &str) -> bool {
    let s = response_str.trim();
    s.is_empty() || s == "CANCELLED" || s == "\"CANCELLED\""
}

fn try_save_history(request: Option<PopupRequest>, response_str: &str) {
    if should_skip_history_save(response_str) {
        return;
    }

    let s = response_str.trim();
    let response_value: serde_json::Value = serde_json::from_str(s)
        .unwrap_or_else(|_| serde_json::Value::String(s.to_string()));

    if let Err(e) = save_history_entry(request, response_value) {
        log::warn!("保存 MCP 历史记录失败: {}", e);
    }
}

fn load_request_from_file(path: &str) -> Option<PopupRequest> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str::<PopupRequest>(&s).ok())
}

/// Global task storage for async interaction
static PENDING_TASKS: LazyLock<Arc<Mutex<HashMap<String, PendingTask>>>> = 
    LazyLock::new(|| Arc::new(Mutex::new(HashMap::new())));

#[derive(Clone)]
struct PendingTask {
    request_file: String,
    response_file: String,
    status: TaskStatus,
    ui_pid: Option<u32>,
}

#[derive(Clone, Serialize, Deserialize)]
struct PersistedPendingTask {
    task_id: String,
    request_file: String,
    response_file: String,
    #[serde(default)]
    ui_pid: Option<u32>,
}

fn persisted_task_path() -> std::path::PathBuf {
    std::env::temp_dir().join("sanshu_mcp_pending_task.json")
}

fn load_persisted_task() -> Option<PersistedPendingTask> {
    let path = persisted_task_path();
    let content = fs::read_to_string(path).ok()?;
    let task = serde_json::from_str::<PersistedPendingTask>(&content).ok()?;
    if std::path::Path::new(&task.request_file).exists() {
        Some(task)
    } else {
        let _ = fs::remove_file(persisted_task_path());
        None
    }
}

fn persist_task(task: &PersistedPendingTask) -> Result<(), McpError> {
    let path = persisted_task_path();
    let content = serde_json::to_string(task)
        .map_err(|e| McpError::internal_error(e.to_string(), None))?;
    fs::write(path, content)
        .map_err(|e| McpError::internal_error(e.to_string(), None))?;
    Ok(())
}

fn clear_persisted_task_if_matches(task_id: &str) {
    if let Some(t) = load_persisted_task() {
        if t.task_id == task_id {
            let _ = fs::remove_file(persisted_task_path());
        }
    }
}

fn is_ui_process_running(pid: u32) -> bool {
    #[cfg(target_os = "linux")]
    {
        std::path::Path::new(&format!("/proc/{}", pid)).exists()
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = pid;
        true
    }
}

fn cleanup_task_files(task_id: &str, task: &PendingTask) {
    let _ = fs::remove_file(&task.request_file);
    let _ = fs::remove_file(&task.response_file);
    clear_persisted_task_if_matches(task_id);
}

#[derive(Clone, PartialEq)]
enum TaskStatus {
    Pending,
    Ready,
    Cancelled,
}

/// Development interaction tool with async support
#[derive(Clone)]
pub struct InteractionTool;

impl InteractionTool {
    /// Start interaction - returns immediately with task_id
    /// UI is launched in background, use get_result to poll for user input
    pub async fn prompt_start(
        request: ZhiRequest,
    ) -> Result<CallToolResult, McpError> {
        let existing_task_id = {
            let mut tasks = PENDING_TASKS.lock().unwrap();
            if tasks.is_empty() {
                if let Some(persisted) = load_persisted_task() {
                    if persisted.ui_pid.is_none() {
                        let _ = fs::remove_file(&persisted.request_file);
                        let _ = fs::remove_file(&persisted.response_file);
                        clear_persisted_task_if_matches(&persisted.task_id);
                    } else {
                        tasks.insert(
                            persisted.task_id.clone(),
                            PendingTask {
                                request_file: persisted.request_file,
                                response_file: persisted.response_file,
                                status: TaskStatus::Pending,
                                ui_pid: persisted.ui_pid,
                            },
                        );
                    }
                }
            }

            let stale_ids: Vec<String> = tasks
                .iter()
                .filter_map(|(task_id, task)| {
                    if task.status == TaskStatus::Pending {
                        if let Some(pid) = task.ui_pid {
                            if !is_ui_process_running(pid) {
                                return Some(task_id.clone());
                            }
                        }
                    }
                    None
                })
                .collect();

            for task_id in stale_ids {
                if let Some(task) = tasks.remove(&task_id) {
                    cleanup_task_files(&task_id, &task);
                }
            }

            tasks
                .iter()
                .find_map(|(task_id, task)| {
                    if task.status == TaskStatus::Pending {
                        Some(task_id.clone())
                    } else {
                        None
                    }
                })
        };

        if let Some(task_id) = existing_task_id {
            let response_text = format!(
                "An interactive dialog is already open. Task ID: {}\n\n\
                DO NOT call prompt again.\n\
                Wait for the user to finish their input in the dialog, then call get_result with task_id \"{}\".\n\n\
                If the dialog is not visible, ask the user to bring it to the front (or close it and retry).",
                task_id, task_id
            );
            return Ok(CallToolResult::success(vec![Content::text(response_text)]));
        }

        let task_id = generate_request_id();
        
        let popup_request = PopupRequest {
            id: task_id.clone(),
            message: request.message,
            predefined_options: if request.choices.is_empty() {
                None
            } else {
                Some(request.choices)
            },
            is_markdown: request.format,
            project_root_path: request.project_root_path,
        };

        // Create temp files
        let temp_dir = std::env::temp_dir();
        let request_file = temp_dir.join(format!("mcp_request_{}.json", task_id));
        let response_file = temp_dir.join(format!("mcp_response_{}.json", task_id));
        let ui_log_file = temp_dir.join(format!("sanshu_ui_mcp_{}.log", task_id));
        
        // Write request
        let request_json = serde_json::to_string_pretty(&popup_request)
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;
        fs::write(&request_file, request_json)
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;
        
        // Remove old response file
        let _ = fs::remove_file(&response_file);

        // Find UI command
        let command_path = find_ui_command()
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        let mut child = Command::new(&command_path)
            .env(
                "MCP_LOG_FILE",
                ui_log_file.to_string_lossy().to_string(),
            )
            .env(
                "RUST_LOG",
                std::env::var("RUST_LOG").unwrap_or_else(|_| "info".to_string()),
            )
            .arg("--mcp-request")
            .arg(request_file.to_string_lossy().to_string())
            .arg("--response-file")
            .arg(response_file.to_string_lossy().to_string())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| McpError::internal_error(format!("Failed to launch UI: {}", e), None))?;

        let ui_pid = Some(child.id());
        std::thread::spawn(move || {
            let _ = child.wait();
        });

        persist_task(&PersistedPendingTask {
            task_id: task_id.clone(),
            request_file: request_file.to_string_lossy().to_string(),
            response_file: response_file.to_string_lossy().to_string(),
            ui_pid,
        })?;

        // Store task info
        {
            let mut tasks = PENDING_TASKS.lock().unwrap();
            tasks.insert(
                task_id.clone(),
                PendingTask {
                    request_file: request_file.to_string_lossy().to_string(),
                    response_file: response_file.to_string_lossy().to_string(),
                    status: TaskStatus::Pending,
                    ui_pid,
                },
            );
        }

        // Return immediately with task_id and instructions
        // IMPORTANT: Tell AI to call get_result once and wait (no polling)
        let response_text = format!(
            "Interactive dialog opened. Task ID: {}\n\
            UI executable: {}\n\n\
            USER IS NOW VIEWING THE DIALOG\n\n\
            NEXT STEP: Call get_result ONCE with task_id \"{}\".\n\
            get_result will WAIT until the user submits/cancels in the dialog.\n\
            DO NOT poll or call get_result repeatedly.",
            task_id, command_path, task_id
        );

        Ok(CallToolResult::success(vec![Content::text(response_text)]))
    }

    pub async fn prompt_sync(
        request: ZhiRequest,
    ) -> Result<CallToolResult, McpError> {
        let existing_task_id = {
            let mut tasks = PENDING_TASKS.lock().unwrap();
            if tasks.is_empty() {
                if let Some(persisted) = load_persisted_task() {
                    if persisted.ui_pid.is_none() {
                        let _ = fs::remove_file(&persisted.request_file);
                        let _ = fs::remove_file(&persisted.response_file);
                        clear_persisted_task_if_matches(&persisted.task_id);
                    } else {
                    tasks.insert(
                        persisted.task_id.clone(),
                        PendingTask {
                            request_file: persisted.request_file,
                            response_file: persisted.response_file,
                            status: TaskStatus::Pending,
                            ui_pid: persisted.ui_pid,
                        },
                    );
                    }
                }
            }

            let stale_ids: Vec<String> = tasks
                .iter()
                .filter_map(|(task_id, task)| {
                    if task.status == TaskStatus::Pending {
                        if let Some(pid) = task.ui_pid {
                            if !is_ui_process_running(pid) {
                                return Some(task_id.clone());
                            }
                        }
                    }
                    None
                })
                .collect();

            for task_id in stale_ids {
                if let Some(task) = tasks.remove(&task_id) {
                    cleanup_task_files(&task_id, &task);
                }
            }

            tasks
                .iter()
                .find_map(|(task_id, task)| {
                    if task.status == TaskStatus::Pending {
                        Some(task_id.clone())
                    } else {
                        None
                    }
                })
        };

        if let Some(task_id) = existing_task_id {
            return Self::get_result(task_id).await;
        }

        let task_id = generate_request_id();

        let popup_request = PopupRequest {
            id: task_id.clone(),
            message: request.message,
            predefined_options: if request.choices.is_empty() {
                None
            } else {
                Some(request.choices)
            },
            is_markdown: request.format,
            project_root_path: request.project_root_path,
        };

        let temp_dir = std::env::temp_dir();
        let request_file = temp_dir.join(format!("mcp_request_{}.json", task_id));
        let response_file = temp_dir.join(format!("mcp_response_{}.json", task_id));
        let ui_log_file = temp_dir.join(format!("sanshu_ui_mcp_{}.log", task_id));

        let request_json = serde_json::to_string_pretty(&popup_request)
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;
        fs::write(&request_file, request_json)
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        let _ = fs::remove_file(&response_file);

        let command_path = find_ui_command()
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        let mut child = Command::new(&command_path)
            .env(
                "MCP_LOG_FILE",
                ui_log_file.to_string_lossy().to_string(),
            )
            .env(
                "RUST_LOG",
                std::env::var("RUST_LOG").unwrap_or_else(|_| "info".to_string()),
            )
            .arg("--mcp-request")
            .arg(request_file.to_string_lossy().to_string())
            .arg("--response-file")
            .arg(response_file.to_string_lossy().to_string())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| McpError::internal_error(format!("Failed to launch UI: {}", e), None))?;

        let ui_pid = Some(child.id());
        std::thread::spawn(move || {
            let _ = child.wait();
        });

        persist_task(&PersistedPendingTask {
            task_id: task_id.clone(),
            request_file: request_file.to_string_lossy().to_string(),
            response_file: response_file.to_string_lossy().to_string(),
            ui_pid,
        })?;

        {
            let mut tasks = PENDING_TASKS.lock().unwrap();
            tasks.insert(
                task_id.clone(),
                PendingTask {
                    request_file: request_file.to_string_lossy().to_string(),
                    response_file: response_file.to_string_lossy().to_string(),
                    status: TaskStatus::Pending,
                    ui_pid,
                },
            );
        }

        let task = PendingTask {
            request_file: request_file.to_string_lossy().to_string(),
            response_file: response_file.to_string_lossy().to_string(),
            status: TaskStatus::Pending,
            ui_pid,
        };

        let _ = task;
        Self::get_result(task_id).await
    }

    /// Get result of a pending interaction task
    /// Returns user input if ready, or status if still waiting
    pub async fn get_result(task_id: String) -> Result<CallToolResult, McpError> {
        let task = {
            let mut tasks = PENDING_TASKS.lock().unwrap();

            if let Some(task) = tasks.get(&task_id).cloned() {
                Some(task)
            } else if let Some(persisted) = load_persisted_task() {
                if persisted.task_id == task_id {
                    let task = PendingTask {
                        request_file: persisted.request_file,
                        response_file: persisted.response_file,
                        status: TaskStatus::Pending,
                        ui_pid: persisted.ui_pid,
                    };
                    tasks.insert(task_id.clone(), task.clone());
                    Some(task)
                } else {
                    None
                }
            } else {
                None
            }
        };

        match task {
            None => {
                Err(McpError::invalid_params(
                    format!("Task not found: {}. Make sure you called prompt first.", task_id),
                    None
                ))
            }
             Some(task) => {
                let max_wait_ms_raw: u64 = std::env::var("SANSHU_GET_RESULT_WAIT_MS")
                    .or_else(|_| std::env::var("MCP_GET_RESULT_WAIT_MS"))
                    .ok()
                    .and_then(|s| s.parse::<u64>().ok())
                    .unwrap_or_else(|| {
                        load_standalone_config()
                            .ok()
                            .map(|c| c.mcp_config.interaction_wait_ms)
                            .unwrap_or(0)
                    });
                let max_wait_ms: Option<u64> = if max_wait_ms_raw == 0 {
                    None
                } else {
                    Some(max_wait_ms_raw)
                };
                let step_ms: u64 = 200;

                let start = Instant::now();
                loop {
                    if let Ok(content) = fs::read_to_string(&task.response_file) {
                        if !content.trim().is_empty() {
                            let request = load_request_from_file(&task.request_file);
                            try_save_history(request, &content);
                            let result = parse_mcp_response(&content)?;

                            let _ = fs::remove_file(&task.request_file);
                            let _ = fs::remove_file(&task.response_file);
                            clear_persisted_task_if_matches(&task_id);
                            {
                                let mut tasks = PENDING_TASKS.lock().unwrap();
                                tasks.remove(&task_id);
                            }

                            return Ok(CallToolResult::success(result));
                        }
                    }

                    if let Some(pid) = task.ui_pid {
                        if !is_ui_process_running(pid) {
                            cleanup_task_files(&task_id, &task);
                            {
                                let mut tasks = PENDING_TASKS.lock().unwrap();
                                tasks.remove(&task_id);
                            }
                            return Ok(CallToolResult::success(vec![Content::text(
                                "Operation cancelled by user".to_string(),
                            )]));
                        }
                    } else {
                        cleanup_task_files(&task_id, &task);
                        {
                            let mut tasks = PENDING_TASKS.lock().unwrap();
                            tasks.remove(&task_id);
                        }
                        return Ok(CallToolResult::success(vec![Content::text(
                            "Operation cancelled by user".to_string(),
                        )]));
                    }

                    if let Some(max_wait_ms) = max_wait_ms {
                        if start.elapsed() >= Duration::from_millis(max_wait_ms) {
                            break;
                        }
                    }
                    sleep(Duration::from_millis(step_ms)).await;
                }
                
                // Still waiting for user input
                let waited_ms = start.elapsed().as_millis();
                let max_wait_display = max_wait_ms
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "infinite".to_string());
                let waiting_msg = format!(
                    "Status: WAITING - User has not submitted yet\n\
                    Task ID: {}\n\n\
                    Long-poll waited: {}ms (max {})\n\n\
                    The user is still working on their response.\n\
                    Ask the user in chat: \"Have you finished your input?\"\n\
                    DO NOT call prompt again while waiting.\n\
                    DO NOT call get_result again until the user confirms.",
                    task_id,
                    waited_ms,
                    max_wait_display
                );
                Ok(CallToolResult {
                    content: vec![Content::text(waiting_msg)],
                    is_error: Some(false),
                    meta: None,
                    structured_content: None,
                })
            }
        }
    }

    /// Original blocking implementation (kept for compatibility)
    pub async fn zhi(
        request: ZhiRequest,
    ) -> Result<CallToolResult, McpError> {
        let popup_request = PopupRequest {
            id: generate_request_id(),
            message: request.message,
            predefined_options: if request.choices.is_empty() {
                None
            } else {
                Some(request.choices)
            },
            is_markdown: request.format,
            project_root_path: request.project_root_path,
        };

        match crate::mcp::handlers::create_tauri_popup(&popup_request) {
            Ok(response) => {
                try_save_history(Some(popup_request.clone()), &response);
                let content = parse_mcp_response(&response)?;
                Ok(CallToolResult::success(content))
            }
            Err(e) => {
                Err(popup_error(e.to_string()).into())
            }
        }
    }
}
