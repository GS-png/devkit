// HTTP客户端构建器模块
use super::proxy::ProxyInfo;

/// 创建HTTP客户端
/// 
/// 根据是否提供代理信息，创建带代理或不带代理的HTTP客户端
/// 
/// # 参数
/// - `proxy_info`: 可选的代理信息
/// - `timeout_secs`: 超时时间（秒）
/// 
/// # 返回值
/// - `Ok(reqwest::Client)`: 成功创建的HTTP客户端
/// - `Err(String)`: 创建失败的错误信息
pub fn create_http_client(
    proxy_info: Option<&ProxyInfo>,
    timeout_secs: u64,
) -> Result<reqwest::Client, String> {
    let mut client_builder = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(timeout_secs));
    
    // 如果提供了代理信息，配置代理
    if let Some(proxy) = proxy_info {
        log::info!("🔧 配置HTTP客户端使用代理: {}:{} ({})", 
            proxy.host, proxy.port, proxy.proxy_type);
        
        let proxy_url = proxy.to_url();
        
        // 使用 Proxy::all() 让所有请求（http 和 https）都走代理
        // 这对于 Clash 等混合代理端口尤其重要，因为它们同时支持 HTTP 和 HTTPS 代理
        let reqwest_proxy = match proxy.proxy_type {
            super::proxy::ProxyType::Http => {
                // HTTP 代理：使用 all() 同时代理 http 和 https 请求
                // 注意：reqwest::Proxy::http() 只代理 http:// 请求，不代理 https://
                // 而 GitHub API 等都是 https，所以必须用 all() 或同时配置 http + https
                reqwest::Proxy::all(&proxy_url)
                    .map_err(|e| format!("创建HTTP代理失败: {}", e))?
            }
            super::proxy::ProxyType::Socks5 => {
                // SOCKS5 代理：同时用于 HTTP 和 HTTPS
                reqwest::Proxy::all(&proxy_url)
                    .map_err(|e| format!("创建SOCKS5代理失败: {}", e))?
            }
        };
        
        client_builder = client_builder.proxy(reqwest_proxy);
    } else {
        log::info!("🔧 配置HTTP客户端使用直连");
    }
    
    // 构建客户端
    client_builder
        .build()
        .map_err(|e| format!("构建HTTP客户端失败: {}", e))
}

/// 创建用于更新检查的HTTP客户端
/// 
/// 便捷方法，使用默认的30秒超时
pub fn create_update_client(proxy_info: Option<&ProxyInfo>) -> Result<reqwest::Client, String> {
    create_http_client(proxy_info, 30)
}

/// 创建用于下载的HTTP客户端
/// 
/// 便捷方法，使用较长的60秒超时（适合大文件下载）
pub fn create_download_client(proxy_info: Option<&ProxyInfo>) -> Result<reqwest::Client, String> {
    create_http_client(proxy_info, 60)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::network::proxy::{ProxyInfo, ProxyType};

    #[test]
    fn test_create_client_without_proxy() {
        let client = create_http_client(None, 10);
        assert!(client.is_ok());
    }
    
    #[test]
    fn test_create_client_with_http_proxy() {
        let proxy = ProxyInfo::new(ProxyType::Http, "127.0.0.1".to_string(), 7890);
        let client = create_http_client(Some(&proxy), 10);
        assert!(client.is_ok());
    }
    
    #[test]
    fn test_create_client_with_socks5_proxy() {
        let proxy = ProxyInfo::new(ProxyType::Socks5, "127.0.0.1".to_string(), 1080);
        let client = create_http_client(Some(&proxy), 10);
        assert!(client.is_ok());
    }
}

