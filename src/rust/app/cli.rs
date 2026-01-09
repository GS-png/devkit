use crate::config::load_standalone_telegram_config;
use crate::telegram::handle_telegram_only_mcp_request;
use crate::log_important;
use crate::app::builder::run_tauri_app;
use anyhow::Result;

/// 处理命令行参数
pub fn handle_cli_args() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    
    // Parse arguments
    let mut request_file: Option<String> = None;
    let mut response_file: Option<String> = None;
    let mut i = 1;
    
    while i < args.len() {
        match args[i].as_str() {
            "--help" | "-h" => {
                print_help();
                return Ok(());
            }
            "--version" | "-v" => {
                print_version();
                return Ok(());
            }
            "--mcp-request" => {
                if i + 1 < args.len() {
                    request_file = Some(args[i + 1].clone());
                    i += 2;
                } else {
                    eprintln!("--mcp-request requires a file path");
                    std::process::exit(1);
                }
            }
            "--response-file" => {
                if i + 1 < args.len() {
                    response_file = Some(args[i + 1].clone());
                    i += 2;
                } else {
                    eprintln!("--response-file requires a file path");
                    std::process::exit(1);
                }
            }
            _ => {
                eprintln!("未知参数: {}", args[i]);
                print_help();
                std::process::exit(1);
            }
        }
    }
    
    // No arguments - start GUI normally
    if request_file.is_none() && response_file.is_none() && args.len() == 1 {
        run_tauri_app();
        return Ok(());
    }
    
    // MCP request mode
    if let Some(req_file) = request_file {
        // Store response file path in environment for UI to use
        if let Some(resp_file) = response_file {
            std::env::set_var("MCP_RESPONSE_FILE", resp_file);
        }
        handle_mcp_request(&req_file)?;
    } else {
        eprintln!("无效的命令行参数");
        print_help();
        std::process::exit(1);
    }

    Ok(())
}

/// 处理MCP请求
fn handle_mcp_request(request_file: &str) -> Result<()> {
    // 检查Telegram配置，决定是否启用纯Telegram模式
    match load_standalone_telegram_config() {
        Ok(telegram_config) => {
            if telegram_config.enabled && telegram_config.hide_frontend_popup {
                // 纯Telegram模式：不启动GUI，直接处理
                if let Err(e) = tokio::runtime::Runtime::new()
                    .unwrap()
                    .block_on(handle_telegram_only_mcp_request(request_file))
                {
                    log_important!(error, "处理Telegram请求失败: {}", e);
                    std::process::exit(1);
                }
            } else {
                // 正常模式：启动GUI处理弹窗
                run_tauri_app();
            }
        }
        Err(e) => {
            log_important!(warn, "加载Telegram配置失败: {}，使用默认GUI模式", e);
            // 配置加载失败时，使用默认行为（启动GUI）
            run_tauri_app();
        }
    }
    Ok(())
}

/// 显示帮助信息
fn print_help() {
    println!("三术 - 智能代码审查工具");
    println!();
    println!("用法:");
    println!("  等一下                    启动设置界面");
    println!("  等一下 --mcp-request <文件>  处理 MCP 请求");
    println!("  等一下 --help             显示此帮助信息");
    println!("  等一下 --version          显示版本信息");
}

/// 显示版本信息
fn print_version() {
    println!("三术 v{}", env!("CARGO_PKG_VERSION"));
}
