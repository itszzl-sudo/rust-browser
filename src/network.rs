//! 网络模块 - 基于 reqwest 实现
//!
//! 封装 HTTP 请求和响应处理，支持：
//! - 超时控制
//! - Cookie 支持
//! - Chrome User-Agent 模拟
//! - gzip/brotli 自动解压

use anyhow::{anyhow, Result};
use log::{debug, info, trace, warn};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;
use url::Url;

use lazy_static::lazy_static;

lazy_static! {
    /// 全局 Cookie 存储
    static ref COOKIE_JAR: Mutex<HashMap<String, CookieEntry>> = Mutex::new(HashMap::new());

    /// 全局 Referer 栈（用于导航历史）
    static ref REFERER_STACK: Mutex<Vec<String>> = Mutex::new(Vec::new());
}

/// 为当前线程创建一个 reqwest blocking client
fn create_blocking_client() -> reqwest::blocking::Client {
    reqwest::blocking::Client::builder()
        .user_agent(CHROME_USER_AGENT)
        .timeout(Duration::from_secs(30))
        .danger_accept_invalid_certs(false)
        .gzip(true)
        .brotli(true)
        .cookie_store(true)
        .build()
        .expect("Failed to create HTTP client")
}

const CHROME_USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36";

#[derive(Clone, Debug)]
struct CookieEntry {
    name: String,
    value: String,
    domain: String,
    path: String,
    secure: bool,
}

#[derive(Clone, Debug)]
pub struct HttpResponse {
    pub status: u16,
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
    pub final_url: String,
}

pub struct NetworkClient {
    timeout_secs: u64,
    enable_cookies: bool,
    custom_ua: Option<String>,
}

impl NetworkClient {
    pub fn new() -> Self {
        info!("初始化 NetworkClient (基于 reqwest)");
        Self {
            timeout_secs: 30,
            enable_cookies: true,
            custom_ua: Some(CHROME_USER_AGENT.to_string()),
        }
    }

    pub fn with_timeout(mut self, secs: u64) -> Self {
        self.timeout_secs = secs;
        self
    }

    pub fn with_user_agent(mut self, ua: String) -> Self {
        self.custom_ua = Some(ua);
        self
    }

    pub fn with_cookies_disabled(mut self) -> Self {
        self.enable_cookies = false;
        self
    }

    fn get_cookies(&self, url: &Url) -> String {
        if !self.enable_cookies {
            return String::new();
        }

        let host = url.host_str().unwrap_or("");
        let path = url.path();

        if let Ok(jar) = COOKIE_JAR.lock() {
            let mut cookies = Vec::new();
            for (_, entry) in jar.iter() {
                if entry.domain == host && path.starts_with(&entry.path) {
                    if !entry.secure || url.scheme() == "https" {
                        cookies.push(format!("{}={}", entry.name, entry.value));
                    }
                }
            }
            return cookies.join("; ");
        }
        String::new()
    }

    fn parse_set_cookie(&self, header: &str, url: &Url) {
        if !self.enable_cookies {
            return;
        }

        let parts: Vec<&str> = header.split(';').collect();
        if parts.is_empty() {
            return;
        }

        let name_value = parts[0];
        if let Some((name, value)) = name_value.split_once('=') {
            let domain = url.host_str().unwrap_or("").to_string();
            let path = url.path().to_string();
            let secure = header.to_lowercase().contains("secure");

            let entry = CookieEntry {
                name: name.trim().to_string(),
                value: value.trim().to_string(),
                domain,
                path,
                secure,
            };

            if let Ok(mut jar) = COOKIE_JAR.lock() {
                jar.insert(
                    format!("{}:{}", url.host_str().unwrap_or(""), name.trim()),
                    entry,
                );
            }
        }
    }

    fn update_referer(&self, url: &str) {
        if let Ok(mut stack) = REFERER_STACK.lock() {
            if stack.len() >= 10 {
                stack.remove(0);
            }
            stack.push(url.to_string());
        }
    }

    fn get_referer(&self) -> Option<String> {
        if let Ok(stack) = REFERER_STACK.lock() {
            return stack.last().cloned();
        }
        None
    }

    /// 异步获取整个 HTTP 响应
    pub async fn fetch(&self, url: &str) -> Result<HttpResponse> {
        debug!("发送 GET 请求: {} (超时: {}s)", url, self.timeout_secs);

        let parsed_url = Url::parse(url).map_err(|e| anyhow!("URL 解析失败: {}", e))?;

        let client = reqwest::Client::builder()
            .user_agent(
                self.custom_ua
                    .clone()
                    .unwrap_or_else(|| CHROME_USER_AGENT.to_string()),
            )
            .timeout(Duration::from_secs(self.timeout_secs))
            .gzip(true)
            .brotli(true)
            .build()
            .map_err(|e| anyhow!("创建 HTTP 客户端失败: {}", e))?;

        let mut req = client.get(url);

        let cookie_header = self.get_cookies(&parsed_url);
        if !cookie_header.is_empty() {
            req = req.header("Cookie", cookie_header);
        }

        if let Some(referer) = self.get_referer() {
            req = req.header("Referer", referer);
        }

        req = req
            .header(
                "Accept",
                "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8",
            )
            .header("Accept-Language", "zh-CN,zh;q=0.9,en;q=0.8")
            .header("Accept-Encoding", "gzip, deflate, br");

        let response = req
            .send()
            .await
            .map_err(|e| anyhow!("网络请求失败: {}", e))?;

        let status = response.status().as_u16();
        let headers: HashMap<String, String> = response
            .headers()
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or("").to_string()))
            .collect();

        let body = response
            .bytes()
            .await
            .map_err(|e| anyhow!("读取响应体失败: {}", e))?;

        if let Some(set_cookie) = headers.get("set-cookie") {
            self.parse_set_cookie(set_cookie, &parsed_url);
        }

        self.update_referer(url);

        Ok(HttpResponse {
            status,
            headers,
            body: body.to_vec(),
            final_url: url.to_string(),
        })
    }

    /// 异步获取 HTML 内容
    pub async fn fetch_html(&self, url: &str) -> Result<String> {
        let response = self.fetch(url).await?;
        if response.status != 200 && response.status != 304 {
            return Err(anyhow!("HTTP 状态码: {}", response.status));
        }
        let body = String::from_utf8_lossy(&response.body).into_owned();
        Ok(body)
    }

    /// 同步获取 HTML 内容（用于渲染器线程）
    pub fn fetch_html_blocking(&self, url: &str) -> Result<String> {
        debug!(
            "[blocking] 发送 GET 请求: {} (超时: {}s)",
            url, self.timeout_secs
        );

        let parsed_url = Url::parse(url).map_err(|e| anyhow!("URL 解析失败: {}", e))?;

        let client = create_blocking_client();
        let mut req = client.get(url);

        // 添加自定义 UA
        if let Some(ref ua) = self.custom_ua {
            req = req.header("User-Agent", ua);
        }

        // 添加 Cookie
        let cookie_header = self.get_cookies(&parsed_url);
        if !cookie_header.is_empty() {
            req = req.header("Cookie", cookie_header);
        }

        // 添加 Referer
        if let Some(referer) = self.get_referer() {
            req = req.header("Referer", referer);
        }

        // 添加标准浏览器请求头
        req = req
            .header(
                "Accept",
                "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8",
            )
            .header("Accept-Language", "zh-CN,zh;q=0.9,en;q=0.8")
            .header("Accept-Encoding", "gzip, deflate, br")
            .header("Connection", "keep-alive")
            .header("Upgrade-Insecure-Requests", "1");

        let response = req.send().map_err(|e| anyhow!("网络请求失败: {}", e))?;

        let status = response.status().as_u16();
        if status != 200 && status != 304 {
            return Err(anyhow!("HTTP 状态码: {}", status));
        }

        // 先读 cookie header（在 response 被消费之前）
        if self.enable_cookies {
            if let Some(set_cookie_headers) = response.headers().get("set-cookie") {
                if let Ok(cookie_str) = set_cookie_headers.to_str() {
                    self.parse_set_cookie(cookie_str, &parsed_url);
                }
            }
        }

        let body = response
            .bytes()
            .map_err(|e| anyhow!("读取响应体失败: {}", e))?;

        let body_str = String::from_utf8_lossy(&body).into_owned();

        self.update_referer(url);
        trace!("收到响应，状态码: {}", status);

        Ok(body_str)
    }

    /// 同步 HTTP GET 请求，返回完整响应
    pub fn get(&self, url: &str) -> Result<HttpResponse> {
        debug!("[GET] {} (timeout: {}s)", url, self.timeout_secs);
        
        let parsed_url = Url::parse(url).map_err(|e| anyhow!("URL 解析失败: {}", e))?;
        let client = create_blocking_client();
        let mut req = client.get(url);
        
        if let Some(ref ua) = self.custom_ua {
            req = req.header("User-Agent", ua);
        }
        
        let cookie_header = self.get_cookies(&parsed_url);
        if !cookie_header.is_empty() {
            req = req.header("Cookie", cookie_header);
        }
        
        if let Some(referer) = self.get_referer() {
            req = req.header("Referer", referer);
        }
        
        req = req
            .header("Accept", "*/*")
            .header("Accept-Language", "zh-CN,zh;q=0.9,en;q=0.8")
            .header("Accept-Encoding", "gzip, deflate, br");
        
        let response = req.send().map_err(|e| anyhow!("网络请求失败: {}", e))?;
        
        let status = response.status().as_u16();
        let final_url = response.url().to_string();
        let headers: HashMap<String, String> = response
            .headers()
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or("").to_string()))
            .collect();
        
        if let Some(set_cookie) = headers.get("set-cookie") {
            self.parse_set_cookie(set_cookie, &parsed_url);
        }
        
        let body = response.bytes().map_err(|e| anyhow!("读取响应体失败: {}", e))?;
        
        self.update_referer(url);
        
        Ok(HttpResponse {
            status,
            headers,
            body: body.to_vec(),
            final_url,
        })
    }

    /// 同步 HTTP POST 请求
    pub fn post(&self, url: &str, body: &[u8], content_type: &str) -> Result<HttpResponse> {
        debug!("[POST] {} (timeout: {}s)", url, self.timeout_secs);
        
        let parsed_url = Url::parse(url).map_err(|e| anyhow!("URL 解析失败: {}", e))?;
        let client = create_blocking_client();
        let mut req = client.post(url).body(body.to_vec());
        
        if let Some(ref ua) = self.custom_ua {
            req = req.header("User-Agent", ua);
        }
        
        req = req.header("Content-Type", content_type);
        
        let cookie_header = self.get_cookies(&parsed_url);
        if !cookie_header.is_empty() {
            req = req.header("Cookie", cookie_header);
        }
        
        if let Some(referer) = self.get_referer() {
            req = req.header("Referer", referer);
        }
        
        let response = req.send().map_err(|e| anyhow!("网络请求失败: {}", e))?;
        
        let status = response.status().as_u16();
        let final_url = response.url().to_string();
        let headers: HashMap<String, String> = response
            .headers()
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or("").to_string()))
            .collect();
        
        let response_body = response.bytes().map_err(|e| anyhow!("读取响应体失败: {}", e))?;
        
        self.update_referer(url);
        
        Ok(HttpResponse {
            status,
            headers,
            body: response_body.to_vec(),
            final_url,
        })
    }

    pub fn clear_cookies(&self) {
        if let Ok(mut jar) = COOKIE_JAR.lock() {
            jar.clear();
            info!("Cookie 存储已清空");
        }
    }

    pub fn clear_referer(&self) {
        if let Ok(mut stack) = REFERER_STACK.lock() {
            stack.clear();
            info!("Referer 栈已清空");
        }
    }
}

impl Clone for NetworkClient {
    fn clone(&self) -> Self {
        Self {
            timeout_secs: self.timeout_secs,
            enable_cookies: self.enable_cookies,
            custom_ua: self.custom_ua.clone(),
        }
    }
}

impl Default for NetworkClient {
    fn default() -> Self {
        Self::new()
    }
}
