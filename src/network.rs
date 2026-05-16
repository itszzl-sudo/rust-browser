//! 网络模块 - 基于 obscura-net 增强版
//!
//! 封装 HTTP 请求和响应处理，支持：
//! - 超时控制
//! - Cookie 支持
//! - Chrome User-Agent 模拟
//! - gzip/brotli 自动解压
//! - Referer/Origin 头

use anyhow::{anyhow, Result};
use brotli::Decompressor as BrotliDecompressor;
use flate2::read::GzDecoder;
use lazy_static::lazy_static;
use log::{debug, info, trace, warn};
use obscura_net::ObscuraHttpClient;
use std::collections::HashMap;
use std::io::Read;
use std::sync::Mutex;
use std::time::Duration;
use url::Url;

lazy_static! {
    /// 全局 Cookie 存储
    static ref COOKIE_JAR: Mutex<HashMap<String, CookieEntry>> = Mutex::new(HashMap::new());

    /// 全局 Referer 栈（用于导航历史）
    static ref REFERER_STACK: Mutex<Vec<String>> = Mutex::new(Vec::new());
}

const DEFAULT_TIMEOUT_SECS: u64 = 30;

const CHROME_USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36";

#[derive(Clone, Debug)]
struct CookieEntry {
    name: String,
    value: String,
    domain: String,
    path: String,
    expires: Option<i64>,
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
    client: ObscuraHttpClient,
    timeout_secs: u64,
    enable_cookies: bool,
    custom_ua: Option<String>,
}

impl NetworkClient {
    pub fn new() -> Self {
        info!("初始化增强版 NetworkClient");
        Self {
            client: ObscuraHttpClient::new(),
            timeout_secs: DEFAULT_TIMEOUT_SECS,
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
                expires: None,
                secure,
            };

            if let Ok(mut jar) = COOKIE_JAR.lock() {
                jar.insert(format!("{}:{}", url.host_str().unwrap_or(""), name.trim()), entry);
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

    fn parse_response(&self, response: obscura_net::Response, url: &str) -> HttpResponse {
        let mut headers = HashMap::new();
        for (key, value) in &response.headers {
            headers.insert(key.clone(), value.clone());
        }

        if let Some(set_cookie) = headers.get("set-cookie") {
            if let Ok(parsed_url) = Url::parse(url) {
                self.parse_set_cookie(set_cookie, &parsed_url);
            }
        }

        HttpResponse {
            status: response.status,
            headers,
            body: response.body,
            final_url: url.to_string(),
        }
    }

    fn decompress_body(&self, response: &mut HttpResponse) -> Result<()> {
        if let Some(encoding) = response.headers.get("content-encoding").cloned() {
            let encoding_lower = encoding.to_lowercase();

            match encoding_lower.as_str() {
                "gzip" => {
                    trace!("解压 gzip 响应体");
                    let mut decoder = GzDecoder::new(&response.body[..]);
                    let mut decompressed = Vec::new();
                    decoder.read_to_end(&mut decompressed)
                        .map_err(|e| anyhow!("gzip 解压失败: {}", e))?;
                    response.body = decompressed;
                    response.headers.remove("content-encoding");
                }
                "br" | "brotli" => {
                    trace!("解压 brotli 响应体");
                    let mut decompressed = Vec::new();
                    let mut decoder = BrotliDecompressor::new(&response.body[..], 4096);
                    decoder.read_to_end(&mut decompressed)
                        .map_err(|e| anyhow!("brotli 解压失败: {}", e))?;
                    response.body = decompressed;
                    response.headers.remove("content-encoding");
                }
                "deflate" => {
                    trace!("解压 deflate 响应体");
                    use flate2::Decompress;
                    let mut decoder = Decompress::new(true);
                    let mut decompressed = Vec::new();
                    let result = decoder.decompress(&response.body, &mut decompressed, flate2::FlushDecompress::Finish);
                    if result.is_ok() {
                        response.body = decompressed;
                    }
                    response.headers.remove("content-encoding");
                }
                _ => {
                    warn!("未知的 Content-Encoding: {}", encoding);
                }
            }
        }
        Ok(())
    }

    pub async fn fetch(&self, url: &str) -> Result<HttpResponse> {
        debug!("发送 GET 请求: {} (超时: {}s)", url, self.timeout_secs);

        let parsed_url = Url::parse(url)
            .map_err(|e| anyhow!("URL 解析失败: {}", e))?;

        let cookie_header = self.get_cookies(&parsed_url);
        if !cookie_header.is_empty() {
            trace!("发送 Cookie: {}", cookie_header);
        }

        let response = self.client
            .fetch(&parsed_url)
            .await
            .map_err(|e| anyhow!("网络请求失败: {}", e))?;

        let mut http_response = self.parse_response(response, url);

        self.decompress_body(&mut http_response)?;

        self.update_referer(url);

        trace!("收到响应，状态码: {}", http_response.status);

        Ok(http_response)
    }

    pub async fn fetch_with_timeout(&self, url: &str, timeout_secs: u64) -> Result<HttpResponse> {
        let client = self.clone();
        let url = url.to_string();

        tokio::time::timeout(Duration::from_secs(timeout_secs), async move {
            client.fetch(&url).await
        })
        .await
        .map_err(|_| anyhow!("请求超时 ({}秒)", timeout_secs))?
    }

    pub async fn fetch_html(&self, url: &str) -> Result<String> {
        let response = self.fetch(url).await?;

        if response.status != 200 && response.status != 304 {
            return Err(anyhow!("HTTP 状态码: {}", response.status));
        }

        let body = String::from_utf8_lossy(&response.body).into_owned();
        Ok(body)
    }

    pub async fn fetch_html_with_timeout(&self, url: &str, timeout_secs: u64) -> Result<String> {
        let response = self.fetch_with_timeout(url, timeout_secs).await?;

        if response.status != 200 && response.status != 304 {
            return Err(anyhow!("HTTP 状态码: {}", response.status));
        }

        let body = String::from_utf8_lossy(&response.body).into_owned();
        Ok(body)
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
            client: ObscuraHttpClient::new(),
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
