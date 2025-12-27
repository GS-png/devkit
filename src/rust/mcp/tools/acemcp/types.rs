use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};
use std::collections::HashMap;

/// Acemcp搜索请求参数
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcemcpRequest {
    /// 项目根目录的绝对路径
    pub project_root_path: String,
    /// 用于查找相关代码上下文的自然语言搜索查询
    pub query: String,
}

/// Acemcp配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcemcpConfig {
    /// API端点URL
    pub base_url: Option<String>,
    /// 认证令牌
    pub token: Option<String>,
    /// 每批上传的文件数量
    pub batch_size: Option<u32>,
    /// 大文件分割前的最大行数
    pub max_lines_per_blob: Option<u32>,
    /// 要索引的文件扩展名列表
    pub text_extensions: Option<Vec<String>>,
    /// 要排除的模式列表
    pub exclude_patterns: Option<Vec<String>>,
    /// 搜索时的智能等待配置（秒）
    /// 当检测到索引正在进行时，随机等待 [min, max] 秒后再执行搜索
    /// 默认值：Some((1, 5))，设为 None 则禁用智能等待
    pub smart_wait_range: Option<(u64, u64)>,
    // 代理配置
    /// 是否启用代理
    pub proxy_enabled: Option<bool>,
    /// 代理主机地址
    pub proxy_host: Option<String>,
    /// 代理端口
    pub proxy_port: Option<u16>,
    /// 代理类型: "http" | "socks5"
    pub proxy_type: Option<String>,
}


/// 索引状态枚举
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum IndexStatus {
    /// 空闲状态（未开始索引）
    Idle,
    /// 正在索引中
    Indexing,
    /// 索引成功完成
    Synced,
    /// 索引失败
    Failed,
}

/// 项目索引状态信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectIndexStatus {
    /// 项目根路径（规范化后）
    pub project_root: String,
    /// 当前索引状态
    pub status: IndexStatus,
    /// 索引进度百分比（0-100）
    pub progress: u8,
    /// 总文件数
    pub total_files: usize,
    /// 已索引文件数
    pub indexed_files: usize,
    /// 待处理文件数
    pub pending_files: usize,
    /// 失败文件数
    pub failed_files: usize,
    /// 最后成功索引时间
    pub last_success_time: Option<DateTime<Utc>>,
    /// 最后失败时间
    pub last_failure_time: Option<DateTime<Utc>>,
    /// 最后错误信息
    pub last_error: Option<String>,
    /// 按目录聚合的统计信息（目录路径 -> (已索引, 待处理)）
    pub directory_stats: HashMap<String, (usize, usize)>,
}

impl Default for ProjectIndexStatus {
    fn default() -> Self {
        Self {
            project_root: String::new(),
            status: IndexStatus::Idle,
            progress: 0,
            total_files: 0,
            indexed_files: 0,
            pending_files: 0,
            failed_files: 0,
            last_success_time: None,
            last_failure_time: None,
            last_error: None,
            directory_stats: HashMap::new(),
        }
    }
}

/// 所有项目的索引状态集合
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProjectsIndexStatus {
    /// 项目路径 -> 索引状态
    pub projects: HashMap<String, ProjectIndexStatus>,
}

/// 单个文件的索引状态
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum FileIndexStatusKind {
    /// 文件已完成索引（所有 blob 均已上传并记录）
    Indexed,
    /// 文件已被纳入候选集合但尚未全部完成索引或需要重新上传
    Pending,
}

/// 文件索引状态信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileIndexStatus {
    /// 相对于项目根目录的文件路径，使用正斜杠(/)分隔
    pub path: String,
    /// 文件索引状态
    pub status: FileIndexStatusKind,
}

/// 项目内所有可索引文件的状态集合（用于前端构建项目结构树）
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProjectFilesStatus {
    /// 项目根路径（规范化后）
    pub project_root: String,
    /// 文件状态列表
    pub files: Vec<FileIndexStatus>,
}

// ============ 代理测速相关类型 ============

/// 检测到的代理信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectedProxy {
    /// 代理主机
    pub host: String,
    /// 代理端口
    pub port: u16,
    /// 代理类型: "http" | "socks5"
    pub proxy_type: String,
    /// 响应时间（毫秒），用于排序
    pub response_time_ms: Option<u64>,
}

/// 代理测速结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxySpeedTestResult {
    /// 测试模式: "proxy" | "direct" | "compare"
    pub mode: String,
    /// 代理配置信息（仅代理模式有效）
    pub proxy_info: Option<DetectedProxy>,
    /// 测试指标列表
    pub metrics: Vec<SpeedTestMetric>,
    /// 测试时间戳
    pub timestamp: String,
    /// 总体推荐建议
    pub recommendation: String,
    /// 是否全部测试成功
    pub success: bool,
}

/// 单项测试指标
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpeedTestMetric {
    /// 指标名称（如 "网络延迟"、"单文件上传"、"语义搜索"）
    pub name: String,
    /// 指标类型: "ping" | "upload_single" | "search"
    pub metric_type: String,
    /// 代理模式耗时（毫秒）
    pub proxy_time_ms: Option<u64>,
    /// 直连模式耗时（毫秒）
    pub direct_time_ms: Option<u64>,
    /// 是否成功
    pub success: bool,
    /// 错误信息
    pub error: Option<String>,
}
