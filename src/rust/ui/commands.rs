use crate::config::{save_config, load_config, AppState, ReplyConfig, WindowConfig, CustomPrompt, CustomPromptConfig, ShortcutConfig, ShortcutBinding};
use crate::constants::{window, ui, validation};
use crate::mcp::types::{build_refill_response, IngredientAttachment, PopupRequest};
use crate::mcp::{discard_spice, fetch_ingredient_bytes, stash_ingredient_bytes};
use crate::mcp::handlers::create_tauri_popup;
use serde::Serialize;
use tauri::{AppHandle, Manager, State};
use arboard::Clipboard;
use base64::engine::general_purpose;
use base64::Engine;
use image::codecs::png::PngEncoder;
use image::ColorType;
use image::ImageFormat;
use image::ImageEncoder;
use percent_encoding::percent_decode_str;
use std::fs;
use std::path::PathBuf;
#[cfg(target_os = "linux")]
use std::process::Command;
#[cfg(target_os = "linux")]
use std::io::ErrorKind;
#[cfg(any(target_os = "windows", target_os = "macos"))]
use std::process::Command;

#[tauri::command]
pub async fn get_app_info() -> Result<String, String> {
    Ok(format!("Bistro v{}", env!("CARGO_PKG_VERSION")))
}

#[derive(Debug, Clone, Serialize)]
pub struct CachedIngredient {
    pub spice_id: String,
    pub dish_type: String,
    pub tag: Option<String>,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone)]
struct ClipboardIngredientBytes {
    dish_type: String,
    tag: Option<String>,
    bytes: Vec<u8>,
}

fn normalize_ingredient_bytes(bytes: &[u8], dish_type: &str) -> Result<(Vec<u8>, String), String> {
    let dt = dish_type.trim();
    match dt {
        "image/bmp" | "image/x-ms-bmp" => {
            let img = image::load_from_memory_with_format(bytes, ImageFormat::Bmp)
                .map_err(|e| format!("读取 BMP 失败: {}", e))?;
            let rgba = img.to_rgba8();
            let (width, height) = rgba.dimensions();
            let raw = rgba.into_raw();

            let mut png_bytes: Vec<u8> = Vec::new();
            let encoder = PngEncoder::new(&mut png_bytes);
            encoder
                .write_image(raw.as_slice(), width, height, ColorType::Rgba8.into())
                .map_err(|e| format!("转换 PNG 失败: {}", e))?;
            Ok((png_bytes, "image/png".to_string()))
        }
        _ => Ok((bytes.to_vec(), dish_type.to_string())),
    }
}

fn stash_ingredient(
    bytes: Vec<u8>,
    dish_type: &str,
    tag: Option<String>,
) -> Result<CachedIngredient, String> {
    let (normalized_bytes, normalized_dish_type) =
        normalize_ingredient_bytes(&bytes, dish_type)?;
    let spice_id = stash_ingredient_bytes(&normalized_bytes, normalized_dish_type.as_str(), tag.clone())
        .map_err(|e| format!("保存食材失败: {}", e))?;
    Ok(CachedIngredient {
        spice_id,
        dish_type: normalized_dish_type,
        tag,
        bytes: normalized_bytes,
    })
}

#[cfg(target_os = "linux")]
fn run_linux_command_output(cmd: &str, args: &[&str]) -> Option<std::process::Output> {
    let candidates = [
        cmd.to_string(),
        format!("/snap/bin/{cmd}"),
        format!("/usr/local/bin/{cmd}"),
        format!("/usr/bin/{cmd}"),
        format!("/bin/{cmd}"),
    ];

    for program in candidates {
        match Command::new(&program).args(args).output() {
            Ok(o) => return Some(o),
            Err(e) if e.kind() == ErrorKind::NotFound => continue,
            Err(_) => continue,
        }
    }
    None
}

#[cfg(target_os = "linux")]
fn try_read_ingredients_from_linux_clipboard_text() -> Option<Vec<ClipboardIngredientBytes>> {
    let texts = try_read_linux_text_candidates()?;
    let mut out: Vec<ClipboardIngredientBytes> = Vec::new();

    for text in texts {
        let paths = extract_file_paths_from_clipboard_text(&text);
        for p in paths {
            if let Some(item) = try_load_ingredient_file_as_clipboard_item(&p) {
                out.push(item);
            }
        }
        if !out.is_empty() {
            break;
        }
    }

    if out.is_empty() { None } else { Some(out) }
}

#[cfg(target_os = "linux")]
fn try_read_linux_text_candidates() -> Option<Vec<String>> {
    let candidates: Vec<(&str, Vec<&str>)> = vec![
        ("wl-paste", vec!["--no-newline", "--type", "text/uri-list"]),
        ("wl-paste", vec!["--no-newline", "--type", "text/plain;charset=utf-8"]),
        ("wl-paste", vec!["--no-newline"]),
        ("wl-paste", vec!["--primary", "--no-newline", "--type", "text/uri-list"]),
        ("wl-paste", vec!["--primary", "--no-newline", "--type", "text/plain;charset=utf-8"]),
        ("wl-paste", vec!["--primary", "--no-newline"]),

        ("wl-clip.paste", vec!["--no-newline", "--type", "text/uri-list"]),
        ("wl-clip.paste", vec!["--no-newline", "--type", "text/plain;charset=utf-8"]),
        ("wl-clip.paste", vec!["--no-newline"]),
        ("wl-clip.paste", vec!["--primary", "--no-newline", "--type", "text/uri-list"]),
        ("wl-clip.paste", vec!["--primary", "--no-newline", "--type", "text/plain;charset=utf-8"]),
        ("wl-clip.paste", vec!["--primary", "--no-newline"]),

        ("xclip", vec!["-selection", "clipboard", "-t", "text/uri-list", "-o"]),
        ("xclip", vec!["-selection", "clipboard", "-o"]),
        ("xclip", vec!["-selection", "primary", "-t", "text/uri-list", "-o"]),
        ("xclip", vec!["-selection", "primary", "-o"]),
    ];

    let mut out: Vec<String> = Vec::new();
    for (cmd, args) in candidates {
        let output = match run_linux_command_output(cmd, &args) {
            Some(o) => o,
            None => continue,
        };
        if !output.status.success() {
            continue;
        }
        let s = match String::from_utf8(output.stdout) {
            Ok(s) => s,
            Err(_) => continue,
        };
        let s = s.trim().to_string();
        if s.is_empty() {
            continue;
        }
        if !out.contains(&s) {
            out.push(s);
        }
    }

    if out.is_empty() { None } else { Some(out) }
}

#[tauri::command]
pub async fn get_always_on_top(state: State<'_, AppState>) -> Result<bool, String> {
    let config = state
        .config
        .lock()
        .map_err(|e| format!("获取配置失败: {}", e))?;
    Ok(config.ui_config.always_on_top)
}

#[tauri::command]
pub async fn set_always_on_top(
    enabled: bool,
    state: State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    {
        let mut config = state
            .config
            .lock()
            .map_err(|e| format!("获取配置失败: {}", e))?;
        config.ui_config.always_on_top = enabled;
    }

    // 保存配置到文件
    save_config(&state, &app)
        .await
        .map_err(|e| format!("保存配置失败: {}", e))?;

    // 应用到当前窗口
    if let Some(window) = app.get_webview_window("main") {
        window
            .set_always_on_top(enabled)
            .map_err(|e| format!("设置窗口置顶失败: {}", e))?;

        log::info!("用户切换窗口置顶状态为: {} (已保存配置)", enabled);
    }

    Ok(())
}

#[tauri::command]
pub async fn sync_window_state(
    state: State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    // 根据配置同步窗口状态
    let always_on_top = {
        let config = state
            .config
            .lock()
            .map_err(|e| format!("获取配置失败: {}", e))?;
        config.ui_config.always_on_top
    };

    // 应用到当前窗口
    if let Some(window) = app.get_webview_window("main") {
        window
            .set_always_on_top(always_on_top)
            .map_err(|e| format!("同步窗口状态失败: {}", e))?;
    }

    Ok(())
}

/// 重新加载配置文件到内存
#[tauri::command]
pub async fn reload_config(
    state: State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    // 从文件重新加载配置到内存
    load_config(&state, &app)
        .await
        .map_err(|e| format!("重新加载配置失败: {}", e))?;

    Ok(())
}

#[tauri::command]
pub async fn get_theme(state: State<'_, AppState>) -> Result<String, String> {
    let config = state
        .config
        .lock()
        .map_err(|e| format!("获取配置失败: {}", e))?;
    Ok(config.ui_config.theme.clone())
}

#[tauri::command]
pub async fn set_theme(
    theme: String,
    state: State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    // 验证主题值
    if !["light", "dark"].contains(&theme.as_str()) {
        return Err("无效的主题值，只支持 light、dark".to_string());
    }

    {
        let mut config = state
            .config
            .lock()
            .map_err(|e| format!("获取配置失败: {}", e))?;
        config.ui_config.theme = theme;
    }

    // 保存配置到文件
    save_config(&state, &app)
        .await
        .map_err(|e| format!("保存配置失败: {}", e))?;

    Ok(())
}

#[tauri::command]
pub async fn get_window_config(state: State<'_, AppState>) -> Result<WindowConfig, String> {
    let config = state
        .config
        .lock()
        .map_err(|e| format!("获取配置失败: {}", e))?;
    Ok(config.ui_config.window_config.clone())
}

#[tauri::command]
pub async fn set_window_config(
    window_config: WindowConfig,
    state: State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    {
        let mut config = state
            .config
            .lock()
            .map_err(|e| format!("获取配置失败: {}", e))?;
        config.ui_config.window_config = window_config;
    }

    // 保存配置到文件
    save_config(&state, &app)
        .await
        .map_err(|e| format!("保存配置失败: {}", e))?;

    Ok(())
}

#[tauri::command]
pub async fn get_reply_config(state: State<'_, AppState>) -> Result<ReplyConfig, String> {
    let config = state
        .config
        .lock()
        .map_err(|e| format!("获取配置失败: {}", e))?;
    Ok(config.reply_config.clone())
}

#[tauri::command]
pub async fn set_reply_config(
    reply_config: ReplyConfig,
    state: State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    {
        let mut config = state
            .config
            .lock()
            .map_err(|e| format!("获取配置失败: {}", e))?;
        config.reply_config = reply_config;
    }

    // 保存配置到文件
    save_config(&state, &app)
        .await
        .map_err(|e| format!("保存配置失败: {}", e))?;

    Ok(())
}

#[tauri::command]
pub async fn get_window_settings(state: State<'_, AppState>) -> Result<serde_json::Value, String> {
    let config = state
        .config
        .lock()
        .map_err(|e| format!("获取配置失败: {}", e))?;

    // 返回窗口设置，包含两种模式的独立尺寸
    let window_settings = serde_json::json!({
        "fixed": config.ui_config.window_config.fixed,
        "current_width": config.ui_config.window_config.current_width(),
        "current_height": config.ui_config.window_config.current_height(),
        "fixed_width": config.ui_config.window_config.fixed_width,
        "fixed_height": config.ui_config.window_config.fixed_height,
        "free_width": config.ui_config.window_config.free_width,
        "free_height": config.ui_config.window_config.free_height
    });

    Ok(window_settings)
}

#[tauri::command]
pub async fn get_window_settings_for_mode(
    fixed: bool,
    state: State<'_, AppState>,
) -> Result<serde_json::Value, String> {
    let config = state
        .config
        .lock()
        .map_err(|e| format!("获取配置失败: {}", e))?;

    // 返回指定模式的窗口设置
    let (width, height) = if fixed {
        (
            config.ui_config.window_config.fixed_width,
            config.ui_config.window_config.fixed_height,
        )
    } else {
        (
            config.ui_config.window_config.free_width,
            config.ui_config.window_config.free_height,
        )
    };

    let window_settings = serde_json::json!({
        "width": width,
        "height": height,
        "fixed": fixed
    });

    Ok(window_settings)
}

#[tauri::command]
pub async fn get_window_constraints_cmd() -> Result<serde_json::Value, String> {
    let constraints = window::get_default_constraints();
    let ui_timings = ui::get_default_ui_timings();

    let mut result = constraints.to_json();
    if let serde_json::Value::Object(ref mut map) = result {
        if let serde_json::Value::Object(ui_map) = ui_timings.to_json() {
            map.extend(ui_map);
        }
    }

    Ok(result)
}

#[tauri::command]
pub async fn get_current_window_size(app: tauri::AppHandle) -> Result<serde_json::Value, String> {
    if let Some(window) = app.get_webview_window("main") {
        // 检查窗口是否最小化
        if let Ok(is_minimized) = window.is_minimized() {
            if is_minimized {
                return Err("窗口已最小化，跳过尺寸获取".to_string());
            }
        }

        // 获取逻辑尺寸而不是物理尺寸
        if let Ok(logical_size) = window.inner_size().map(|physical_size| {
            // 获取缩放因子
            let scale_factor = window.scale_factor().unwrap_or(1.0);

            // 转换为逻辑尺寸
            let logical_width = physical_size.width as f64 / scale_factor;
            let logical_height = physical_size.height as f64 / scale_factor;

            tauri::LogicalSize::new(logical_width, logical_height)
        }) {
            let width = logical_size.width.round() as u32;
            let height = logical_size.height.round() as u32;

            // 验证并调整尺寸到有效范围
            let (clamped_width, clamped_height) = crate::constants::window::clamp_window_size(width as f64, height as f64);
            let final_width = clamped_width as u32;
            let final_height = clamped_height as u32;

            if final_width != width || final_height != height {
                log::info!("窗口尺寸已调整: {}x{} -> {}x{}", width, height, final_width, final_height);
            }

            let window_size = serde_json::json!({
                "width": final_width,
                "height": final_height
            });
            return Ok(window_size);
        }
    }

    Err("无法获取当前窗口大小".to_string())
}

#[tauri::command]
pub async fn set_window_settings(
    window_settings: serde_json::Value,
    state: State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    {
        let mut config = state
            .config
            .lock()
            .map_err(|e| format!("获取配置失败: {}", e))?;

        // 更新窗口配置
        if let Some(fixed) = window_settings.get("fixed").and_then(|v| v.as_bool()) {
            config.ui_config.window_config.fixed = fixed;
        }

        // 更新固定模式尺寸（添加尺寸验证）
        if let Some(width) = window_settings.get("fixed_width").and_then(|v| v.as_f64()) {
            if let Some(height) = window_settings.get("fixed_height").and_then(|v| v.as_f64()) {
                if validation::is_valid_window_size(width, height) {
                    config.ui_config.window_config.fixed_width = width;
                    config.ui_config.window_config.fixed_height = height;
                }
            } else if width >= window::MIN_WIDTH {
                config.ui_config.window_config.fixed_width = width;
            }
        } else if let Some(height) = window_settings.get("fixed_height").and_then(|v| v.as_f64()) {
            if height >= window::MIN_HEIGHT {
                config.ui_config.window_config.fixed_height = height;
            }
        }

        // 更新自由拉伸模式尺寸（添加尺寸验证）
        if let Some(width) = window_settings.get("free_width").and_then(|v| v.as_f64()) {
            if let Some(height) = window_settings.get("free_height").and_then(|v| v.as_f64()) {
                if validation::is_valid_window_size(width, height) {
                    config.ui_config.window_config.free_width = width;
                    config.ui_config.window_config.free_height = height;
                }
            } else if width >= window::MIN_WIDTH {
                config.ui_config.window_config.free_width = width;
            }
        } else if let Some(height) = window_settings.get("free_height").and_then(|v| v.as_f64()) {
            if height >= window::MIN_HEIGHT {
                config.ui_config.window_config.free_height = height;
            }
        }

        // 兼容旧的width/height参数，更新当前模式的尺寸（添加尺寸验证）
        if let (Some(width), Some(height)) = (
            window_settings.get("width").and_then(|v| v.as_f64()),
            window_settings.get("height").and_then(|v| v.as_f64()),
        ) {
            if validation::is_valid_window_size(width, height) {
                config
                    .ui_config
                    .window_config
                    .update_current_size(width, height);
            }
        }
    }

    // 保存配置到文件
    save_config(&state, &app)
        .await
        .map_err(|e| format!("保存配置失败: {}", e))?;

    Ok(())
}

#[tauri::command]
pub async fn send_mcp_response(
    response: serde_json::Value,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let mut response = response;
    resolve_spice_ids_in_dish_response(&mut response)?;

    // 将响应序列化为JSON字符串
    let response_str =
        serde_json::to_string(&response).map_err(|e| format!("序列化响应失败: {}", e))?;

    if response_str.trim().is_empty() {
        return Err("响应内容不能为空".to_string());
    }

    // 检查是否为MCP模式
    let args: Vec<String> = std::env::args().collect();
    let is_mcp_mode = args.iter().any(|arg| arg == "--mcp-request");

    if is_mcp_mode {
        // 检查是否有响应文件路径（分离式异步模式）
        if let Ok(response_file) = std::env::var("MCP_RESPONSE_FILE") {
            // 写入到响应文件（用于异步轮询模式）
            std::fs::write(&response_file, &response_str)
                .map_err(|e| format!("写入响应文件失败: {}", e))?;
            log::info!("MCP响应已写入文件: {}", response_file);
        } else {
            // 传统模式：直接输出到stdout（MCP协议要求）
            println!("{}", response_str);
            std::io::Write::flush(&mut std::io::stdout())
                .map_err(|e| format!("刷新stdout失败: {}", e))?;
        }
    } else {
        // 通过channel发送响应（如果有的话）
        let sender = {
            let mut channel = state
                .response_channel
                .lock()
                .map_err(|e| format!("获取响应通道失败: {}", e))?;
            channel.take()
        };

        if let Some(sender) = sender {
            let _ = sender.send(response_str);
        }
    }

    Ok(())
}

fn resolve_spice_ids_in_dish_response(response: &mut serde_json::Value) -> Result<(), String> {
    let obj = match response.as_object_mut() {
        Some(o) => o,
        None => return Ok(()),
    };

    let ingredients_value = match obj.get_mut("ingredients") {
        Some(v) => v,
        None => return Ok(()),
    };

    let ingredients = match ingredients_value.as_array_mut() {
        Some(a) => a,
        None => return Ok(()),
    };

    if !ingredients.is_empty() {
        let has_spice_id = ingredients
            .iter()
            .any(|v| v.get("spice_id").and_then(|t| t.as_str()).is_some());
        if !has_spice_id {
            return Err("食材必须使用 spice_id 提交（旧的提交已移除）".to_string());
        }
    }

    let max_single_bytes: u64 = 8 * 1024 * 1024;
    let max_total_bytes: u64 = 16 * 1024 * 1024;
    let mut total_bytes: u64 = 0;

    let mut out: Vec<IngredientAttachment> = Vec::new();

    for item in ingredients.iter() {
        if let Some(spice_id) = item.get("spice_id").and_then(|t| t.as_str()) {
            let (bytes, label) = fetch_ingredient_bytes(spice_id)
                .map_err(|e| format!("读取食材失败: {}", e))?;

            if label.size_bytes > max_single_bytes {
                return Err("食材太大，建议换一份更小的内容或缩小截图范围".to_string());
            }
            total_bytes = total_bytes.saturating_add(label.size_bytes);
            if total_bytes > max_total_bytes {
                return Err("食材总大小太大，建议减少数量或换更小的内容".to_string());
            }

            let b64 = general_purpose::STANDARD.encode(bytes);
            out.push(IngredientAttachment {
                sauce: b64,
                dish_type: label.dish_type,
                tag: label.tag,
            });

            let _ = discard_spice(spice_id);
            continue;
        }

        return Err("食材必须使用 spice_id 提交（旧的提交已移除）".to_string());
    }

    *ingredients_value = serde_json::to_value(out)
        .map_err(|e| format!("处理食材失败: {}", e))?;
    Ok(())
}

 #[tauri::command]
 pub fn get_cli_args() -> Result<serde_json::Value, String> {
     let args: Vec<String> = std::env::args().collect();
     let mut result = serde_json::Map::new();

     // 检查是否有 --mcp-request 参数
     if args.len() >= 3 && args[1] == "--mcp-request" {
         result.insert(
             "mcp_request".to_string(),
             serde_json::Value::String(args[2].clone()),
         );
     }

     Ok(serde_json::Value::Object(result))
 }

 #[tauri::command]
 pub fn read_mcp_request(file_path: String) -> Result<serde_json::Value, String> {
     if !std::path::Path::new(&file_path).exists() {
         return Err(format!("文件不存在: {}", file_path));
     }

     match std::fs::read_to_string(&file_path) {
         Ok(content) => {
             if content.trim().is_empty() {
                 return Err("文件内容为空".to_string());
             }
             match serde_json::from_str(&content) {
                 Ok(json) => Ok(json),
                 Err(e) => Err(format!("解析JSON失败: {}", e)),
             }
         }
         Err(e) => Err(format!("读取文件失败: {}", e)),
     }
 }

#[tauri::command]
pub async fn stash_ingredient_bytes_cmd(
    bytes: Vec<u8>,
    dish_type: String,
    tag: Option<String>,
) -> Result<String, String> {
    let (normalized_bytes, normalized_dish_type) =
        normalize_ingredient_bytes(&bytes, dish_type.as_str())?;
    stash_ingredient_bytes(&normalized_bytes, normalized_dish_type.as_str(), tag)
        .map_err(|e| format!("保存食材失败: {}", e))
}

#[tauri::command]
pub async fn discard_spice_cmd(spice_id: String) -> Result<(), String> {
    discard_spice(&spice_id)
        .map_err(|e| format!("删除食材失败: {}", e))
}

#[tauri::command]
pub async fn read_clipboard_ingredients_cached() -> Result<Vec<CachedIngredient>, String> {
    let items = read_clipboard_ingredients_impl()?;
    let mut out: Vec<CachedIngredient> = Vec::new();
    for item in items {
        out.push(stash_ingredient(item.bytes, item.dish_type.as_str(), item.tag)?);
    }
    Ok(out)
}

fn read_clipboard_ingredients_impl() -> Result<Vec<ClipboardIngredientBytes>, String> {
    let mut clipboard = Clipboard::new().map_err(|e| format!("打开系统剪贴板失败: {}", e))?;

    match clipboard.get_image() {
        Ok(img) => {
            let width = u32::try_from(img.width).map_err(|_| "食材宽度过大，无法处理".to_string())?;
            let height = u32::try_from(img.height).map_err(|_| "食材高度过大，无法处理".to_string())?;

            let expected_len = img
                .width
                .checked_mul(img.height)
                .and_then(|v| v.checked_mul(4))
                .ok_or_else(|| "食材尺寸异常，无法处理".to_string())?;

            if img.bytes.len() < expected_len {
                return Err("剪贴板食材长度不正确".to_string());
            }

            let mut png_bytes: Vec<u8> = Vec::new();
            let encoder = PngEncoder::new(&mut png_bytes);
            encoder
                .write_image(
                    img.bytes.as_ref(),
                    width,
                    height,
                    ColorType::Rgba8.into(),
                )
                .map_err(|e| format!("编码 PNG 失败: {}", e))?;

            return Ok(vec![ClipboardIngredientBytes {
                dish_type: "image/png".to_string(),
                tag: None,
                bytes: png_bytes,
            }]);
        }
        Err(arboard::Error::ContentNotAvailable) => {}
        Err(e) => {
            let primary_err = format!("读取剪贴板食材失败: {}", e);
            if let Some(items) = try_read_ingredients_from_clipboard_text(&mut clipboard) {
                return Ok(items);
            }
            #[cfg(target_os = "windows")]
            {
                if let Some(items) = try_read_windows_clipboard_ingredient_files() {
                    return Ok(items);
                }
            }
            #[cfg(target_os = "macos")]
            {
                if let Some(items) = try_read_macos_clipboard_ingredient_files() {
                    return Ok(items);
                }
            }
            #[cfg(target_os = "linux")]
            {
                if let Some(items) = try_read_linux_clipboard_ingredient() {
                    return Ok(items);
                }
            }
            return Err(primary_err);
        }
    }

    if let Some(items) = try_read_ingredients_from_clipboard_text(&mut clipboard) {
        return Ok(items);
    }

    #[cfg(target_os = "windows")]
    {
        if let Some(items) = try_read_windows_clipboard_ingredient_files() {
            return Ok(items);
        }
    }

    #[cfg(target_os = "macos")]
    {
        if let Some(items) = try_read_macos_clipboard_ingredient_files() {
            return Ok(items);
        }
    }

    #[cfg(target_os = "linux")]
    {
        if let Some(items) = try_read_ingredients_from_linux_clipboard_text() {
            return Ok(items);
        }
    }

    #[cfg(target_os = "linux")]
    {
        if let Some(items) = try_read_linux_clipboard_ingredient() {
            return Ok(items);
        }
    }

    #[cfg(target_os = "linux")]
    {
        let wl_ok = linux_command_exists("wl-paste");
        let wl_clip_ok = linux_command_exists("wl-clip.paste");
        let xclip_ok = linux_command_exists("xclip");
        if !wl_ok && !wl_clip_ok && !xclip_ok {
            return Err("剪贴板里没有食材（Linux 下未检测到 wl-paste / wl-clip.paste / xclip：Wayland 建议安装 wl-clipboard 或 snap 的 wl-clip）".to_string());
        }
    }

    Err("剪贴板里没有食材".to_string())
}

fn linux_command_exists(cmd: &str) -> bool {
    #[cfg(target_os = "linux")]
    {
        run_linux_command_output(cmd, &["--version"]).is_some()
    }

    #[cfg(not(target_os = "linux"))]
    {
        let _ = cmd;
        false
    }
}

fn try_read_ingredients_from_clipboard_text(
    clipboard: &mut Clipboard,
) -> Option<Vec<ClipboardIngredientBytes>> {
    let text = clipboard.get_text().ok()?;
    let paths = extract_file_paths_from_clipboard_text(&text);
    if paths.is_empty() {
        return None;
    }

    let mut out: Vec<ClipboardIngredientBytes> = Vec::new();
    for p in paths {
        if let Some(item) = try_load_ingredient_file_as_clipboard_item(&p) {
            out.push(item);
        }
    }

    if out.is_empty() { None } else { Some(out) }
}

fn extract_file_paths_from_clipboard_text(text: &str) -> Vec<PathBuf> {
    let mut lines = text
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>();

    if matches!(lines.first().copied(), Some("copy") | Some("cut")) {
        lines.remove(0);
    }

    let mut out = Vec::new();
    for line in lines {
        if line.starts_with('#') {
            continue;
        }

        if let Some(p) = parse_file_url_or_path(line) {
            out.push(p);
        }
    }
    out
}

fn parse_file_url_or_path(s: &str) -> Option<PathBuf> {
    let raw = s.trim();

    if let Some(rest) = raw.strip_prefix("file://") {
        let rest = rest.strip_prefix("localhost").unwrap_or(rest);
        let mut path_str = rest.to_string();
        if !path_str.starts_with('/') {
            path_str.insert(0, '/');
        }
        let decoded = percent_decode_str(&path_str).decode_utf8_lossy();
        let decoded = decoded.as_ref();
        let decoded = if decoded.len() >= 4
            && decoded.starts_with('/')
            && decoded.as_bytes()[2] == b':'
            && (decoded.as_bytes()[1] as char).is_ascii_alphabetic()
            && (decoded.as_bytes()[3] == b'/' || decoded.as_bytes()[3] == b'\\')
        {
            &decoded[1..]
        } else {
            decoded
        };
        let p = PathBuf::from(decoded);
        if p.exists() { return Some(p); }
        return None;
    }

    let p = PathBuf::from(raw);
    if p.exists() {
        return Some(p);
    }
    None
}

fn try_load_ingredient_file_as_clipboard_item(path: &PathBuf) -> Option<ClipboardIngredientBytes> {
    let mime = guess_ingredient_mime_from_path(path)?;
    let bytes = fs::read(path).ok()?;
    if bytes.is_empty() {
        return None;
    }
    Some(ClipboardIngredientBytes {
        dish_type: mime.to_string(),
        tag: None,
        bytes,
    })
}

#[cfg(target_os = "windows")]
fn try_read_windows_clipboard_ingredient_files() -> Option<Vec<ClipboardIngredientBytes>> {
    let script = "[Console]::OutputEncoding=[System.Text.Encoding]::UTF8; Get-Clipboard -Format FileDropList -ErrorAction SilentlyContinue | ForEach-Object { $_.FullName }";
    let output = Command::new("powershell")
        .args(["-NoProfile", "-Sta", "-Command", script])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?;
    let mut out = Vec::new();
    for line in text.lines().map(|l| l.trim()).filter(|l| !l.is_empty()) {
        let p = PathBuf::from(line);
        if let Some(item) = try_load_ingredient_file_as_clipboard_item(&p) {
            out.push(item);
        }
    }
    if out.is_empty() { None } else { Some(out) }
}

#[cfg(target_os = "macos")]
fn try_read_macos_clipboard_ingredient_files() -> Option<Vec<ClipboardIngredientBytes>> {
    let output = Command::new("osascript")
        .args([
            "-e",
            "set out to \"\"",
            "-e",
            "try",
            "-e",
            "set theFiles to the clipboard as alias list",
            "-e",
            "on error",
            "-e",
            "set theFiles to {the clipboard as alias}",
            "-e",
            "end try",
            "-e",
            "repeat with f in theFiles",
            "-e",
            "set out to out & (POSIX path of f) & linefeed",
            "-e",
            "end repeat",
            "-e",
            "return out",
        ])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?;
    let mut out = Vec::new();
    for line in text.lines().map(|l| l.trim()).filter(|l| !l.is_empty()) {
        let p = PathBuf::from(line);
        if let Some(item) = try_load_ingredient_file_as_clipboard_item(&p) {
            out.push(item);
        }
    }
    if out.is_empty() { None } else { Some(out) }
}

fn guess_ingredient_mime_from_path(path: &PathBuf) -> Option<&'static str> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    match ext.as_str() {
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "webp" => Some("image/webp"),
        "gif" => Some("image/gif"),
        "bmp" => Some("image/bmp"),
        "tif" | "tiff" => Some("image/tiff"),
        _ => None,
    }
}

#[cfg(target_os = "linux")]
fn try_read_linux_clipboard_ingredient() -> Option<Vec<ClipboardIngredientBytes>> {
    let candidates: Vec<(&str, Vec<&str>, &str)> = vec![
        ("wl-paste", vec!["--no-newline", "--type", "image/png"], "image/png"),
        ("wl-paste", vec!["--no-newline", "--type", "image/jpeg"], "image/jpeg"),
        ("wl-paste", vec!["--no-newline", "--type", "image/webp"], "image/webp"),
        ("wl-paste", vec!["--no-newline", "--type", "image/bmp"], "image/bmp"),
        ("wl-paste", vec!["--primary", "--no-newline", "--type", "image/png"], "image/png"),
        ("wl-paste", vec!["--primary", "--no-newline", "--type", "image/jpeg"], "image/jpeg"),
        ("wl-paste", vec!["--primary", "--no-newline", "--type", "image/webp"], "image/webp"),
        ("wl-paste", vec!["--primary", "--no-newline", "--type", "image/bmp"], "image/bmp"),

        ("wl-clip.paste", vec!["--no-newline", "--type", "image/png"], "image/png"),
        ("wl-clip.paste", vec!["--no-newline", "--type", "image/jpeg"], "image/jpeg"),
        ("wl-clip.paste", vec!["--no-newline", "--type", "image/webp"], "image/webp"),
        ("wl-clip.paste", vec!["--no-newline", "--type", "image/bmp"], "image/bmp"),
        ("wl-clip.paste", vec!["--primary", "--no-newline", "--type", "image/png"], "image/png"),
        ("wl-clip.paste", vec!["--primary", "--no-newline", "--type", "image/jpeg"], "image/jpeg"),
        ("wl-clip.paste", vec!["--primary", "--no-newline", "--type", "image/webp"], "image/webp"),
        ("wl-clip.paste", vec!["--primary", "--no-newline", "--type", "image/bmp"], "image/bmp"),
        ("xclip", vec!["-selection", "clipboard", "-t", "image/png", "-o"], "image/png"),
        ("xclip", vec!["-selection", "clipboard", "-t", "image/jpeg", "-o"], "image/jpeg"),
        ("xclip", vec!["-selection", "clipboard", "-t", "image/webp", "-o"], "image/webp"),
        ("xclip", vec!["-selection", "clipboard", "-t", "image/bmp", "-o"], "image/bmp"),
        ("xclip", vec!["-selection", "primary", "-t", "image/png", "-o"], "image/png"),
        ("xclip", vec!["-selection", "primary", "-t", "image/jpeg", "-o"], "image/jpeg"),
        ("xclip", vec!["-selection", "primary", "-t", "image/webp", "-o"], "image/webp"),
        ("xclip", vec!["-selection", "primary", "-t", "image/bmp", "-o"], "image/bmp"),
    ];

    for (cmd, args, mime) in candidates {
        let output = match run_linux_command_output(cmd, &args) {
            Some(o) => o,
            None => continue,
        };
        if output.status.success() && !output.stdout.is_empty() {
            return Some(vec![ClipboardIngredientBytes {
                dish_type: mime.to_string(),
                tag: None,
                bytes: output.stdout,
            }]);
        }
    }
    None
}

#[tauri::command]
pub async fn open_external_url(url: String) -> Result<(), String> {
    use std::process::Command;

    // 移除不重要的调试信息

    // 根据操作系统选择合适的命令
    let result = if cfg!(target_os = "windows") {
        Command::new("cmd")
            .args(["/C", "start", &url])
            .spawn()
    } else if cfg!(target_os = "macos") {
        Command::new("open")
            .arg(&url)
            .spawn()
    } else {
        // Linux 和其他 Unix 系统
        Command::new("xdg-open")
            .arg(&url)
            .spawn()
    };

    match result {
        Ok(_) => Ok(()),
        Err(e) => Err(format!("无法打开链接: {}", e))
    }
}

#[tauri::command]
pub async fn exit_app(app: AppHandle) -> Result<(), String> {
    // 直接调用强制退出，用于程序内部的退出操作（如MCP响应后退出）
    crate::ui::exit::force_exit_app(app).await
}



/// 处理应用退出请求（用于前端退出快捷键）
#[tauri::command]
pub async fn handle_app_exit_request(app: AppHandle) -> Result<bool, String> {
    crate::ui::exit_handler::handle_exit_request_internal(app).await
}

/// 构建继续操作的MCP响应
#[tauri::command]
pub fn build_mcp_continue_response(
    request_id: Option<String>,
    source: String,
) -> Result<String, String> {
    Ok(build_refill_response(request_id, &source))
}

/// 创建测试popup窗口
#[tauri::command]
pub async fn create_test_popup(request: serde_json::Value) -> Result<String, String> {
    // 将JSON值转换为PopupRequest
    let popup_request: PopupRequest = serde_json::from_value(request)
        .map_err(|e| format!("解析请求参数失败: {}", e))?;

    // 调用现有的popup创建函数
    match create_tauri_popup(&popup_request) {
        Ok(response) => Ok(response),
        Err(e) => Err(format!("创建测试popup失败: {}", e))
    }
}

// 自定义prompt相关命令

/// 获取自定义prompt配置
#[tauri::command]
pub async fn get_custom_prompt_config(state: State<'_, AppState>) -> Result<CustomPromptConfig, String> {
    let config = state
        .config
        .lock()
        .map_err(|e| format!("获取配置失败: {}", e))?;
    Ok(config.custom_prompt_config.clone())
}

/// 添加自定义prompt
#[tauri::command]
pub async fn add_custom_prompt(
    prompt: CustomPrompt,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<(), String> {
    {
        let mut config = state
            .config
            .lock()
            .map_err(|e| format!("获取配置失败: {}", e))?;

        // 检查是否超过最大数量限制
        if config.custom_prompt_config.prompts.len() >= config.custom_prompt_config.max_prompts as usize {
            return Err(format!("自定义prompt数量已达到上限: {}", config.custom_prompt_config.max_prompts));
        }

        // 检查ID是否已存在
        if config.custom_prompt_config.prompts.iter().any(|p| p.id == prompt.id) {
            return Err("prompt ID已存在".to_string());
        }

        config.custom_prompt_config.prompts.push(prompt);
    }

    // 保存配置到文件
    save_config(&state, &app)
        .await
        .map_err(|e| format!("保存配置失败: {}", e))?;

    Ok(())
}

/// 更新自定义prompt
#[tauri::command]
pub async fn update_custom_prompt(
    prompt: CustomPrompt,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<(), String> {
    {
        let mut config = state
            .config
            .lock()
            .map_err(|e| format!("获取配置失败: {}", e))?;

        // 查找并更新prompt
        if let Some(existing_prompt) = config.custom_prompt_config.prompts.iter_mut().find(|p| p.id == prompt.id) {
            *existing_prompt = prompt;
        } else {
            return Err("未找到指定的prompt".to_string());
        }
    }

    // 保存配置到文件
    save_config(&state, &app)
        .await
        .map_err(|e| format!("保存配置失败: {}", e))?;

    Ok(())
}

/// 删除自定义prompt
#[tauri::command]
pub async fn delete_custom_prompt(
    prompt_id: String,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<(), String> {
    {
        let mut config = state
            .config
            .lock()
            .map_err(|e| format!("获取配置失败: {}", e))?;

        // 查找并删除prompt
        let initial_len = config.custom_prompt_config.prompts.len();
        config.custom_prompt_config.prompts.retain(|p| p.id != prompt_id);

        if config.custom_prompt_config.prompts.len() == initial_len {
            return Err("未找到指定的prompt".to_string());
        }
    }

    // 保存配置到文件
    save_config(&state, &app)
        .await
        .map_err(|e| format!("保存配置失败: {}", e))?;

    Ok(())
}

/// 设置自定义prompt启用状态
#[tauri::command]
pub async fn set_custom_prompt_enabled(
    enabled: bool,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<(), String> {
    {
        let mut config = state
            .config
            .lock()
            .map_err(|e| format!("获取配置失败: {}", e))?;
        config.custom_prompt_config.enabled = enabled;
    }

    // 保存配置到文件
    save_config(&state, &app)
        .await
        .map_err(|e| format!("保存配置失败: {}", e))?;

    Ok(())
}

/// 更新自定义prompt排序
#[tauri::command]
pub async fn update_custom_prompt_order(
    prompt_ids: Vec<String>,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<(), String> {
    log::debug!("开始更新prompt排序，接收到的IDs: {:?}", prompt_ids);

    {
        let mut config = state
            .config
            .lock()
            .map_err(|e| format!("获取配置失败: {}", e))?;

        log::debug!("更新前的prompt顺序:");
        for prompt in &config.custom_prompt_config.prompts {
            log::debug!("  {} (sort_order: {})", prompt.name, prompt.sort_order);
        }

        // 根据新的顺序更新sort_order
        for (index, prompt_id) in prompt_ids.iter().enumerate() {
            if let Some(prompt) = config.custom_prompt_config.prompts.iter_mut().find(|p| p.id == *prompt_id) {
                let old_order = prompt.sort_order;
                prompt.sort_order = (index + 1) as i32;
                prompt.updated_at = chrono::Utc::now().to_rfc3339();
                log::debug!("更新prompt '{}': {} -> {}", prompt.name, old_order, prompt.sort_order);
            }
        }

        // 按sort_order排序
        config.custom_prompt_config.prompts.sort_by_key(|p| p.sort_order);

        log::debug!("更新后的prompt顺序:");
        for prompt in &config.custom_prompt_config.prompts {
            log::debug!("  {} (sort_order: {})", prompt.name, prompt.sort_order);
        }
    }

    log::debug!("开始保存配置文件...");
    let save_start = std::time::Instant::now();

    // 保存配置到文件
    save_config(&state, &app)
        .await
        .map_err(|e| format!("保存配置失败: {}", e))?;

    let save_duration = save_start.elapsed();
    log::debug!("配置保存完成，耗时: {:?}", save_duration);

    Ok(())
}

/// 更新条件性prompt状态
#[tauri::command]
pub async fn update_conditional_prompt_state(
    prompt_id: String,
    new_state: bool,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<(), String> {
    {
        let mut config = state
            .config
            .lock()
            .map_err(|e| format!("获取配置失败: {}", e))?;

        // 查找并更新指定prompt的current_state
        if let Some(prompt) = config.custom_prompt_config.prompts.iter_mut().find(|p| p.id == prompt_id) {
            prompt.current_state = new_state;
            prompt.updated_at = chrono::Utc::now().to_rfc3339();
        } else {
            return Err(format!("未找到ID为 {} 的prompt", prompt_id));
        }
    }

    // 保存配置到文件
    save_config(&state, &app)
        .await
        .map_err(|e| format!("保存配置失败: {}", e))?;

    Ok(())
}





/// 获取配置文件的真实路径
#[tauri::command]
pub async fn get_config_file_path(app: AppHandle) -> Result<String, String> {
    let config_path = crate::config::get_config_path(&app)
        .map_err(|e| format!("获取配置文件路径失败: {}", e))?;

    // 获取绝对路径
    let absolute_path = if config_path.is_absolute() {
        config_path
    } else {
        // 如果是相对路径，获取当前工作目录并拼接
        std::env::current_dir()
            .unwrap_or_else(|_| std::path::PathBuf::from("."))
            .join(&config_path)
    };

    // 跨平台路径规范化
    let normalized_path = normalize_path_display(&absolute_path);

    Ok(normalized_path)
}

/// 跨平台路径显示规范化
fn normalize_path_display(path: &std::path::Path) -> String {
    // 如果文件存在，尝试获取规范路径
    let canonical_path = if path.exists() {
        match path.canonicalize() {
            Ok(canonical) => Some(canonical),
            Err(_) => None,
        }
    } else {
        None
    };

    let display_path = canonical_path.as_ref().map(|p| p.as_path()).unwrap_or(path);
    let path_str = display_path.to_string_lossy();

    // 处理不同平台的路径格式
    #[cfg(target_os = "windows")]
    {
        // Windows: 移除长路径前缀 \\?\
        if path_str.starts_with(r"\\?\") {
            path_str[4..].to_string()
        } else {
            path_str.to_string()
        }
    }

    #[cfg(target_os = "macos")]
    {
        // macOS: 处理可能的符号链接和特殊路径
        path_str.to_string()
    }

    #[cfg(target_os = "linux")]
    {
        // Linux: 标准Unix路径处理
        path_str.to_string()
    }

    #[cfg(target_os = "ios")]
    {
        // iOS: 类似macOS的处理
        path_str.to_string()
    }

    #[cfg(target_os = "android")]
    {
        // Android: 类似Linux的处理
        path_str.to_string()
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux", target_os = "ios", target_os = "android")))]
    {
        // 其他平台: 通用处理
        path_str.to_string()
    }
}

// 快捷键相关命令

/// 获取快捷键配置
#[tauri::command]
pub async fn get_shortcut_config(state: State<'_, AppState>) -> Result<ShortcutConfig, String> {
    let config = state
        .config
        .lock()
        .map_err(|e| format!("获取配置失败: {}", e))?;
    Ok(config.shortcut_config.clone())
}

/// 更新快捷键绑定
#[tauri::command]
pub async fn update_shortcut_binding(
    shortcut_id: String,
    binding: ShortcutBinding,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<(), String> {
    {
        let mut config = state
            .config
            .lock()
            .map_err(|e| format!("获取配置失败: {}", e))?;

        // 更新指定的快捷键绑定
        config.shortcut_config.shortcuts.insert(shortcut_id, binding);
    }

    // 保存配置到文件
    save_config(&state, &app)
        .await
        .map_err(|e| format!("保存配置失败: {}", e))?;

    Ok(())
}



/// 重置快捷键为默认值
#[tauri::command]
pub async fn reset_shortcuts_to_default(
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<(), String> {
    {
        let mut config = state
            .config
            .lock()
            .map_err(|e| format!("获取配置失败: {}", e))?;
        config.shortcut_config = crate::config::default_shortcut_config();
    }

    // 保存配置到文件
    save_config(&state, &app)
        .await
        .map_err(|e| format!("保存配置失败: {}", e))?;

    Ok(())
}
