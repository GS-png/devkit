use anyhow::Result;
use std::process::Command;
use std::fs;
use std::path::Path;

use crate::mcp::types::PopupRequest;

static DEV_SERVER_CHILD: std::sync::LazyLock<std::sync::Mutex<Option<std::process::Child>>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(None));

/// Create UI popup
///
/// Prefers UI command in same directory as MCP server, falls back to global
pub fn create_tauri_popup(request: &PopupRequest) -> Result<String> {
    // Create temp request file - cross platform
    let temp_dir = std::env::temp_dir();
    let temp_file = temp_dir.join(format!("mcp_request_{}.json", request.id));
    let request_json = serde_json::to_string_pretty(request)?;
    fs::write(&temp_file, request_json)?;

    // Find UI command path
    let command_path = find_ui_command()?;

    // Execute UI command
    let output = Command::new(&command_path)
        .arg("--mcp-request")
        .arg(temp_file.to_string_lossy().to_string())
        .output()?;

    // Cleanup temp file
    let _ = fs::remove_file(&temp_file);

    if output.status.success() {
        let response = String::from_utf8_lossy(&output.stdout);
        let response = response.trim();
        if response.is_empty() {
            Ok("CANCELLED".to_string())
        } else {
            Ok(response.to_string())
        }
    } else {
        let error = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("UI process failed: {}", error);
    }
}

/// Find UI command path
///
/// Priority: same directory -> global -> development
pub fn find_ui_command() -> Result<String> {
    let ui_path_override = std::env::var("SANSHU_UI_PATH")
        .or_else(|_| std::env::var("MCP_UI_PATH"))
        .ok()
        .map(std::path::PathBuf::from);

    let ui_mode = std::env::var("SANSHU_UI_MODE")
        .or_else(|_| std::env::var("MCP_UI_MODE"))
        .unwrap_or_default();
    let explicit_debug = matches!(ui_mode.as_str(), "debug" | "dev") || (ui_mode.is_empty() && cfg!(debug_assertions));
    let mut dev_listening = is_vite_dev_server_listening();
    if explicit_debug && !dev_listening && auto_dev_enabled() {
        ensure_vite_dev_server_running()?;
        dev_listening = is_vite_dev_server_listening();
    }

    if let Some(path) = ui_path_override {
        if path.exists() && is_executable(&path) {
            return Ok(path.to_string_lossy().to_string());
        }
    }
    let prefer_debug = explicit_debug || dev_listening;
    if prefer_debug {
        let repo_debug_ui_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("debug")
            .join("sanshu-ui");
        if repo_debug_ui_path.exists() && is_executable(&repo_debug_ui_path) {
            return Ok(repo_debug_ui_path.to_string_lossy().to_string());
        }

        if let Ok(current_exe) = std::env::current_exe() {
            if let Some(target_dir) = current_exe
                .ancestors()
                .find(|p| p.file_name().and_then(|s| s.to_str()) == Some("target"))
            {
                let debug_ui_path = target_dir.join("debug").join("sanshu-ui");
                if debug_ui_path.exists() && is_executable(&debug_ui_path) {
                    return Ok(debug_ui_path.to_string_lossy().to_string());
                }
            }
        }
    }

    let repo_release_ui_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("release")
        .join("sanshu-ui");
    if repo_release_ui_path.exists() && is_executable(&repo_release_ui_path) {
        return Ok(repo_release_ui_path.to_string_lossy().to_string());
    }

    // 1. Try UI command in same directory as MCP server
    if let Ok(current_exe) = std::env::current_exe() {
        if let Some(exe_dir) = current_exe.parent() {
            if let Some(target_dir) = current_exe
                .ancestors()
                .find(|p| p.file_name().and_then(|s| s.to_str()) == Some("target"))
            {
                let release_ui_path = target_dir.join("release").join("sanshu-ui");
                if release_ui_path.exists() && is_executable(&release_ui_path) {
                    return Ok(release_ui_path.to_string_lossy().to_string());
                }
            }
            // Try new name first
            let local_ui_path = exe_dir.join("sanshu-ui");
            if local_ui_path.exists() && is_executable(&local_ui_path) {
                return Ok(local_ui_path.to_string_lossy().to_string());
            }
            // Fallback to old name for compatibility
            let legacy_ui_path = exe_dir.join("等一下");
            if legacy_ui_path.exists() && is_executable(&legacy_ui_path) {
                return Ok(legacy_ui_path.to_string_lossy().to_string());
            }
        }
    }

    // 2. Try global command
    if test_command_available("sanshu-ui") {
        return Ok("sanshu-ui".to_string());
    }
    // Fallback to old name for compatibility
    if test_command_available("等一下") {
        return Ok("等一下".to_string());
    }

    // 3. Return detailed error
    anyhow::bail!(
        "UI command not found. Please ensure:\n\
         1. Project is built: cargo build --release\n\
         2. Or globally installed: ./install.sh\n\
         3. Or sanshu-ui is in the same directory"
    )
}

/// Test if command is available
fn test_command_available(command: &str) -> bool {
    Command::new(command)
        .arg("--version")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

fn auto_dev_enabled() -> bool {
    let v = std::env::var("SANSHU_AUTO_DEV_SERVER")
        .or_else(|_| std::env::var("MCP_AUTO_DEV_SERVER"))
        .unwrap_or_else(|_| "1".to_string());
    let v = v.trim();
    !(v == "0" || v.eq_ignore_ascii_case("false") || v.eq_ignore_ascii_case("off"))
}

fn ensure_vite_dev_server_running() -> Result<()> {
    if is_vite_dev_server_listening() {
        return Ok(());
    }

    let project_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    if !project_root.join("node_modules").exists() {
        anyhow::bail!(
            "Debug UI requires frontend dev server but node_modules is missing. Please run: pnpm install"
        );
    }

    if !test_command_available("pnpm") {
        anyhow::bail!("pnpm not found in PATH. Please install pnpm or switch to release UI.");
    }

    let log_path = std::path::Path::new("/tmp").join("sanshu_pnpm_dev.log");
    let log_file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .map_err(|e| anyhow::anyhow!("Failed to open dev server log file {}: {}", log_path.display(), e))?;
    let log_file_err = log_file
        .try_clone()
        .map_err(|e| anyhow::anyhow!("Failed to clone dev server log file handle: {}", e))?;

    {
        let mut child_opt = DEV_SERVER_CHILD.lock().unwrap();
        if let Some(child) = child_opt.as_mut() {
            if let Ok(None) = child.try_wait() {
                return Ok(());
            }
        }

        let child = Command::new("pnpm")
            .arg("dev")
            .arg("--")
            .arg("--host")
            .arg("127.0.0.1")
            .arg("--port")
            .arg("5176")
            .arg("--strictPort")
            .current_dir(project_root)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::from(log_file))
            .stderr(std::process::Stdio::from(log_file_err))
            .spawn()
            .map_err(|e| anyhow::anyhow!("Failed to start pnpm dev: {}", e))?;

        *child_opt = Some(child);
    }

    let mut waited_ms: u64 = 0;
    let step_ms: u64 = 150;
    let max_wait_ms: u64 = std::env::var("SANSHU_DEV_WAIT_MS")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(8_000);
    while waited_ms < max_wait_ms {
        if is_vite_dev_server_listening() {
            return Ok(());
        }
        std::thread::sleep(std::time::Duration::from_millis(step_ms));
        waited_ms = waited_ms.saturating_add(step_ms);
    }

    anyhow::bail!(
        "pnpm dev did not become ready on 127.0.0.1:5176 within {}ms. See log: {}",
        max_wait_ms,
        log_path.display()
    )
}

fn is_vite_dev_server_listening() -> bool {
    let addr = std::net::SocketAddr::new(
        std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST),
        5176,
    );
    std::net::TcpStream::connect_timeout(&addr, std::time::Duration::from_millis(120)).is_ok()
}

/// Check if file is executable
fn is_executable(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        path.metadata()
            .map(|metadata| metadata.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    }

    #[cfg(windows)]
    {
        // Windows 上检查文件扩展名
        path.extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.eq_ignore_ascii_case("exe"))
            .unwrap_or(false)
    }
}
