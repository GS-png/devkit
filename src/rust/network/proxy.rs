// 代理检测和配置模块
use serde::{Deserialize, Serialize};

/// 代理类型
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ProxyType {
    Http,
    Socks5,
}

impl Default for ProxyType {
    fn default() -> Self {
        ProxyType::Http
    }
}

impl std::fmt::Display for ProxyType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProxyType::Http => write!(f, "http"),
            ProxyType::Socks5 => write!(f, "socks5"),
        }
    }
}

/// 代理信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyInfo {
    pub proxy_type: ProxyType,
    pub host: String,
    pub port: u16,
}

impl ProxyInfo {
    /// 创建新的代理信息
    pub fn new(proxy_type: ProxyType, host: String, port: u16) -> Self {
        Self {
            proxy_type,
            host,
            port,
        }
    }
    
    /// 获取代理URL
    pub fn to_url(&self) -> String {
        format!("{}://{}:{}", self.proxy_type, self.host, self.port)
    }
}

/// 代理检测器
pub struct ProxyDetector;

impl ProxyDetector {
    /// 常用代理端口列表（按优先级排序）
    /// 
    /// - 7890: Clash 混合代理端口（HTTP + SOCKS5）
    /// - 7891: Clash HTTP 代理端口
    /// - 10809: V2Ray SOCKS5 代理端口
    /// - 10808: V2Ray HTTP 代理端口
    /// - 1080: 通用 SOCKS5 代理端口
    /// - 8080: 通用 HTTP 代理端口
    const COMMON_PORTS: &'static [(u16, ProxyType)] = &[
        (7890, ProxyType::Http),    // Clash 混合端口（优先尝试HTTP）
        (7891, ProxyType::Http),    // Clash HTTP端口
        (10808, ProxyType::Http),   // V2Ray HTTP端口
        (10809, ProxyType::Socks5), // V2Ray SOCKS5端口
        (1080, ProxyType::Socks5),  // 通用SOCKS5端口
        (8080, ProxyType::Http),    // 通用HTTP端口
    ];
    
    /// 检测本地可用的代理
    /// 
    /// 按优先级顺序检测常用代理端口，返回第一个可用的代理
    /// 
    /// # 返回值
    /// - `Some(ProxyInfo)`: 找到可用的代理
    /// - `None`: 没有找到可用的代理
    pub async fn detect_available_proxy() -> Option<ProxyInfo> {
        log::info!("🔍 开始检测本地代理");
        
        for (port, proxy_type) in Self::COMMON_PORTS {
            let proxy_info = ProxyInfo::new(proxy_type.clone(), "127.0.0.1".to_string(), *port);
            
            log::info!("🔍 检测代理端口: {} ({})", port, proxy_type);
            
            if Self::check_proxy(&proxy_info).await {
                log::info!("✅ 找到可用代理: {}:{} ({})", proxy_info.host, proxy_info.port, proxy_info.proxy_type);
                return Some(proxy_info);
            }
        }
        
        log::warn!("⚠️ 未找到可用的本地代理");
        None
    }
    
    /// 检测指定代理是否可用
    /// 
    /// 通过代理发送测试请求到 Google 的 generate_204 端点
    /// 该端点专门用于网络连接测试，返回 HTTP 204 状态码
    /// 
    /// # 参数
    /// - `proxy_info`: 要检测的代理信息
    /// 
    /// # 返回值
    /// - `true`: 代理可用
    /// - `false`: 代理不可用
    pub async fn check_proxy(proxy_info: &ProxyInfo) -> bool {
        // 创建代理URL
        let proxy_url = proxy_info.to_url();
        
        // 尝试创建带代理的HTTP客户端
        let client_builder = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(3));
        
        // 统一使用 Proxy::all() 让所有请求都走代理
        // 这样无论测试 http 还是 https 端点都能正确走代理
        let client = match proxy_info.proxy_type {
            ProxyType::Http => {
                // HTTP 代理：使用 all() 同时代理 http 和 https 请求
                match reqwest::Proxy::all(&proxy_url) {
                    Ok(proxy) => client_builder.proxy(proxy),
                    Err(e) => {
                        log::debug!("❌ 创建HTTP代理失败: {}", e);
                        return false;
                    }
                }
            }
            ProxyType::Socks5 => {
                // SOCKS5 代理：同时用于 HTTP 和 HTTPS
                match reqwest::Proxy::all(&proxy_url) {
                    Ok(proxy) => client_builder.proxy(proxy),
                    Err(e) => {
                        log::debug!("❌ 创建SOCKS5代理失败: {}", e);
                        return false;
                    }
                }
            }
        };
        
        let client = match client.build() {
            Ok(c) => c,
            Err(e) => {
                log::debug!("❌ 构建HTTP客户端失败: {}", e);
                return false;
            }
        };
        
        // 发送测试请求
        // 使用 Google 的 generate_204 端点进行连接测试
        match client
            .get("http://www.gstatic.com/generate_204")
            .send()
            .await
        {
            Ok(response) => {
                let is_success = response.status().is_success() || response.status() == 204;
                if is_success {
                    log::debug!("✅ 代理 {}:{} 可用", proxy_info.host, proxy_info.port);
                } else {
                    log::debug!("❌ 代理 {}:{} 响应异常: HTTP {}", 
                        proxy_info.host, proxy_info.port, response.status());
                }
                is_success
            }
            Err(e) => {
                log::debug!("❌ 代理 {}:{} 连接失败: {}", 
                    proxy_info.host, proxy_info.port, e);
                false
            }
        }
    }
    
    /// 检测指定端口的代理是否可用
    /// 
    /// 便捷方法，用于检测单个端口
    pub async fn check_port(port: u16, proxy_type: ProxyType) -> bool {
        let proxy_info = ProxyInfo::new(proxy_type, "127.0.0.1".to_string(), port);
        Self::check_proxy(&proxy_info).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_detect_available_proxy() {
        let proxy = ProxyDetector::detect_available_proxy().await;
        match proxy {
            Some(info) => {
                println!("找到可用代理: {}:{} ({})", info.host, info.port, info.proxy_type);
            }
            None => {
                println!("未找到可用代理");
            }
        }
    }
    
    #[tokio::test]
    async fn test_check_specific_port() {
        // 测试 Clash 默认端口
        let is_available = ProxyDetector::check_port(7890, ProxyType::Http).await;
        println!("端口 7890 (HTTP) 可用: {}", is_available);
    }
}

