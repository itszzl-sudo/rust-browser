//! 网络模块 - 使用 obscura-net
//!
//! 封装 HTTP 请求和响应处理
use anyhow::{anyhow, Result};
use log::{debug, info, trace};
use obscura_net::{ObscuraHttpClient, Response};
use url::Url;

/// 网络客户端
pub struct NetworkClient {
    /// Obscura HTTP 客户端
    client: ObscuraHttpClient,
}

impl NetworkClient {
    /// 创建新的网络客户端
    pub fn new() -> Self {
        info!("初始化 NetworkClient (基于 obscura-net)");
        Self {
            client: ObscuraHttpClient::new(),
        }
    }

    /// 发送 GET 请求
    pub async fn fetch(&self, url: &str) -> Result<Response> {
        debug!("发送 GET 请求: {}", url);
        
        let url = Url::parse(url)
            .map_err(|e| anyhow!("URL 解析失败: {}", e))?;
        
        let response = self.client
            .fetch(&url)
            .await
            .map_err(|e| anyhow!("网络请求失败: {}", e))?;
        
        trace!("收到响应，状态码: {}", response.status);
        
        Ok(response)
    }

    /// 发送请求并获取 HTML 文本
    pub async fn fetch_html(&self, url: &str) -> Result<String> {
        let response = self.fetch(url).await?;
        
        if response.status != 200 && response.status != 304 {
            return Err(anyhow!("HTTP 状态码: {}", response.status));
        }
        
        let body = String::from_utf8_lossy(&response.body).into_owned();
        Ok(body)
    }
}

impl Default for NetworkClient {
    fn default() -> Self {
        Self::new()
    }
}
