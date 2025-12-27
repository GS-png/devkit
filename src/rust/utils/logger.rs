use std::env;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::Once;
use std::time::{SystemTime, UNIX_EPOCH};
use log::LevelFilter;
use env_logger::{Builder, Target};

static INIT: Once = Once::new();

/// 日志轮转配置
#[derive(Debug, Clone)]
pub struct LogRotationConfig {
    /// 单个日志文件最大大小（字节），默认 200MB
    pub max_size_bytes: u64,
    /// 日志文件保留天数，默认 7 天
    pub retention_days: u32,
    /// 最大备份文件数量，默认 5 个
    pub max_backup_count: u32,
}

impl Default for LogRotationConfig {
    fn default() -> Self {
        Self {
            max_size_bytes: 200 * 1024 * 1024, // 200MB
            retention_days: 7,
            max_backup_count: 5,
        }
    }
}

/// 日志配置
#[derive(Debug, Clone)]
pub struct LogConfig {
    /// 日志级别
    pub level: LevelFilter,
    /// 日志文件路径（None 表示不输出到文件）
    pub file_path: Option<String>,
    /// 是否为 MCP 模式（MCP 模式下不输出到 stderr）
    pub is_mcp_mode: bool,
    /// 日志轮转配置
    pub rotation: LogRotationConfig,
}

impl Default for LogConfig {
    fn default() -> Self {
        Self {
            level: LevelFilter::Warn,
            file_path: None,
            is_mcp_mode: false,
            rotation: LogRotationConfig::default(),
        }
    }
}

/// 获取 GUI 模式的日志文件路径
/// 使用 dirs::config_dir() 确保跨平台兼容性
/// Windows: C:\Users\<用户>\AppData\Roaming\sanshu\log\acemcp.log
/// Linux: ~/.config/sanshu/log/acemcp.log
/// macOS: ~/Library/Application Support/sanshu/log/acemcp.log
fn get_gui_log_path() -> Option<PathBuf> {
    dirs::config_dir().map(|config_dir| {
        config_dir.join("sanshu").join("log").join("acemcp.log")
    })
}

/// 确保日志目录存在
fn ensure_log_directory(log_path: &PathBuf) -> std::io::Result<()> {
    if let Some(parent) = log_path.parent() {
        if !parent.exists() {
            fs::create_dir_all(parent)?;
        }
    }
    Ok(())
}

/// 执行日志轮转
/// 检查日志文件大小并进行轮转，同时清理过期日志
fn rotate_log_if_needed(log_path: &PathBuf, rotation_config: &LogRotationConfig) {
    // 检查当前日志文件大小
    if let Ok(metadata) = fs::metadata(log_path) {
        if metadata.len() >= rotation_config.max_size_bytes {
            // 需要轮转：将现有日志文件重命名
            perform_log_rotation(log_path, rotation_config.max_backup_count);
        }
    }
    
    // 清理过期日志文件
    cleanup_old_logs(log_path, rotation_config);
}

/// 执行日志文件轮转
/// acemcp.log -> acemcp.log.1 -> acemcp.log.2 ...
fn perform_log_rotation(log_path: &PathBuf, max_backup_count: u32) {
    let log_dir = match log_path.parent() {
        Some(dir) => dir,
        None => return,
    };
    
    let log_name = match log_path.file_name().and_then(|n| n.to_str()) {
        Some(name) => name,
        None => return,
    };
    
    // 删除最旧的备份（如果存在）
    let oldest_backup = log_dir.join(format!("{}.{}", log_name, max_backup_count));
    let _ = fs::remove_file(&oldest_backup);
    
    // 将现有备份依次重命名（从后往前）
    for i in (1..max_backup_count).rev() {
        let from = log_dir.join(format!("{}.{}", log_name, i));
        let to = log_dir.join(format!("{}.{}", log_name, i + 1));
        if from.exists() {
            let _ = fs::rename(&from, &to);
        }
    }
    
    // 将当前日志文件重命名为 .1
    let first_backup = log_dir.join(format!("{}.1", log_name));
    let _ = fs::rename(log_path, &first_backup);
}

/// 清理过期的日志备份文件
fn cleanup_old_logs(log_path: &PathBuf, rotation_config: &LogRotationConfig) {
    let log_dir = match log_path.parent() {
        Some(dir) => dir,
        None => return,
    };
    
    let log_name = match log_path.file_name().and_then(|n| n.to_str()) {
        Some(name) => name,
        None => return,
    };
    
    // 计算过期时间阈值（当前时间 - 保留天数）
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let retention_secs = rotation_config.retention_days as u64 * 24 * 60 * 60;
    let threshold = now.saturating_sub(retention_secs);
    
    // 遍历备份文件并删除过期的
    for i in 1..=rotation_config.max_backup_count {
        let backup_path = log_dir.join(format!("{}.{}", log_name, i));
        if backup_path.exists() {
            if let Ok(metadata) = fs::metadata(&backup_path) {
                if let Ok(modified) = metadata.modified() {
                    if let Ok(duration) = modified.duration_since(UNIX_EPOCH) {
                        if duration.as_secs() < threshold {
                            // 文件已过期，删除
                            let _ = fs::remove_file(&backup_path);
                        }
                    }
                }
            }
        }
    }
}

/// 初始化日志系统
pub fn init_logger(config: LogConfig) -> Result<(), Box<dyn std::error::Error>> {
    INIT.call_once(|| {
        let mut builder = Builder::new();
        
        // 设置日志级别
        builder.filter_level(config.level);
        
        // 设置日志格式
        builder.format(|buf, record| {
            let log_line = format!(
                "{} [{}] [{}] {}",
                chrono::Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"),
                record.level(),
                record.module_path().unwrap_or("unknown"),
                record.args()
            );
            
            // 写入到原始目标（stderr 或文件）
            writeln!(buf, "{}", log_line)?;
            
            Ok(())
        });
        
        // 根据模式设置输出目标
        if config.is_mcp_mode {
            // MCP 模式：只输出到文件，不输出到 stderr
            if let Some(file_path) = &config.file_path {
                let log_path = PathBuf::from(file_path);
                
                // 确保日志目录存在
                let _ = ensure_log_directory(&log_path);
                
                // 执行日志轮转检查
                rotate_log_if_needed(&log_path, &config.rotation);
                
                if let Ok(log_file) = OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&log_path) 
                {
                    builder.target(Target::Pipe(Box::new(log_file)));
                } else {
                    // 如果文件打开失败，禁用日志输出
                    builder.filter_level(LevelFilter::Off);
                }
            } else {
                // MCP 模式下没有指定文件路径，禁用日志输出
                builder.filter_level(LevelFilter::Off);
            }
        } else {
            // 非 MCP 模式：同时输出到文件和 stderr
            if let Some(file_path) = &config.file_path {
                let log_path = PathBuf::from(file_path);
                
                // 确保日志目录存在
                let _ = ensure_log_directory(&log_path);
                
                // 执行日志轮转检查
                rotate_log_if_needed(&log_path, &config.rotation);
                
                // 尝试打开文件，如果成功则同时输出到文件和 stderr
                if let Ok(log_file) = OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&log_path) 
                {
                    // 使用自定义目标，同时写入文件和 stderr
                    use std::io::Write;
                    struct DualWriter {
                        file: std::fs::File,
                    }
                    impl Write for DualWriter {
                        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
                            let written = self.file.write(buf)?;
                            let _ = std::io::stderr().write_all(buf);
                            Ok(written)
                        }
                        fn flush(&mut self) -> std::io::Result<()> {
                            self.file.flush()?;
                            std::io::stderr().flush()
                        }
                    }
                    builder.target(Target::Pipe(Box::new(DualWriter { file: log_file })));
                } else {
                    // 如果文件打开失败，只输出到 stderr
                    builder.target(Target::Stderr);
                }
            } else {
                // 没有指定文件路径，只输出到 stderr
                builder.target(Target::Stderr);
            }
        }
        
        builder.init();
    });
    
    Ok(())
}

/// 自动检测模式并初始化日志系统
/// GUI 模式也会输出日志到文件（与 MCP 模式使用相同路径）
pub fn auto_init_logger() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();
    let is_mcp_mode = args.len() >= 3 && args[1] == "--mcp-request";
    
    // 获取日志文件路径（GUI 和 MCP 模式统一使用配置目录）
    let log_file_path = env::var("MCP_LOG_FILE")
        .ok()
        .or_else(|| get_gui_log_path().map(|p| p.to_string_lossy().to_string()));
    
    let config = if is_mcp_mode {
        // MCP 模式：只输出到文件，不输出到 stderr
        LogConfig {
            level: env::var("RUST_LOG")
                .unwrap_or_else(|_| "warn".to_string())
                .parse::<LevelFilter>()
                .unwrap_or(LevelFilter::Warn),
            file_path: log_file_path,
            is_mcp_mode: true,
            rotation: LogRotationConfig::default(),
        }
    } else {
        // GUI 模式：同时输出到文件和 stderr
        LogConfig {
            level: env::var("RUST_LOG")
                .unwrap_or_else(|_| "info".to_string())
                .parse::<LevelFilter>()
                .unwrap_or(LevelFilter::Info),
            file_path: log_file_path,
            is_mcp_mode: false,
            rotation: LogRotationConfig::default(),
        }
    };
    
    init_logger(config)
}

/// 便利宏：只在重要情况下记录日志
#[macro_export]
macro_rules! log_important {
    (error, $($arg:tt)*) => {
        log::error!($($arg)*)
    };
    (warn, $($arg:tt)*) => {
        log::warn!($($arg)*)
    };
    (info, $($arg:tt)*) => {
        log::info!($($arg)*)
    };
}

/// 便利宏：调试日志（只在 debug 级别下输出）
#[macro_export]
macro_rules! log_debug {
    ($($arg:tt)*) => {
        log::debug!($($arg)*)
    };
}

/// 便利宏：跟踪日志（只在 trace 级别下输出）
#[macro_export]
macro_rules! log_trace {
    ($($arg:tt)*) => {
        log::trace!($($arg)*)
    };
}
