use tauri::{AppHandle, State};

use crate::config::{AppState, save_config};
use crate::network::proxy::{ProxyDetector, ProxyInfo, ProxyType};
use super::AcemcpTool;
use super::types::{AcemcpRequest, ProjectIndexStatus, ProjectsIndexStatus, ProjectFilesStatus, DetectedProxy, ProxySpeedTestResult, SpeedTestMetric};
use reqwest;

#[derive(Debug, serde::Deserialize)]
pub struct SaveAcemcpConfigArgs {
    #[serde(alias = "baseUrl", alias = "base_url")]
    pub base_url: String,
    #[serde(alias = "token", alias = "_token")]
    pub token: String,
    #[serde(alias = "batchSize", alias = "batch_size")]
    pub batch_size: u32,
    #[serde(alias = "maxLinesPerBlob", alias = "_max_lines_per_blob")]
    pub max_lines_per_blob: u32,
    #[serde(alias = "textExtensions", alias = "_text_extensions")]
    pub text_extensions: Vec<String>,
    #[serde(alias = "excludePatterns", alias = "_exclude_patterns")]
    pub exclude_patterns: Vec<String>,
    #[serde(alias = "watchDebounceMs", alias = "watch_debounce_ms")]
    pub watch_debounce_ms: Option<u64>, // 文件监听防抖延迟（毫秒）
    // 代理配置
    #[serde(alias = "proxyEnabled", alias = "proxy_enabled")]
    pub proxy_enabled: Option<bool>,
    #[serde(alias = "proxyHost", alias = "proxy_host")]
    pub proxy_host: Option<String>,
    #[serde(alias = "proxyPort", alias = "proxy_port")]
    pub proxy_port: Option<u16>,
    #[serde(alias = "proxyType", alias = "proxy_type")]
    pub proxy_type: Option<String>,
    #[serde(alias = "proxyUsername", alias = "proxy_username")]
    pub proxy_username: Option<String>,
    #[serde(alias = "proxyPassword", alias = "proxy_password")]
    pub proxy_password: Option<String>,
}


#[tauri::command]
pub async fn save_acemcp_config(
    args: SaveAcemcpConfigArgs,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<(), String> {
    // 规范化 base_url：补充协议（如缺失）并去除末尾斜杠，防止URL拼接时出现双斜杠
    let mut base_url = args.base_url.trim().to_string();
    if !(base_url.starts_with("http://") || base_url.starts_with("https://")) {
        base_url = format!("http://{}", base_url);
        log::warn!("BASE_URL 缺少协议，已自动补全为: {}", base_url);
    }
    // 去除末尾的所有斜杠，确保URL格式统一
    while base_url.ends_with('/') {
        base_url.pop();
    }
    log::info!("规范化后的 BASE_URL: {}", base_url);

    {
        let mut config = state
            .config
            .lock()
            .map_err(|e| format!("获取配置失败: {}", e))?;

        config.mcp_config.acemcp_base_url = Some(base_url.clone());
        config.mcp_config.acemcp_token = Some(args.token.clone());
        config.mcp_config.acemcp_batch_size = Some(args.batch_size);
        config.mcp_config.acemcp_max_lines_per_blob = Some(args.max_lines_per_blob);
        config.mcp_config.acemcp_text_extensions = Some(args.text_extensions.clone());
        config.mcp_config.acemcp_exclude_patterns = Some(args.exclude_patterns.clone());
        config.mcp_config.acemcp_watch_debounce_ms = args.watch_debounce_ms;
        // 保存代理配置
        config.mcp_config.acemcp_proxy_enabled = args.proxy_enabled;
        config.mcp_config.acemcp_proxy_host = args.proxy_host.clone();
        config.mcp_config.acemcp_proxy_port = args.proxy_port;
        config.mcp_config.acemcp_proxy_type = args.proxy_type.clone();
        config.mcp_config.acemcp_proxy_username = args.proxy_username.clone();
        config.mcp_config.acemcp_proxy_password = args.proxy_password.clone();
    }

    save_config(&state, &app)
        .await
        .map_err(|e| format!("保存配置失败: {}", e))?;

    Ok(())
}

#[derive(Debug, serde::Deserialize)]
pub struct TestAcemcpArgs {
    #[serde(alias = "baseUrl", alias = "base_url")]
    pub base_url: String,
    #[serde(alias = "token", alias = "_token")]
    pub token: String,
}

#[derive(Debug, serde::Serialize)]
pub struct TestConnectionResult {
    pub success: bool,
    pub message: String,
}

#[tauri::command]
pub async fn test_acemcp_connection(
    args: TestAcemcpArgs,
    state: State<'_, AppState>,
) -> Result<TestConnectionResult, String> {
    // 获取配置并立即释放锁
    let (
        effective_base_url,
        effective_token,
        proxy_enabled,
        proxy_host,
        proxy_port,
        proxy_type,
        proxy_username,
        proxy_password,
    ) = {
        let config = state.config
            .lock()
            .map_err(|e| format!("获取配置失败: {}", e))?;
        
        let base_url = config.mcp_config.acemcp_base_url.as_ref().unwrap_or(&args.base_url).clone();
        let token = config.mcp_config.acemcp_token.as_ref().unwrap_or(&args.token).clone();

        // 代理配置（连接测试也需要遵循“所有 ACE 通信走代理”的要求）
        let proxy_enabled = config.mcp_config.acemcp_proxy_enabled.unwrap_or(false);
        let proxy_host = config.mcp_config.acemcp_proxy_host.clone().unwrap_or_else(|| "127.0.0.1".to_string());
        let proxy_port = config.mcp_config.acemcp_proxy_port.unwrap_or(7890);
        let proxy_type = config.mcp_config.acemcp_proxy_type.clone().unwrap_or_else(|| "http".to_string());
        let proxy_username = config.mcp_config.acemcp_proxy_username.clone();
        let proxy_password = config.mcp_config.acemcp_proxy_password.clone();

        (
            base_url,
            token,
            proxy_enabled,
            proxy_host,
            proxy_port,
            proxy_type,
            proxy_username,
            proxy_password,
        )
    };
    
    // 验证 URL 格式
    if !effective_base_url.starts_with("http://") && !effective_base_url.starts_with("https://") {
        let msg = "无效的API端点URL格式，必须以 http:// 或 https:// 开头".to_string();
        return Ok(TestConnectionResult {
            success: false,
            message: msg,
        });
    }
    
    // 验证 token
    if effective_token.trim().is_empty() {
        let msg = "认证令牌不能为空".to_string();
        return Ok(TestConnectionResult {
            success: false,
            message: msg,
        });
    }
    
    // 规范化 base_url
    let normalized_url = if effective_base_url.ends_with('/') {
        effective_base_url[..effective_base_url.len() - 1].to_string()
    } else {
        effective_base_url.clone()
    };
    
    // 实际测试连接 - 发送一个简单的健康检查请求
    let mut client_builder = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10));

    // 如果启用代理，则连接测试也走代理（避免“设置了代理但测试仍失败”的误导）
    if proxy_enabled {
        // 校验代理类型，避免拼接出无效 URL
        match proxy_type.as_str() {
            "http" | "https" | "socks5" => {}
            other => {
                return Ok(TestConnectionResult {
                    success: false,
                    message: format!("不支持的代理类型: {}（仅支持 http/https/socks5）", other),
                });
            }
        }

        let proxy_url = format!("{}://{}:{}", proxy_type, proxy_host, proxy_port);
        let mut reqwest_proxy = reqwest::Proxy::all(&proxy_url)
            .map_err(|e| format!("创建代理失败: {}", e))?;

        // 代理认证（Basic Auth）
        if let Some(username) = proxy_username.as_deref() {
            let username = username.trim();
            if !username.is_empty() {
                let password = proxy_password.as_deref().unwrap_or("");
                reqwest_proxy = reqwest_proxy.basic_auth(username, password);
            }
        }

        client_builder = client_builder.proxy(reqwest_proxy);
    }

    let client = client_builder
        .build()
        .map_err(|e| format!("创建 HTTP 客户端失败: {}", e))?;
    
    // 尝试访问一个常见的端点（如果存在健康检查端点）
    let test_url = format!("{}/health", normalized_url);
    
    match client
        .get(&test_url)
        .header(reqwest::header::AUTHORIZATION, format!("Bearer {}", effective_token))
        .send()
        .await
    {
        Ok(response) => {
            let status = response.status();
            
            if status.is_success() {
                let msg = format!("连接测试成功！API 端点响应正常 (HTTP {})", status.as_u16());
                return Ok(TestConnectionResult {
                    success: true,
                    message: msg,
                });
            }
        }
        Err(_) => {
            // 健康检查端点可能不存在，继续测试实际 API 端点
        }
    }
    
    // 如果健康检查失败，尝试测试实际的代码库检索端点
    let search_url = format!("{}/agents/codebase-retrieval", normalized_url);
    
    // 发送一个最小的测试请求
    let test_payload = serde_json::json!({
        "information_request": "test",
        "blobs": {"checkpoint_id": null, "added_blobs": [], "deleted_blobs": []},
        "dialog": [],
        "max_output_length": 0,
        "disable_codebase_retrieval": false,
        "enable_commit_retrieval": false,
    });
    
    match client
        .post(&search_url)
        .header(reqwest::header::AUTHORIZATION, format!("Bearer {}", effective_token))
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .json(&test_payload)
        .send()
        .await
    {
        Ok(response) => {
            let status = response.status();
            
            if status.is_success() {
                let msg = format!("连接测试成功！API 端点响应正常 (HTTP {})", status.as_u16());
                Ok(TestConnectionResult {
                    success: true,
                    message: msg,
                })
            } else {
                let body = response.text().await.unwrap_or_default();
                let msg = format!("API 端点返回错误状态: {} {}", status.as_u16(), status.as_str());
                Ok(TestConnectionResult {
                    success: false,
                    message: format!("{} - 响应: {}", msg, if body.len() > 200 { format!("{}...", &body[..200]) } else { body }),
                })
            }
        }
        Err(e) => {
            let msg = format!("连接失败: {}", e);
            Ok(TestConnectionResult {
                success: false,
                message: msg,
            })
        }
    }
}

/// 读取日志文件内容
#[tauri::command]
pub async fn read_acemcp_logs(_state: State<'_, AppState>) -> Result<Vec<String>, String> {
    // 使用 dirs::config_dir() 获取系统配置目录，确保跨平台兼容性
    // Windows: C:\Users\<用户>\AppData\Roaming\sanshu\log\acemcp.log
    // Linux: ~/.config/sanshu/log/acemcp.log
    // macOS: ~/Library/Application Support/sanshu/log/acemcp.log
    let config_dir = dirs::config_dir()
        .ok_or_else(|| "无法获取系统配置目录，请检查操作系统环境".to_string())?;

    let log_path = config_dir.join("sanshu").join("log").join("acemcp.log");

    // 确保日志目录存在
    if let Some(log_dir) = log_path.parent() {
        if !log_dir.exists() {
            std::fs::create_dir_all(log_dir)
                .map_err(|e| format!("创建日志目录失败: {} (路径: {})", e, log_dir.display()))?;
        }
    }

    // 如果日志文件不存在，返回空数组
    if !log_path.exists() {
        return Ok(vec![]);
    }

    // 读取日志文件内容
    let content = std::fs::read_to_string(&log_path)
        .map_err(|e| format!("读取日志文件失败: {} (路径: {})", e, log_path.display()))?;

    // 返回最近1000行日志
    let all_lines: Vec<String> = content
        .lines()
        .map(|s| s.to_string())
        .collect();

    // 只返回最后1000行
    let lines: Vec<String> = if all_lines.len() > 1000 {
        let skip_count = all_lines.len() - 1000;
        all_lines.into_iter().skip(skip_count).collect()
    } else {
        all_lines
    };

    Ok(lines)
}

#[tauri::command]
pub async fn clear_acemcp_cache(_state: State<'_, AppState>) -> Result<String, String> {
    // 使用 dirs::home_dir() 获取用户主目录，确保跨平台兼容性
    // 如果获取失败，降级到当前目录（与项目中 home_projects_file() 保持一致）
    let home = dirs::home_dir().unwrap_or_else(|| std::path::PathBuf::from("."));
    let cache_dir = home.join(".acemcp").join("data");

    // 如果缓存目录存在，先删除
    if cache_dir.exists() {
        std::fs::remove_dir_all(&cache_dir)
            .map_err(|e| format!("删除缓存目录失败: {} (路径: {})", e, cache_dir.display()))?;
    }

    // 重新创建缓存目录
    std::fs::create_dir_all(&cache_dir)
        .map_err(|e| format!("创建缓存目录失败: {} (路径: {})", e, cache_dir.display()))?;

    let cache_path = cache_dir.to_string_lossy().to_string();
    log::info!("acemcp缓存已清除: {}", cache_path);
    Ok(cache_path)
}

#[derive(Debug, serde::Serialize)]
pub struct AcemcpConfigResponse {
    pub base_url: Option<String>,
    pub token: Option<String>,
    pub batch_size: u32,
    pub max_lines_per_blob: u32,
    pub text_extensions: Vec<String>,
    pub exclude_patterns: Vec<String>,
    pub watch_debounce_ms: u64, // 文件监听防抖延迟（毫秒），默认 180000 (3分钟)
    // 代理配置
    pub proxy_enabled: bool,
    pub proxy_host: String,
    pub proxy_port: u16,
    pub proxy_type: String,
    pub proxy_username: String,
    pub proxy_password: String,
}

#[tauri::command]
pub async fn get_acemcp_config(state: State<'_, AppState>) -> Result<AcemcpConfigResponse, String> {
    let config = state.config
        .lock()
        .map_err(|e| format!("获取配置失败: {}", e))?;
    Ok(AcemcpConfigResponse {
        base_url: config.mcp_config.acemcp_base_url.clone(),
        token: config.mcp_config.acemcp_token.clone(),
        batch_size: config.mcp_config.acemcp_batch_size.unwrap_or(10),
        max_lines_per_blob: config.mcp_config.acemcp_max_lines_per_blob.unwrap_or(800),
        // 默认文件扩展名列表（与前端 McpToolsTab.vue 保持一致）
        // 用户首次打开设置界面时，所有扩展名默认全部勾选
        text_extensions: config.mcp_config.acemcp_text_extensions.clone().unwrap_or_else(|| {
            vec![
                ".py".to_string(), ".js".to_string(), ".ts".to_string(),
                ".jsx".to_string(), ".tsx".to_string(), ".java".to_string(),
                ".go".to_string(), ".rs".to_string(), ".cpp".to_string(),
                ".c".to_string(), ".h".to_string(), ".hpp".to_string(),
                ".cs".to_string(), ".rb".to_string(), ".php".to_string(),
                ".md".to_string(), ".txt".to_string(), ".json".to_string(),
                ".yaml".to_string(), ".yml".to_string(), ".toml".to_string(),
                ".xml".to_string(), ".html".to_string(), ".css".to_string(),
                ".scss".to_string(), ".sql".to_string(), ".sh".to_string(),
                ".bash".to_string()
            ]
        }),
        exclude_patterns: config.mcp_config.acemcp_exclude_patterns.clone().unwrap_or_else(|| {
            vec!["node_modules".to_string(), ".git".to_string(), "target".to_string(), "dist".to_string()]
        }),
        watch_debounce_ms: config.mcp_config.acemcp_watch_debounce_ms.unwrap_or(180_000),
        // 代理配置
        proxy_enabled: config.mcp_config.acemcp_proxy_enabled.unwrap_or(false),
        proxy_host: config.mcp_config.acemcp_proxy_host.clone().unwrap_or_else(|| "127.0.0.1".to_string()),
        proxy_port: config.mcp_config.acemcp_proxy_port.unwrap_or(7890),
        proxy_type: config.mcp_config.acemcp_proxy_type.clone().unwrap_or_else(|| "http".to_string()),
        proxy_username: config.mcp_config.acemcp_proxy_username.clone().unwrap_or_default(),
        proxy_password: config.mcp_config.acemcp_proxy_password.clone().unwrap_or_default(),
    })
}

#[derive(Debug, serde::Serialize)]
pub struct DebugSearchResult {
    /// 搜索是否成功
    pub success: bool,
    /// 搜索结果文本
    pub result: Option<String>,
    /// 错误信息
    pub error: Option<String>,
    /// 请求发送时间 ISO8601 格式
    pub request_time: String,
    /// 响应接收时间 ISO8601 格式
    pub response_time: String,
    /// 总耗时（毫秒）
    pub total_duration_ms: u64,
    /// 搜索结果数量
    pub result_count: Option<usize>,
    /// 项目路径
    pub project_path: String,
    /// 查询语句
    pub query: String,
}

/// 纯 Rust 的调试命令：直接执行 acemcp 搜索，返回结果及耗时统计
#[tauri::command]
pub async fn debug_acemcp_search(
    project_root_path: String,
    query: String,
    _app: AppHandle,
) -> Result<DebugSearchResult, String> {
    use std::time::Instant;
    
    // 记录请求开始时间
    let request_time = chrono::Utc::now();
    let request_time_str = request_time.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string();
    let start_instant = Instant::now();
    
    let req = AcemcpRequest { 
        project_root_path: project_root_path.clone(), 
        query: query.clone() 
    };
    
    // 调用搜索函数（日志会通过 log crate 输出到日志文件）
    log::info!("[调试搜索] 开始执行: project={}, query={}", project_root_path, query);
    let search_result = AcemcpTool::search_context(req).await;
    
    // 记录响应接收时间
    let response_time = chrono::Utc::now();
    let response_time_str = response_time.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string();
    let total_duration_ms = start_instant.elapsed().as_millis() as u64;
    
    log::info!("[调试搜索] 执行完成: 耗时 {}ms", total_duration_ms);
    
    match search_result {
        Ok(result) => {
            let mut result_text = String::new();
            let mut result_count: Option<usize> = None;
            
            if let Ok(val) = serde_json::to_value(&result) {
                if let Some(arr) = val.get("content").and_then(|v| v.as_array()) {
                    result_count = Some(arr.len());
                    for item in arr {
                        if item.get("type").and_then(|t| t.as_str()) == Some("text") {
                            if let Some(txt) = item.get("text").and_then(|t| t.as_str()) {
                                result_text.push_str(txt);
                            }
                        }
                    }
                }
            }
            
            Ok(DebugSearchResult {
                success: true,
                result: Some(result_text),
                error: None,
                request_time: request_time_str,
                response_time: response_time_str,
                total_duration_ms,
                result_count,
                project_path: project_root_path,
                query,
            })
        }
        Err(e) => {
            let error_msg = format!("执行失败: {}", e);
            log::error!("[调试搜索] 错误: {}", error_msg);
            
            Ok(DebugSearchResult {
                success: false,
                result: None,
                error: Some(error_msg),
                request_time: request_time_str,
                response_time: response_time_str,
                total_duration_ms,
                result_count: None,
                project_path: project_root_path,
                query,
            })
        }
    }
}


/// 执行acemcp工具
#[tauri::command]
pub async fn execute_acemcp_tool(
    tool_name: String,
    arguments: serde_json::Value,
) -> Result<serde_json::Value, String> {
    match tool_name.as_str() {
        "search_context" => {
            // 解析参数
            let project_root_path = arguments.get("project_root_path")
                .and_then(|v| v.as_str())
                .ok_or_else(|| "缺少project_root_path参数".to_string())?
                .to_string();
            
            let query = arguments.get("query")
                .and_then(|v| v.as_str())
                .ok_or_else(|| "缺少query参数".to_string())?
                .to_string();
            
            // 执行搜索
            let req = AcemcpRequest { project_root_path, query };
            match AcemcpTool::search_context(req).await {
                Ok(result) => {
                    // 转换结果为JSON
                    if let Ok(val) = serde_json::to_value(&result) {
                        Ok(serde_json::json!({
                            "status": "success",
                            "result": val
                        }))
                    } else {
                        Err("结果序列化失败".to_string())
                    }
                }
                Err(e) => Ok(serde_json::json!({
                    "status": "error",
                    "error": e.to_string()
                })),
            }
        }
        _ => Err(format!("未知的工具: {}", tool_name)),
    }
}

/// 获取指定项目的索引状态
#[tauri::command]
pub fn get_acemcp_index_status(project_root_path: String) -> Result<ProjectIndexStatus, String> {
    Ok(AcemcpTool::get_index_status(project_root_path))
}

/// 获取所有项目的索引状态
#[tauri::command]
pub fn get_all_acemcp_index_status() -> Result<ProjectsIndexStatus, String> {
    Ok(AcemcpTool::get_all_index_status())
}

/// 获取指定项目内所有可索引文件的索引状态，用于前端构建文件树
#[tauri::command]
pub async fn get_acemcp_project_files_status(
    project_root_path: String,
) -> Result<ProjectFilesStatus, String> {
    AcemcpTool::get_project_files_status(project_root_path)
        .await
        .map_err(|e| e.to_string())
}

/// 手动触发索引更新
#[tauri::command]
pub async fn trigger_acemcp_index_update(project_root_path: String) -> Result<String, String> {
    AcemcpTool::trigger_index_update(project_root_path)
        .await
        .map_err(|e| e.to_string())
}

/// 获取全局自动索引开关状态
#[tauri::command]
pub fn get_auto_index_enabled() -> Result<bool, String> {
    let watcher_manager = super::watcher::get_watcher_manager();
    Ok(watcher_manager.is_auto_index_enabled())
}

/// 设置全局自动索引开关
#[tauri::command]
pub async fn set_auto_index_enabled(
    enabled: bool,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<(), String> {
    let watcher_manager = super::watcher::get_watcher_manager();
    watcher_manager.set_auto_index_enabled(enabled);

    // 持久化到配置，确保跨重启生效
    {
        let mut config = state
            .config
            .lock()
            .map_err(|e| format!("获取配置失败: {}", e))?;
        config.mcp_config.acemcp_auto_index_enabled = Some(enabled);
    }

    save_config(&state, &app)
        .await
        .map_err(|e| format!("保存配置失败: {}", e))?;
    Ok(())
}

/// 获取当前正在监听的项目列表
#[tauri::command]
pub fn get_watching_projects() -> Result<Vec<String>, String> {
    let watcher_manager = super::watcher::get_watcher_manager();
    Ok(watcher_manager.get_watching_projects())
}

/// 检查指定项目是否正在监听
#[tauri::command]
pub fn is_project_watching(project_root_path: String) -> Result<bool, String> {
    let watcher_manager = super::watcher::get_watcher_manager();
    Ok(watcher_manager.is_watching(&project_root_path))
}

/// 启动项目文件监听
/// 从配置中读取防抖延迟参数
#[tauri::command]
pub async fn start_project_watching(
    project_root_path: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    // 从配置中读取防抖延迟
    let debounce_ms = {
        let config = state.config.lock().map_err(|e| format!("获取配置失败: {}", e))?;
        config.mcp_config.acemcp_watch_debounce_ms
    };
    
    // 获取 acemcp 配置
    let acemcp_config = super::AcemcpTool::get_acemcp_config()
        .await
        .map_err(|e| format!("获取 acemcp 配置失败: {}", e))?;
    
    log::info!("启动项目监听: path={}, debounce_ms={:?}", project_root_path, debounce_ms);
    
    // 启动监听
    let watcher_manager = super::watcher::get_watcher_manager();
    watcher_manager.start_watching(project_root_path, acemcp_config, debounce_ms)
        .await
        .map_err(|e| format!("启动监听失败: {}", e))
}

/// 停止监听指定项目
#[tauri::command]
pub fn stop_project_watching(project_root_path: String) -> Result<(), String> {
    let watcher_manager = super::watcher::get_watcher_manager();
    watcher_manager.stop_watching(&project_root_path)
        .map_err(|e| e.to_string())
}

/// 停止所有项目监听
#[tauri::command]
pub fn stop_all_watching() -> Result<(), String> {
    let watcher_manager = super::watcher::get_watcher_manager();
    watcher_manager.stop_all();
    Ok(())
}

/// 删除指定项目的索引记录
/// 同时清理 projects.json 和 projects_status.json 中的数据
#[tauri::command]
pub async fn remove_acemcp_project_index(project_root_path: String) -> Result<String, String> {
    use std::path::PathBuf;
    use std::fs;
    use std::collections::HashMap;

    // 辅助函数：规范化路径 key（去除扩展路径前缀，统一使用正斜杠）
    fn normalize_path_key(path: &str) -> String {
        let mut normalized = path.to_string();
        // 去除 Windows 扩展长度路径前缀
        if normalized.starts_with("\\\\?\\") {
            normalized = normalized[4..].to_string();
        } else if normalized.starts_with("//?/") {
            normalized = normalized[4..].to_string();
        }
        // 统一使用正斜杠
        normalized.replace('\\', "/")
    }

    // 规范化传入的路径
    let normalized_root = normalize_path_key(&project_root_path);

    log::info!("[remove_acemcp_project_index] 开始删除项目索引记录");
    log::info!("[remove_acemcp_project_index] 原始路径: {}", project_root_path);
    log::info!("[remove_acemcp_project_index] 规范化后路径: {}", normalized_root);

    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    let data_dir = home.join(".acemcp").join("data");

    let mut projects_deleted = false;
    let mut status_deleted = false;

    // 1. 从 projects.json 中删除项目的 blob 列表
    let projects_path = data_dir.join("projects.json");
    if projects_path.exists() {
        if let Ok(data) = fs::read_to_string(&projects_path) {
            if let Ok(mut projects) = serde_json::from_str::<HashMap<String, Vec<String>>>(&data) {
                // 调试日志：输出现有的 key 列表
                let existing_keys: Vec<&String> = projects.keys().collect();
                log::info!("[remove_acemcp_project_index] projects.json 中现有项目: {:?}", existing_keys);
                
                // 遍历查找匹配的 key（对每个 key 也进行规范化后比较）
                let key_to_remove: Option<String> = projects.keys()
                    .find(|k| normalize_path_key(k) == normalized_root)
                    .cloned();
                
                if let Some(key) = key_to_remove {
                    log::info!("[remove_acemcp_project_index] 找到匹配的 key: {}", key);
                    projects.remove(&key);
                    if let Ok(new_data) = serde_json::to_string_pretty(&projects) {
                        let _ = fs::write(&projects_path, new_data);
                        log::info!("[remove_acemcp_project_index] ✓ 已从 projects.json 删除项目: {}", key);
                        projects_deleted = true;
                    }
                } else {
                    log::warn!("[remove_acemcp_project_index] ✗ 在 projects.json 中未找到匹配的项目，规范化路径: {}", normalized_root);
                }
            }
        }
    } else {
        log::warn!("[remove_acemcp_project_index] projects.json 文件不存在: {:?}", projects_path);
    }

    // 2. 从 projects_status.json 中删除项目状态
    let status_path = data_dir.join("projects_status.json");
    if status_path.exists() {
        if let Ok(data) = fs::read_to_string(&status_path) {
            if let Ok(mut status) = serde_json::from_str::<serde_json::Value>(&data) {
                if let Some(projects) = status.get_mut("projects") {
                    if let Some(map) = projects.as_object_mut() {
                        // 调试日志：输出现有的 key 列表
                        let existing_keys: Vec<&String> = map.keys().collect();
                        log::info!("[remove_acemcp_project_index] projects_status.json 中现有项目: {:?}", existing_keys);
                        
                        // 遍历查找匹配的 key（对每个 key 也进行规范化后比较）
                        let key_to_remove: Option<String> = map.keys()
                            .find(|k| normalize_path_key(k) == normalized_root)
                            .cloned();
                        
                        if let Some(key) = key_to_remove {
                            log::info!("[remove_acemcp_project_index] 找到匹配的 key: {}", key);
                            map.remove(&key);
                            if let Ok(new_data) = serde_json::to_string_pretty(&status) {
                                let _ = fs::write(&status_path, new_data);
                                log::info!("[remove_acemcp_project_index] ✓ 已从 projects_status.json 删除项目: {}", key);
                                status_deleted = true;
                            }
                        } else {
                            log::warn!("[remove_acemcp_project_index] ✗ 在 projects_status.json 中未找到匹配的项目，规范化路径: {}", normalized_root);
                        }
                    }
                }
            }
        }
    } else {
        log::warn!("[remove_acemcp_project_index] projects_status.json 文件不存在: {:?}", status_path);
    }

    // 3. 停止该项目的文件监听（如果有）
    let watcher_manager = super::watcher::get_watcher_manager();
    let _ = watcher_manager.stop_watching(&normalized_root);

    // 汇总删除结果
    if projects_deleted || status_deleted {
        log::info!("[remove_acemcp_project_index] 删除完成: projects.json={}, status.json={}", projects_deleted, status_deleted);
        Ok(format!("已删除项目索引记录: {}", normalized_root))
    } else {
        log::warn!("[remove_acemcp_project_index] 未能从任何文件中删除项目，可能路径不匹配");
        // 仍返回成功，因为可能项目本身就不存在（已被其他方式删除）
        Ok(format!("项目索引记录可能已不存在: {}", normalized_root))
    }
}

/// 检查指定目录是否存在
#[tauri::command]
pub fn check_directory_exists(directory_path: String) -> Result<bool, String> {
    use std::path::PathBuf;

    let path = PathBuf::from(&directory_path);
    
    // 尝试规范化路径（处理 Windows 扩展路径前缀等情况）
    let normalized = path.canonicalize().unwrap_or(path.clone());
    
    Ok(normalized.exists() && normalized.is_dir())
}

// ============ 代理检测和测速命令 ============

/// 自动检测本地可用的代理
/// 返回所有检测到的可用代理列表
#[tauri::command]
pub async fn detect_acemcp_proxy(extra_ports: Option<Vec<u16>>) -> Result<Vec<DetectedProxy>, String> {
    log::info!("🔍 开始检测本地代理...");
    
    // 常用代理端口列表
    let mut ports_to_check: Vec<(u16, &'static str)> = vec![
        (7890, "http"),   // Clash 混合端口
        (7891, "http"),   // Clash HTTP 端口
        (10808, "http"),  // V2Ray HTTP 端口
        (10809, "socks5"), // V2Ray SOCKS5 端口
        (1080, "socks5"), // 通用 SOCKS5 端口
        (8080, "http"),   // 通用 HTTP 代理端口
    ];
    
    // 追加用户自定义端口（同时尝试 http 与 socks5）
    if let Some(extra) = extra_ports {
        let mut seen: std::collections::HashSet<(u16, &'static str)> =
            ports_to_check.iter().copied().collect();

        for port in extra {
            if port == 0 {
                continue;
            }

            for proxy_type_str in ["http", "socks5"] {
                if seen.insert((port, proxy_type_str)) {
                    ports_to_check.push((port, proxy_type_str));
                }
            }
        }
    }

    // 并发检测所有端口（符合需求：并发检测 + 3 秒超时由 ProxyDetector 内部保证）
    let mut tasks = tokio::task::JoinSet::new();
    for (port, proxy_type_str) in ports_to_check {
        tasks.spawn(async move {
            let proxy_type = if proxy_type_str == "socks5" {
                ProxyType::Socks5
            } else {
                ProxyType::Http
            };

            let proxy_info = ProxyInfo::new(proxy_type, "127.0.0.1".to_string(), port);
            let start = std::time::Instant::now();

            if ProxyDetector::check_proxy(&proxy_info).await {
                let response_time = start.elapsed().as_millis() as u64;
                log::info!(
                    "✅ 检测到可用代理: 127.0.0.1:{} ({}), 响应时间: {}ms",
                    port,
                    proxy_type_str,
                    response_time
                );

                Some(DetectedProxy {
                    host: "127.0.0.1".to_string(),
                    port,
                    proxy_type: proxy_type_str.to_string(),
                    response_time_ms: Some(response_time),
                })
            } else {
                None
            }
        });
    }

    let mut detected_proxies: Vec<DetectedProxy> = Vec::new();
    while let Some(res) = tasks.join_next().await {
        match res {
            Ok(Some(p)) => detected_proxies.push(p),
            Ok(None) => {}
            Err(e) => log::debug!("代理检测任务异常（忽略，不影响整体结果）: {}", e),
        }
    }
    
    // 按响应时间排序
    detected_proxies.sort_by(|a, b| {
        a.response_time_ms.unwrap_or(u64::MAX).cmp(&b.response_time_ms.unwrap_or(u64::MAX))
    });
    
    log::info!("🔍 代理检测完成，找到 {} 个可用代理", detected_proxies.len());
    Ok(detected_proxies)
}

/// 代理测速命令
/// 测试代理和直连模式下的网络延迟和搜索性能
#[tauri::command]
pub async fn test_acemcp_proxy_speed(
    test_mode: String,        // "proxy" | "direct" | "compare"
    proxy_host: Option<String>,
    proxy_port: Option<u16>,
    proxy_type: Option<String>,
    proxy_username: Option<String>,
    proxy_password: Option<String>,
    test_query: String,
    project_root_path: String,
    project_upload_mode: Option<String>,      // "sample" | "full"
    project_upload_max_files: Option<u32>,    // 采样模式下的文件上限
    state: State<'_, AppState>,
) -> Result<ProxySpeedTestResult, String> {
    log::info!("🚀 开始代理测速: mode={}, query={}", test_mode, test_query);
    
    // 获取配置
    let (base_url, token, batch_size, max_lines_per_blob) = {
        let config = state.config.lock().map_err(|e| format!("获取配置失败: {}", e))?;
        (
            config.mcp_config.acemcp_base_url.clone().ok_or("未配置租户地址")?,
            config.mcp_config.acemcp_token.clone().ok_or("未配置 ACE Token")?,
            config.mcp_config.acemcp_batch_size.unwrap_or(10) as usize,
            config.mcp_config.acemcp_max_lines_per_blob.unwrap_or(800) as usize,
        )
    };
    
    let mut metrics: Vec<SpeedTestMetric> = Vec::new();
    let test_proxy = test_mode == "proxy" || test_mode == "compare";
    let test_direct = test_mode == "direct" || test_mode == "compare";
    
    // 构建代理信息
    let proxy_info = if test_proxy {
        let host = proxy_host.clone().unwrap_or_else(|| "127.0.0.1".to_string());
        let port = proxy_port.unwrap_or(7890);
        let p_type = proxy_type.clone().unwrap_or_else(|| "http".to_string());
        Some(DetectedProxy {
            host,
            port,
            proxy_type: p_type,
            response_time_ms: None,
        })
    } else {
        None
    };

    // 构建代理设置（用于实际 HTTP 请求，支持 https + 认证）
    let proxy_settings = if test_proxy {
        if let Some(ref pi) = proxy_info {
            Some(ProxySettings {
                proxy_type: pi.proxy_type.clone(),
                host: pi.host.clone(),
                port: pi.port,
                username: proxy_username.clone(),
                password: proxy_password.clone(),
            })
        } else {
            None
        }
    } else {
        None
    };

    // 项目上传测速策略（按 zhi 确认：默认采样，可全量/可自定义上限）
    let project_upload_mode = project_upload_mode
        .unwrap_or_else(|| "sample".to_string())
        .to_lowercase();
    let sample_max_files = project_upload_max_files.unwrap_or(200).max(1) as usize;
    let project_upload_max_files_limit: Option<usize> = match project_upload_mode.as_str() {
        "full" => None,
        "sample" => Some(sample_max_files),
        other => return Err(format!("无效的项目上传模式: {}（仅支持 sample/full）", other)),
    };

    // 读取测试项目文件列表（用于上传测速）
    let project_root_path = project_root_path.trim().to_string();
    let mut project_files_status: Option<ProjectFilesStatus> = None;
    let mut project_files_error: Option<String> = None;

    if project_root_path.is_empty() {
        project_files_error = Some("未选择测试项目，已跳过上传测试".to_string());
    } else {
        match AcemcpTool::get_project_files_status(project_root_path.clone()).await {
            Ok(v) => {
                if v.files.is_empty() {
                    project_files_error = Some("测试项目未发现可索引文件，已跳过上传测试".to_string());
                } else {
                    project_files_status = Some(v);
                }
            }
            Err(e) => {
                project_files_error = Some(format!("获取测试项目文件列表失败: {}", e));
            }
        }
    }

    // 构建测速 HTTP Client（复用连接池 + connect_timeout）
    // 说明：测速过程中会多次请求，如果每次都 build client 会有额外开销
    let proxy_client: Option<reqwest::Client> = if test_proxy {
        if let Some(ref ps) = proxy_settings {
            Some(build_speed_test_client(Some(ps), 120)?)
        } else {
            None
        }
    } else {
        None
    };

    let direct_client: Option<reqwest::Client> = if test_direct {
        Some(build_speed_test_client(None, 120)?)
    } else {
        None
    };
    
    // 1. Ping 测试 - 测量到 ACE 服务器的网络延迟
    let health_url = format!("{}/health", base_url);
    let mut ping_metric = SpeedTestMetric {
        name: "🌐 网络延迟".to_string(),
        metric_type: "ping".to_string(),
        proxy_time_ms: None,
        direct_time_ms: None,
        success: true,
        error: None,
    };
    
    // 代理模式 Ping
    if test_proxy {
        if let Some(ref client) = proxy_client {
            let rounds = 3usize;
            let mut ok: Vec<u64> = Vec::with_capacity(rounds);
            let mut last_err: Option<String> = None;

            for _ in 0..rounds {
                match ping_endpoint(client, &health_url, &token).await {
                    Ok(ms) => ok.push(ms),
                    Err(e) => last_err = Some(e),
                }
            }

            if ok.is_empty() {
                ping_metric.success = false;
                append_error(&mut ping_metric.error, format!("代理 Ping 失败: {}", last_err.unwrap_or_else(|| "未知错误".to_string())));
            } else {
                let avg = ok.iter().sum::<u64>() / ok.len() as u64;
                ping_metric.proxy_time_ms = Some(avg);
                if ok.len() != rounds {
                    ping_metric.success = false;
                    append_error(
                        &mut ping_metric.error,
                        format!(
                            "代理 Ping 部分失败: 成功 {}/{}，最后错误: {}",
                            ok.len(),
                            rounds,
                            last_err.unwrap_or_else(|| "未知错误".to_string())
                        ),
                    );
                }
            }
        } else {
            ping_metric.success = false;
            append_error(&mut ping_metric.error, "代理 Ping 跳过：代理 client 未初始化".to_string());
        }
    }
    
    // 直连模式 Ping
    if test_direct {
        let direct_client = direct_client.as_ref().ok_or_else(|| "直连 Ping 跳过：直连 client 未初始化".to_string())?;
        let rounds = 3usize;
        let mut ok: Vec<u64> = Vec::with_capacity(rounds);
        let mut last_err: Option<String> = None;

        for _ in 0..rounds {
            match ping_endpoint(direct_client, &health_url, &token).await {
                Ok(ms) => ok.push(ms),
                Err(e) => last_err = Some(e),
            }
        }

        if ok.is_empty() {
            ping_metric.success = false;
            append_error(&mut ping_metric.error, format!("直连 Ping 失败: {}", last_err.unwrap_or_else(|| "未知错误".to_string())));
        } else {
            let avg = ok.iter().sum::<u64>() / ok.len() as u64;
            ping_metric.direct_time_ms = Some(avg);
            if ok.len() != rounds {
                ping_metric.success = false;
                append_error(
                    &mut ping_metric.error,
                    format!(
                        "直连 Ping 部分失败: 成功 {}/{}，最后错误: {}",
                        ok.len(),
                        rounds,
                        last_err.unwrap_or_else(|| "未知错误".to_string())
                    ),
                );
            }
        }
    }
    metrics.push(ping_metric);
    
    // 2. 语义搜索测试（支持多条查询：按换行/分号分隔）
    let search_url = format!("{}/agents/codebase-retrieval", base_url);

    let mut queries: Vec<String> = test_query
        .split('\n')
        .flat_map(|line| line.split(';'))
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect();

    if queries.is_empty() {
        queries.push("代码搜索测试".to_string());
    }

    // 防止用户输入过多查询导致请求数量过大
    const MAX_QUERIES: usize = 5;
    if queries.len() > MAX_QUERIES {
        queries.truncate(MAX_QUERIES);
    }

    for q in queries {
        let display_q = if q.len() > 30 {
            format!("{}...", &q[..30])
        } else {
            q.clone()
        };

        let mut search_metric = SpeedTestMetric {
            name: format!("🔍 语义搜索 {}", display_q),
            metric_type: "search".to_string(),
            proxy_time_ms: None,
            direct_time_ms: None,
            success: true,
            error: None,
        };

        let search_payload = serde_json::json!({
            "information_request": q,
            "blobs": {"checkpoint_id": null, "added_blobs": [], "deleted_blobs": []},
            "dialog": [],
            "max_output_length": 100,
            "disable_codebase_retrieval": false,
            "enable_commit_retrieval": false,
        });

        // 代理模式搜索
        if test_proxy {
            if let Some(ref client) = proxy_client {
                match search_endpoint(client, &search_url, &token, &search_payload).await {
                    Ok(ms) => search_metric.proxy_time_ms = Some(ms),
                    Err(e) => {
                        search_metric.success = false;
                        search_metric.error = Some(format!("代理搜索失败: {}", e));
                    }
                }
            } else {
                search_metric.success = false;
                search_metric.error = Some("代理搜索跳过：代理 client 未初始化".to_string());
            }
        }

        // 直连模式搜索
        if test_direct {
            let direct_client = direct_client.as_ref().ok_or_else(|| "直连搜索跳过：直连 client 未初始化".to_string())?;
            match search_endpoint(direct_client, &search_url, &token, &search_payload).await {
                Ok(ms) => search_metric.direct_time_ms = Some(ms),
                Err(e) => {
                    if search_metric.error.is_none() {
                        search_metric.success = false;
                        search_metric.error = Some(format!("直连搜索失败: {}", e));
                    }
                }
            }
        }

        metrics.push(search_metric);
    }

    // 3. 单文件上传测试（真实走 /batch-upload）
    let mut upload_single_metric = SpeedTestMetric {
        name: "📤 单文件上传".to_string(),
        metric_type: "upload_single".to_string(),
        proxy_time_ms: None,
        direct_time_ms: None,
        success: true,
        error: None,
    };

    if let Some(err) = project_files_error.clone() {
        upload_single_metric.success = false;
        upload_single_metric.error = Some(err);
    } else if let Some(ref pfs) = project_files_status {
        // 随机选择一个文件进行单文件上传测速（更贴近真实场景）
        if !pfs.files.is_empty() {
            let random_index = fastrand::usize(0..pfs.files.len());
            let file = &pfs.files[random_index];
            match build_single_file_blobs_for_speed_test(&project_root_path, &file.path, max_lines_per_blob) {
                Ok((blobs, file_bytes)) => {
                    let upload_url = format!("{}/batch-upload", base_url);
                    upload_single_metric.name = format!(
                        "📤 单文件上传 ({}，{} blobs)",
                        format_bytes(file_bytes),
                        blobs.len()
                    );

                    if test_proxy {
                        if let Some(ref client) = proxy_client {
                            match upload_blobs_batch(client, &upload_url, &token, &blobs, 120).await {
                                Ok(ms) => upload_single_metric.proxy_time_ms = Some(ms),
                                Err(e) => {
                                    upload_single_metric.success = false;
                                    append_error(&mut upload_single_metric.error, format!("代理上传失败: {}", e));
                                }
                            }
                        } else {
                            upload_single_metric.success = false;
                            append_error(&mut upload_single_metric.error, "代理上传跳过：代理 client 未初始化".to_string());
                        }
                    }

                    if test_direct {
                        let direct_client = direct_client.as_ref().ok_or_else(|| "直连上传跳过：直连 client 未初始化".to_string())?;
                        match upload_blobs_batch(direct_client, &upload_url, &token, &blobs, 120).await {
                            Ok(ms) => upload_single_metric.direct_time_ms = Some(ms),
                            Err(e) => {
                                upload_single_metric.success = false;
                                append_error(&mut upload_single_metric.error, format!("直连上传失败: {}", e));
                            }
                        }
                    }
                }
                Err(e) => {
                    upload_single_metric.success = false;
                    upload_single_metric.error = Some(e);
                }
            }
        } else {
            upload_single_metric.success = false;
            upload_single_metric.error = Some("测试项目没有可用文件，已跳过单文件上传测试".to_string());
        }
    }
    metrics.push(upload_single_metric);

    // 4. 项目上传测试（按策略：采样/全量）
    let mut upload_project_metric = SpeedTestMetric {
        name: "📦 项目上传".to_string(),
        metric_type: "upload_project".to_string(),
        proxy_time_ms: None,
        direct_time_ms: None,
        success: true,
        error: None,
    };

    if let Some(err) = project_files_error.clone() {
        upload_project_metric.success = false;
        upload_project_metric.error = Some(err);
    } else if let Some(ref pfs) = project_files_status {
        let mut detail: Option<ProjectUploadResult> = None;

        if test_proxy {
            if let Some(ref client) = proxy_client {
                match upload_project_for_speed_test(
                    client,
                    &base_url,
                    &token,
                    &project_root_path,
                    pfs,
                    batch_size,
                    max_lines_per_blob,
                    project_upload_max_files_limit,
                )
                .await
                {
                    Ok(r) => {
                        upload_project_metric.proxy_time_ms = Some(r.elapsed_ms);
                        if detail.is_none() {
                            detail = Some(r);
                        }
                    }
                    Err(e) => {
                        upload_project_metric.success = false;
                        append_error(&mut upload_project_metric.error, format!("代理项目上传失败: {}", e));
                    }
                }
            } else {
                upload_project_metric.success = false;
                append_error(&mut upload_project_metric.error, "代理项目上传跳过：代理 client 未初始化".to_string());
            }
        }

        if test_direct {
            let direct_client = direct_client.as_ref().ok_or_else(|| "直连项目上传跳过：直连 client 未初始化".to_string())?;
            match upload_project_for_speed_test(
                direct_client,
                &base_url,
                &token,
                &project_root_path,
                pfs,
                batch_size,
                max_lines_per_blob,
                project_upload_max_files_limit,
            )
            .await
            {
                Ok(r) => {
                    upload_project_metric.direct_time_ms = Some(r.elapsed_ms);
                    if detail.is_none() {
                        detail = Some(r);
                    }
                }
                Err(e) => {
                    upload_project_metric.success = false;
                    append_error(&mut upload_project_metric.error, format!("直连项目上传失败: {}", e));
                }
            }
        }

        if let Some(r) = detail {
            let mode_label = match project_upload_mode.as_str() {
                "full" => format!("全量 {} 文件", r.planned_files),
                _ => format!("采样 {}/{} 文件", r.planned_files, r.total_files),
            };

            upload_project_metric.name = format!(
                "📦 项目上传 ({}，{}，{} blobs)",
                mode_label,
                format_bytes(r.total_bytes),
                r.blob_count
            );

            if r.skipped_files > 0 {
                upload_project_metric.success = false;
                append_error(&mut upload_project_metric.error, format!("读取失败文件: {} 个", r.skipped_files));
                if let Some(e) = r.first_error {
                    append_error(&mut upload_project_metric.error, e);
                }
            }

            if r.truncated {
                append_error(&mut upload_project_metric.error, "已按采样上限截断文件数量".to_string());
            }
        }
    }
    metrics.push(upload_project_metric);
    
    // 生成推荐建议（附带成功率与失败摘要）
    let mut recommendation = generate_recommendation(&metrics, &test_mode);
    let all_success = metrics.iter().all(|m| m.success);

    let total = metrics.len().max(1);
    let ok = metrics.iter().filter(|m| m.success).count();
    recommendation = format!("{} | 总体成功率: {}/{}", recommendation, ok, total);

    if test_proxy {
        let ok_proxy = metrics.iter().filter(|m| m.proxy_time_ms.is_some()).count();
        recommendation = format!("{} | 代理成功: {}/{}", recommendation, ok_proxy, total);
    }

    if test_direct {
        let ok_direct = metrics.iter().filter(|m| m.direct_time_ms.is_some()).count();
        recommendation = format!("{} | 直连成功: {}/{}", recommendation, ok_direct, total);
    }

    if !all_success {
        if let Some(first_fail) = metrics.iter().find(|m| !m.success) {
            if let Some(err) = &first_fail.error {
                let mut err_short = err.replace('\n', " / ");
                if err_short.len() > 120 {
                    err_short.truncate(120);
                    err_short.push_str("...");
                }
                recommendation = format!("{} | 失败示例: {} - {}", recommendation, first_fail.name, err_short);
            } else {
                recommendation = format!("{} | 存在失败项", recommendation);
            }
        }
    }
    
    let result = ProxySpeedTestResult {
        mode: test_mode,
        proxy_info,
        metrics,
        timestamp: chrono::Utc::now().to_rfc3339(),
        recommendation,
        success: all_success,
    };
    
    log::info!("🚀 代理测速完成: success={}", all_success);
    Ok(result)
}

/// 代理设置（用于测速等临时请求）
/// 支持：HTTP / HTTPS / SOCKS5 代理 + Basic Auth
#[derive(Debug, Clone)]
struct ProxySettings {
    proxy_type: String,           // "http" | "https" | "socks5"
    host: String,
    port: u16,
    username: Option<String>,
    password: Option<String>,
}

impl ProxySettings {
    /// 构建 reqwest 代理配置（避免泄露密码到日志）
    fn to_reqwest_proxy(&self) -> Result<reqwest::Proxy, String> {
        // 校验代理类型，避免拼接出无效 URL
        match self.proxy_type.as_str() {
            "http" | "https" | "socks5" => {}
            other => return Err(format!("不支持的代理类型: {}（仅支持 http/https/socks5）", other)),
        }

        if self.host.trim().is_empty() {
            return Err("代理主机不能为空".to_string());
        }

        let proxy_url = format!("{}://{}:{}", self.proxy_type, self.host.trim(), self.port);
        let mut reqwest_proxy = reqwest::Proxy::all(&proxy_url)
            .map_err(|e| format!("创建代理失败: {}", e))?;

        // 代理认证（Basic Auth）
        if let Some(username) = self.username.as_deref() {
            let username = username.trim();
            if !username.is_empty() {
                let password = self.password.as_deref().unwrap_or("");
                reqwest_proxy = reqwest_proxy.basic_auth(username, password);
            }
        }

        Ok(reqwest_proxy)
    }
}

/// 上传用的 blob 结构（与 /batch-upload 接口的输入保持一致）
#[derive(Debug, Clone, serde::Serialize)]
struct UploadBlob {
    path: String,
    content: String,
}

/// 读取文件内容，支持多种编码检测（与 acemcp::mcp.rs 保持一致）
fn read_file_with_encoding_for_speed_test(path: &std::path::Path) -> Result<String, String> {
    use std::fs;
    use std::io::Read;

    use encoding_rs::{GBK, WINDOWS_1252, UTF_8};

    let mut file = fs::File::open(path).map_err(|e| format!("打开文件失败: {}", e))?;
    let mut buf = Vec::new();
    file.read_to_end(&mut buf).map_err(|e| format!("读取文件失败: {}", e))?;

    // 尝试 utf-8
    let (decoded, _, had_errors) = UTF_8.decode(&buf);
    if !had_errors {
        return Ok(decoded.into_owned());
    }

    // 尝试 gbk
    let (decoded, _, had_errors) = GBK.decode(&buf);
    if !had_errors {
        log::debug!("测速读取文件：成功使用 GBK 编码: {:?}", path);
        return Ok(decoded.into_owned());
    }

    // 尝试 latin-1 (WINDOWS_1252 是 ISO-8859-1 的超集)
    let (decoded, _, had_errors) = WINDOWS_1252.decode(&buf);
    if !had_errors {
        log::debug!("测速读取文件：成功使用 WINDOWS_1252 编码: {:?}", path);
        return Ok(decoded.into_owned());
    }

    // 降级：utf-8 lossy
    let (decoded, _, _) = UTF_8.decode(&buf);
    log::debug!("测速读取文件：使用 UTF-8 (lossy)，部分字符可能丢失: {:?}", path);
    Ok(decoded.into_owned())
}

/// 分割文件内容为多个 blob（如果超过最大行数）
/// 与 acemcp::mcp.rs 保持一致：chunk 索引从 1 开始
fn split_content_for_speed_test(path: &str, content: &str, max_lines: usize) -> Vec<UploadBlob> {
    let lines: Vec<&str> = content.split_inclusive('\n').collect();
    let total_lines = lines.len();

    if total_lines <= max_lines {
        return vec![UploadBlob {
            path: path.to_string(),
            content: content.to_string(),
        }];
    }

    let num_chunks = (total_lines + max_lines - 1) / max_lines;
    let mut blobs = Vec::new();

    for chunk_idx in 0..num_chunks {
        let start_line = chunk_idx * max_lines;
        let end_line = usize::min(start_line + max_lines, total_lines);
        let chunk_lines = &lines[start_line..end_line];
        let chunk_content = chunk_lines.join("");

        let chunk_path = format!("{}#chunk{}of{}", path, chunk_idx + 1, num_chunks);
        blobs.push(UploadBlob {
            path: chunk_path,
            content: chunk_content,
        });
    }

    blobs
}

/// 构建测速用 HTTP Client（支持代理 + connect_timeout）
/// 说明：测速过程中会多次请求，如果每次都 build client 会有额外开销
fn build_speed_test_client(proxy: Option<&ProxySettings>, timeout_secs: u64) -> Result<reqwest::Client, String> {
    let mut client_builder = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_millis(crate::constants::network::CONNECTION_TIMEOUT_MS))
        .timeout(std::time::Duration::from_secs(timeout_secs));

    if let Some(p) = proxy {
        client_builder = client_builder.proxy(p.to_reqwest_proxy()?);
    }

    client_builder
        .build()
        .map_err(|e| format!("构建客户端失败: {}", e))
}

/// 上传一批 blobs，返回耗时（毫秒）
async fn upload_blobs_batch(
    client: &reqwest::Client,
    upload_url: &str,
    token: &str,
    blobs: &[UploadBlob],
    timeout_secs: u64,
) -> Result<u64, String> {
    if blobs.is_empty() {
        return Ok(0);
    }

    let payload = serde_json::json!({ "blobs": blobs });
    let start = std::time::Instant::now();

    let resp = client
        .post(upload_url)
        .timeout(std::time::Duration::from_secs(timeout_secs))
        .header(reqwest::header::AUTHORIZATION, format!("Bearer {}", token))
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .json(&payload)
        .send()
        .await
        .map_err(|e| format!("上传请求失败: {}", e))?;

    let elapsed = start.elapsed().as_millis() as u64;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("HTTP {} {}", status, body));
    }

    Ok(elapsed)
}

fn format_bytes(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * 1024;
    const GB: u64 = 1024 * 1024 * 1024;

    if bytes >= GB {
        return format!("{:.2}GB", bytes as f64 / GB as f64);
    }
    if bytes >= MB {
        return format!("{:.2}MB", bytes as f64 / MB as f64);
    }
    if bytes >= KB {
        return format!("{:.2}KB", bytes as f64 / KB as f64);
    }
    format!("{}B", bytes)
}

fn append_error(dst: &mut Option<String>, msg: String) {
    match dst {
        Some(s) => {
            s.push('\n');
            s.push_str(&msg);
        }
        None => {
            *dst = Some(msg);
        }
    }
}

struct ProjectUploadResult {
    elapsed_ms: u64,
    planned_files: usize,
    tested_files: usize,
    total_files: usize,
    skipped_files: usize,
    blob_count: usize,
    total_bytes: u64,
    truncated: bool,
    first_error: Option<String>,
}

/// 项目上传测速：按文件列表读取内容并批量上传 blobs
/// - `max_files`: Some(n) 表示最多测试 n 个文件（采样），None 表示全量
async fn upload_project_for_speed_test(
    client: &reqwest::Client,
    base_url: &str,
    token: &str,
    project_root_path: &str,
    project_files_status: &ProjectFilesStatus,
    batch_size: usize,
    max_lines_per_blob: usize,
    max_files: Option<usize>,
) -> Result<ProjectUploadResult, String> {
    use std::path::PathBuf;

    let total_files = project_files_status.files.len();
    let files_to_test = match max_files {
        Some(max) => usize::min(max, total_files),
        None => total_files,
    };

    let truncated = max_files.is_some() && total_files > files_to_test;
    let upload_url = format!("{}/batch-upload", base_url);

    let start = std::time::Instant::now();

    let mut batch: Vec<UploadBlob> = Vec::with_capacity(batch_size);
    let mut tested_files = 0usize;
    let mut skipped_files = 0usize;
    let mut blob_count = 0usize;
    let mut total_bytes = 0u64;
    let mut first_error: Option<String> = None;

    for file in project_files_status.files.iter().take(files_to_test) {
        let abs_path = PathBuf::from(project_root_path).join(&file.path);

        // 统计文件大小（即使读取失败也尽量统计）
        if let Ok(meta) = std::fs::metadata(&abs_path) {
            total_bytes += meta.len();
        }

        let content = match read_file_with_encoding_for_speed_test(&abs_path) {
            Ok(c) => c,
            Err(e) => {
                skipped_files += 1;
                if first_error.is_none() {
                    first_error = Some(format!("读取文件失败: path={}, error={}", file.path, e));
                }
                continue;
            }
        };

        tested_files += 1;
        let blobs = split_content_for_speed_test(&file.path, &content, max_lines_per_blob);
        blob_count += blobs.len();

        for b in blobs {
            batch.push(b);
            if batch.len() >= batch_size {
                // 上传一批
                let _ = upload_blobs_batch(client, &upload_url, token, &batch, 120).await?;
                batch.clear();
            }
        }
    }

    if !batch.is_empty() {
        let _ = upload_blobs_batch(client, &upload_url, token, &batch, 120).await?;
    }

    let elapsed_ms = start.elapsed().as_millis() as u64;

    Ok(ProjectUploadResult {
        elapsed_ms,
        planned_files: files_to_test,
        tested_files,
        total_files,
        skipped_files,
        blob_count,
        total_bytes,
        truncated,
        first_error,
    })
}

fn build_single_file_blobs_for_speed_test(
    project_root_path: &str,
    rel_path: &str,
    max_lines_per_blob: usize,
) -> Result<(Vec<UploadBlob>, u64), String> {
    use std::path::PathBuf;

    let abs_path = PathBuf::from(project_root_path).join(rel_path);
    let file_bytes = std::fs::metadata(&abs_path).map(|m| m.len()).unwrap_or(0);

    let content = read_file_with_encoding_for_speed_test(&abs_path)
        .map_err(|e| format!("读取文件失败: path={}, error={}", rel_path, e))?;

    let blobs = split_content_for_speed_test(rel_path, &content, max_lines_per_blob);
    Ok((blobs, file_bytes))
}

/// Ping 测试辅助函数
async fn ping_endpoint(client: &reqwest::Client, url: &str, token: &str) -> Result<u64, String> {
    let start = std::time::Instant::now();
    let response = client
        .head(url)
        .timeout(std::time::Duration::from_secs(10))
        .header(reqwest::header::AUTHORIZATION, format!("Bearer {}", token))
        .send()
        .await
        .map_err(|e| format!("请求失败: {}", e))?;
    
    let elapsed = start.elapsed().as_millis() as u64;
    
    if response.status().is_success() || response.status().as_u16() == 404 {
        // 404 也算成功，因为只是测试连通性
        Ok(elapsed)
    } else {
        Err(format!("HTTP {}", response.status()))
    }
}

/// 搜索测试辅助函数
async fn search_endpoint(client: &reqwest::Client, url: &str, token: &str, payload: &serde_json::Value) -> Result<u64, String> {
    let start = std::time::Instant::now();
    let response = client
        .post(url)
        .timeout(std::time::Duration::from_secs(30))
        .header(reqwest::header::AUTHORIZATION, format!("Bearer {}", token))
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .json(payload)
        .send()
        .await
        .map_err(|e| format!("请求失败: {}", e))?;
    
    let elapsed = start.elapsed().as_millis() as u64;
    
    if response.status().is_success() {
        Ok(elapsed)
    } else {
        Err(format!("HTTP {}", response.status()))
    }
}

/// 生成推荐建议
fn generate_recommendation(metrics: &[SpeedTestMetric], mode: &str) -> String {
    if mode != "compare" {
        return "单模式测试完成".to_string();
    }
    
    let mut proxy_total: u64 = 0;
    let mut direct_total: u64 = 0;
    let mut proxy_count = 0;
    let mut direct_count = 0;
    
    for m in metrics {
        if let Some(pt) = m.proxy_time_ms {
            proxy_total += pt;
            proxy_count += 1;
        }
        if let Some(dt) = m.direct_time_ms {
            direct_total += dt;
            direct_count += 1;
        }
    }
    
    if proxy_count == 0 || direct_count == 0 {
        return "无法对比，部分测试失败".to_string();
    }
    
    let proxy_avg = proxy_total / proxy_count as u64;
    let direct_avg = direct_total / direct_count as u64;
    
    if proxy_avg < direct_avg {
        let improvement = ((direct_avg - proxy_avg) as f64 / direct_avg as f64 * 100.0) as u32;
        format!("🟢 建议启用代理，性能提升约 {}%", improvement)
    } else if direct_avg < proxy_avg {
        let degradation = ((proxy_avg - direct_avg) as f64 / proxy_avg as f64 * 100.0) as u32;
        format!("🔴 建议直连，代理性能下降约 {}%", degradation)
    } else {
        "🟡 代理与直连性能相当".to_string()
    }
}
