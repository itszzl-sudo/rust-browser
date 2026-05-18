//! 网络模块 — 基于 reqwest 实现
//!
//! 封装 HTTP 请求和响应处理，支持：
//! - 超时控制
//! - Cookie 支持（符合 RFC 6265）
//! - Chrome User-Agent 模拟
//! - gzip/brotli 自动解压
//! - 导航 / 资源请求区分（Referer 更新策略）
//! - 实例级 Cookie jar（支持独立/共享）

use log::{debug, info, trace};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use url::Url;

use lazy_static::lazy_static;

lazy_static! {
    /// 全局默认 Cookie 存储（Arc 共享）
    static ref GLOBAL_COOKIE_JAR: SharedCookieJar =
        std::sync::Arc::new(Mutex::new(HashMap::new()));

    /// 全局 Referer 栈（用于导航历史）
    static ref REFERER_STACK: Mutex<Vec<String>> = Mutex::new(Vec::new());
}

// ═════════════════════════════════════════════════════════════════
// User-Agent
// ═════════════════════════════════════════════════════════════════

const CHROME_USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
     (KHTML, like Gecko) Chrome/130.0.0.0 Safari/537.36";

// ═════════════════════════════════════════════════════════════════
// 自定义错误类型
// ═════════════════════════════════════════════════════════════════

#[derive(Debug, thiserror::Error)]
pub enum NetworkError {
    #[error("URL 解析失败: {0}")]
    InvalidUrl(String),

    #[error("网络请求失败: {0}")]
    RequestFailed(String),

    #[error("读取响应体失败: {0}")]
    ReadBodyFailed(String),

    #[error("HTTP 状态码错误: {0}")]
    HttpStatus(u16),

    #[error("创建 HTTP 客户端失败: {0}")]
    ClientCreationFailed(String),
}

// ═════════════════════════════════════════════════════════════════
// Cookie 存储
// ═════════════════════════════════════════════════════════════════

/// Cookie 条目，符合 RFC 6265 规范
#[derive(Clone, Debug)]
struct CookieEntry {
    name: String,
    value: String,
    domain: String,
    path: String,
    secure: bool,
    http_only: bool,
    expires: Option<Instant>,
}

impl CookieEntry {
    /// 检查 cookie 是否已过期
    fn is_expired(&self) -> bool {
        self.expires.map_or(false, |exp| Instant::now() > exp)
    }
}

/// 共享 Cookie jar 类型
type SharedCookieJar = std::sync::Arc<Mutex<HashMap<String, CookieEntry>>>;

/// 创建新的共享 Cookie jar
fn new_shared_jar() -> SharedCookieJar {
    std::sync::Arc::new(Mutex::new(HashMap::new()))
}

// ═════════════════════════════════════════════════════════════════
// 响应结构体
// ═════════════════════════════════════════════════════════════════

#[derive(Clone, Debug)]
pub struct HttpResponse {
    pub status: u16,
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
    pub final_url: String,
}

// ═════════════════════════════════════════════════════════════════
// NetworkClient
// ═════════════════════════════════════════════════════════════════

pub struct NetworkClient {
    timeout_secs: u64,
    enable_cookies: bool,
    custom_ua: Option<String>,
    cookie_jar: SharedCookieJar,
}

impl NetworkClient {
    /// 创建一个新的 NetworkClient，共享全局 Cookie jar。
    pub fn new() -> Self {
        info!("初始化 NetworkClient (基于 reqwest)");
        Self {
            timeout_secs: 30,
            enable_cookies: true,
            custom_ua: Some(CHROME_USER_AGENT.to_string()),
            cookie_jar: GLOBAL_COOKIE_JAR.clone(),
        }
    }

    /// 设置请求超时秒数
    pub fn with_timeout(mut self, secs: u64) -> Self {
        self.timeout_secs = secs;
        self
    }

    /// 设置自定义 User-Agent
    pub fn with_user_agent(mut self, ua: String) -> Self {
        self.custom_ua = Some(ua);
        self
    }

    /// 禁用 Cookie
    pub fn with_cookies_disabled(mut self) -> Self {
        self.enable_cookies = false;
        self
    }

    /// 使用独立（不共享全局）的 Cookie jar。
    /// 调用此方法后，当前实例拥有自己的 Cookie 存储，
    /// 不会与其他实例共享 Cookie。
    pub fn with_own_cookie_jar(mut self) -> Self {
        self.cookie_jar = new_shared_jar();
        self
    }

    // ── Cookie 方法 ──

    /// 清除该实例所使用的 Cookie jar 中的所有 Cookie
    pub fn clear_cookies(&self) {
        if let Ok(mut jar) = self.cookie_jar.lock() {
            jar.clear();
            info!("Cookie 存储已清空");
        }
    }

    /// 清除全局 Referer 栈
    pub fn clear_referer(&self) {
        if let Ok(mut stack) = REFERER_STACK.lock() {
            stack.clear();
            info!("Referer 栈已清空");
        }
    }

    // ── 请求头常量（统一） ──

    fn default_headers() -> Vec<(&'static str, &'static str)> {
        vec![
            (
                "Accept",
                "text/html,application/xhtml+xml,application/xml;q=0.9,\
                 image/avif,image/webp,image/apng,*/*;q=0.8",
            ),
            ("Accept-Language", "zh-CN,zh;q=0.9,en;q=0.8"),
            ("Accept-Encoding", "gzip, deflate, br"),
            ("Connection", "keep-alive"),
            ("Upgrade-Insecure-Requests", "1"),
            ("Sec-Fetch-Dest", "document"),
            ("Sec-Fetch-Mode", "navigate"),
            ("Sec-Fetch-Site", "none"),
            ("Sec-Fetch-User", "?1"),
        ]
    }

    // ── Cookie 工具 ──

    /// 从 Cookie jar 中获取与给定 URL 匹配的 Cookie 字符串
    fn get_cookies(&self, url: &Url) -> String {
        if !self.enable_cookies {
            return String::new();
        }

        let host = url.host_str().unwrap_or("");
        let path = url.path();

        if let Ok(mut jar) = self.cookie_jar.lock() {
            // 先清理过期 cookie
            jar.retain(|_, entry| !entry.is_expired());

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

    /// 解析 `Set-Cookie` 响应头，符合 RFC 6265
    fn parse_set_cookie(&self, header: &str, request_url: &Url) {
        if !self.enable_cookies {
            return;
        }

        // RFC 6265 § 5.2: Set-Cookie 的分隔符是 ";"
        let parts: Vec<&str> = header.split(';').collect();
        if parts.is_empty() {
            return;
        }

        // ---- 解析 name=value（值可能包含 "=" 号） ----
        let name_value_str = parts[0];
        let eq_pos = name_value_str.find('=');
        let (name, value) = match eq_pos {
            Some(pos) => (
                name_value_str[..pos].trim().to_string(),
                name_value_str[pos + 1..].trim().to_string(),
            ),
            None => return, // 没有 "=" 不合法
        };

        // 默认值
        let request_host = request_url.host_str().unwrap_or("").to_string();
        let mut domain = request_host.clone();
        let mut path = request_url.path().to_string();
        let mut secure = false;
        let mut http_only = false;
        let mut expires: Option<Instant> = None;

        // 解析各个属性（RFC 6265 § 5.2）
        for attr in &parts[1..] {
            let attr = attr.trim();
            if attr.is_empty() {
                continue;
            }

            let attr_lower = attr.to_lowercase();
            if attr_lower == "secure" {
                secure = true;
            } else if attr_lower == "httponly" {
                http_only = true;
            } else if let Some(val) = attr_lower.strip_prefix("domain=") {
                let val = val.trim();
                // RFC 6265 § 5.2.3: 后置点号去除
                let val = val.strip_prefix('.').unwrap_or(val);
                // 检查 domain 是否匹配请求 host 尾部
                if request_host.ends_with(&format!(".{}", val)) || request_host == val {
                    domain = val.to_string();
                }
                // 不匹配则忽略 Domain 属性
            } else if let Some(val) = attr_lower.strip_prefix("path=") {
                let val = val.trim();
                if val.starts_with('/') || val.starts_with("//") {
                    path = val.to_string();
                } else {
                    // RFC 6265 § 5.2.4: 不以 '/' 开头使用 default-path
                    path = Self::default_path(request_url.path());
                }
            } else if let Some(val) = attr_lower.strip_prefix("max-age=") {
                // Max-Age 优先于 Expires
                if let Ok(seconds) = val.trim().parse::<i64>() {
                    if seconds <= 0 {
                        // 立即过期（<=0 时删除 cookie）
                        if let Ok(mut jar) = self.cookie_jar.lock() {
                            let key = format!("{}:{}:{}", domain, name, path);
                            jar.remove(&key);
                        }
                        return;
                    }
                    expires = Some(Instant::now() + Duration::from_secs(seconds as u64));
                }
            } else if let Some(val) = attr.strip_prefix("Expires=") {
                // 如果已设置 Max-Age 则跳过
                if expires.is_none() {
                    if let Some(exp) = Self::parse_expires(val.trim()) {
                        expires = Some(exp);
                    }
                }
            }
        }

        // 修正 path：如果未设置 path 属性，使用 default-path
        if !parts
            .iter()
            .any(|p| p.trim().to_lowercase().starts_with("path="))
        {
            path = Self::default_path(request_url.path());
        }

        let entry = CookieEntry {
            name,
            value,
            domain,
            path,
            secure,
            http_only,
            expires,
        };

        if let Ok(mut jar) = self.cookie_jar.lock() {
            let key = format!("{}:{}:{}", entry.domain, entry.name, entry.path);
            jar.insert(key, entry);
        }
    }

    /// RFC 6265 § 5.1.4: default-path 算法
    fn default_path(request_path: &str) -> String {
        if !request_path.starts_with('/') {
            return "/".to_string();
        }
        // 如果路径只有一个 "/"，返回 "/"
        if request_path == "/" {
            return "/".to_string();
        }
        // 找到最后一个 "/" 并取其之前的部分
        if let Some(last_slash) = request_path[..request_path.len() - 1].rfind('/') {
            request_path[..=last_slash].to_string()
        } else {
            "/".to_string()
        }
    }

    /// 解析 HTTP Date（Expires 属性），仅支持常见格式
    fn parse_expires(val: &str) -> Option<Instant> {
        // 尝试解析几种常见的 HTTP 日期格式
        // 简单处理：使用系统时间解析
        let val = val.trim();
        // RFC 1123: "EEE, dd MMM yyyy HH:mm:ss GMT"
        // RFC 850: "EEEE, dd-MMM-yy HH:mm:ss GMT"
        // ANSI C: "EEE MMM dd HH:mm:ss yyyy"
        // 由于标准库没有直接的 HTTP date 解析，使用 httpdate 或自行解析
        // 这里用简易方式：尝试用 chrono 解析，但为避免新增依赖，使用自定义简化版。
        // 实际项目中建议使用 `httpdate` crate（已间接通过 reqwest 依赖）。
        // 这里利用 reqwest 内部的 httpdate（通过 http 包）或手动解析。
        // 简易：使用 `humantime` 或手动实现。此处直接保存原始字符串并在获取时比较。
        // 简化实现：只支持常见的 RFC 1123 格式
        Self::parse_http_date(val)
    }

    /// 简易 HTTP 日期解析（RFC 1123 / RFC 850 / ANSI C）
    fn parse_http_date(val: &str) -> Option<Instant> {
        // 当前已知的有效 HTTP 日期格式：
        // "Thu, 01 Dec 2050 16:00:00 GMT" — RFC 1123
        // 简易实现：仅支持已知格式，否则忽略过期时间
        // 使用 `httpdate` 更好，但为避免新增依赖，用正则简易匹配
        let months = [
            "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
        ];

        let val = val.trim().trim_end_matches("GMT").trim();
        // 尝试解析 "DD MMM YYYY HH:MM:SS" 部分
        let parts: Vec<&str> = val.split(|c: char| c == ' ' || c == ',').collect();
        let parts: Vec<&str> = parts.into_iter().filter(|s| !s.is_empty()).collect();

        // 期望格式中有日期部分
        let (day_str, month_str, year_str, time_str) = if parts.len() >= 4 {
            // 跳过 weekday, 找数字日、月、年
            // 找到数字日（1-31）
            let day_part = parts
                .iter()
                .find(|p| p.parse::<u32>().is_ok() && p.len() <= 2)?;
            let idx = parts.iter().position(|p| *p == *day_part)?;
            let day = day_part;
            let month = parts.get(idx + 1)?;
            let year = parts.get(idx + 2)?;
            let time = parts.get(idx + 3)?;
            (day, *month, *year, *time)
        } else if parts.len() >= 4 {
            // ANSI C: "EEE MMM DD HH:MM:SS YYYY"
            let day = parts.get(2)?;
            let month = parts.get(1)?;
            let year = parts.get(4)?;
            let time = parts.get(3)?;
            (day, *month, *year, *time)
        } else {
            return None;
        };

        let day: u32 = day_str.parse().ok()?;
        let month_idx = months
            .iter()
            .position(|m| m.eq_ignore_ascii_case(month_str))? as u32
            + 1;
        let year: u32 = year_str.parse().ok()?;

        let time_parts: Vec<&str> = time_str.split(':').collect();
        if time_parts.len() != 3 {
            return None;
        }
        let hour: u32 = time_parts[0].parse().ok()?;
        let min: u32 = time_parts[1].parse().ok()?;
        let sec: u32 = time_parts[2].parse().ok()?;

        // 使用简单时间计算（忽略时区，假设 GMT）
        // 用 Duration 近似计算 Unix timestamp
        let days_since_epoch = Self::days_from_ymd(year, month_idx, day);
        let total_secs =
            days_since_epoch as u64 * 86400 + hour as u64 * 3600 + min as u64 * 60 + sec as u64;

        let unix_epoch = Instant::now()
            - Duration::from_secs(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs(),
            );

        Some(unix_epoch + Duration::from_secs(total_secs))
    }

    /// 从年、月、日计算距 UNIX 纪元的天数
    fn days_from_ymd(year: u32, month: u32, day: u32) -> i64 {
        let y = year as i64;
        let m = month as i64;
        let d = day as i64;

        // 将 1 月、2 月视为前一年的 13 月、14 月
        let (y_adj, m_adj) = if m <= 2 { (y - 1, m + 12) } else { (y, m) };

        // 格里历公式
        let era = if y_adj >= 0 { y_adj } else { y_adj - 399 };
        let era_y = y_adj - era * 400;
        let yoe = if era_y > 0 { era_y - 1 } else { era_y };
        let doy = (153 * (m_adj - 3) + 2) / 5 + d - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;

        era * 146097 + doe - 719468
    }

    // ── Referer ──

    /// 更新 Referer 栈（仅导航 / navigate 类请求调用）
    fn update_referer(&self, url: &str) {
        if let Ok(mut stack) = REFERER_STACK.lock() {
            if stack.len() >= 10 {
                stack.remove(0);
            }
            stack.push(url.to_string());
        }
    }

    /// 获取当前 Referer
    fn get_referer(&self) -> Option<String> {
        if let Ok(stack) = REFERER_STACK.lock() {
            return stack.last().cloned();
        }
        None
    }

    // ── 公共 Request Builder（同步） ──

    /// 构建同步请求并返回原始 `reqwest::blocking::Response`
    fn build_blocking_request(
        &self,
        method: reqwest::Method,
        url: &str,
        parsed_url: &Url,
        body_option: Option<Vec<u8>>,
        content_type: Option<&str>,
        is_navigation: bool,
    ) -> Result<reqwest::blocking::Response, NetworkError> {
        // 构建 reqwest 客户端
        let client = reqwest::blocking::Client::builder()
            .user_agent(
                self.custom_ua
                    .clone()
                    .unwrap_or_else(|| CHROME_USER_AGENT.to_string()),
            )
            .timeout(Duration::from_secs(self.timeout_secs))
            .danger_accept_invalid_certs(false)
            .gzip(true)
            .brotli(true)
            // 使用环境变量中的代理设置（HTTP_PROXY, HTTPS_PROXY, NO_PROXY）
            .proxy(reqwest::Proxy::custom(|url| {
                // 检查环境变量中的代理设置
                if let Some(http_proxy) = std::env::var_os("HTTP_PROXY") {
                    if url.scheme() == "http" {
                        return Some(http_proxy.to_string_lossy().into_owned());
                    }
                }
                if let Some(https_proxy) = std::env::var_os("HTTPS_PROXY") {
                    if url.scheme() == "https" {
                        return Some(https_proxy.to_string_lossy().into_owned());
                    }
                }
                None
            }))
            .build()
            .map_err(|e| NetworkError::ClientCreationFailed(e.to_string()))?;

        let mut req = match method {
            reqwest::Method::GET => client.get(url),
            reqwest::Method::POST => {
                let mut r = client.post(url);
                if let Some(body) = body_option {
                    r = r.body(body);
                }
                if let Some(ct) = content_type {
                    r = r.header("Content-Type", ct);
                }
                r
            }
            _ => {
                return Err(NetworkError::RequestFailed(format!(
                    "不支持的 HTTP 方法: {:?}",
                    method
                )));
            }
        };

        // 添加标准浏览器请求头
        for (key, val) in Self::default_headers() {
            req = req.header(key, val);
        }

        // Cookie
        let cookie_header = self.get_cookies(parsed_url);
        if !cookie_header.is_empty() {
            req = req.header("Cookie", cookie_header);
        }

        // Referer
        if let Some(referer) = self.get_referer() {
            req = req.header("Referer", referer);
        }

        let response = req
            .send()
            .map_err(|e| NetworkError::RequestFailed(e.to_string()))?;

        // 处理 Set-Cookie（在 response 消费之前）
        if self.enable_cookies {
            // reqwest blocking Response 的 headers() 仍可用
            if let Some(set_cookie_headers) = response.headers().get("set-cookie") {
                if let Ok(cookie_str) = set_cookie_headers.to_str() {
                    self.parse_set_cookie(cookie_str, parsed_url);
                }
            }
        }

        // 导航请求才更新 Referer
        if is_navigation {
            self.update_referer(url);
        }

        Ok(response)
    }

    // ── 公共 Request Builder（异步） ──

    /// 构建异步请求并返回原始 `reqwest::Response`
    async fn build_async_request(
        &self,
        url: &str,
        parsed_url: &Url,
        is_navigation: bool,
    ) -> Result<reqwest::Response, NetworkError> {
        let client = reqwest::Client::builder()
            .user_agent(
                self.custom_ua
                    .clone()
                    .unwrap_or_else(|| CHROME_USER_AGENT.to_string()),
            )
            .timeout(Duration::from_secs(self.timeout_secs))
            .gzip(true)
            .brotli(true)
            .proxy(reqwest::Proxy::custom(|url| {
                if let Some(http_proxy) = std::env::var_os("HTTP_PROXY") {
                    if url.scheme() == "http" {
                        return Some(http_proxy.to_string_lossy().into_owned());
                    }
                }
                if let Some(https_proxy) = std::env::var_os("HTTPS_PROXY") {
                    if url.scheme() == "https" {
                        return Some(https_proxy.to_string_lossy().into_owned());
                    }
                }
                None
            }))
            .build()
            .map_err(|e| NetworkError::ClientCreationFailed(e.to_string()))?;

        let mut req = client.get(url);

        // 添加标准浏览器请求头
        for (key, val) in Self::default_headers() {
            req = req.header(key, val);
        }

        // Cookie
        let cookie_header = self.get_cookies(parsed_url);
        if !cookie_header.is_empty() {
            req = req.header("Cookie", cookie_header);
        }

        // Referer
        if let Some(referer) = self.get_referer() {
            req = req.header("Referer", referer);
        }

        let response = req
            .send()
            .await
            .map_err(|e| NetworkError::RequestFailed(e.to_string()))?;

        // 导航请求才更新 Referer
        if is_navigation {
            self.update_referer(url);
        }

        Ok(response)
    }

    // ── 公共响应解析 ──

    /// 从 `reqwest::Response` 解析为 `HttpResponse`
    fn parse_response_into_http_response(
        response: reqwest::blocking::Response,
        _original_url: &str,
    ) -> Result<HttpResponse, NetworkError> {
        let status = response.status().as_u16();
        let final_url = response.url().to_string();
        let headers: HashMap<String, String> = response
            .headers()
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or("").to_string()))
            .collect();

        let body = response
            .bytes()
            .map_err(|e| NetworkError::ReadBodyFailed(e.to_string()))?;

        trace!("收到响应，状态码: {}, final_url: {}", status, final_url);

        Ok(HttpResponse {
            status,
            headers,
            body: body.to_vec(),
            final_url,
        })
    }

    /// 从异步 `reqwest::Response` 解析为 `HttpResponse`
    async fn parse_async_response_into_http_response(
        response: reqwest::Response,
        _original_url: &str,
        parsed_url: &Url,
        enable_cookies: bool,
        cookie_parser: &NetworkClient,
    ) -> Result<HttpResponse, NetworkError> {
        let status = response.status().as_u16();
        let final_url = response.url().to_string();
        let headers: HashMap<String, String> = response
            .headers()
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or("").to_string()))
            .collect();

        // 处理 Set-Cookie（在读取 body 之前）
        if enable_cookies {
            if let Some(set_cookie) = headers.get("set-cookie") {
                cookie_parser.parse_set_cookie(set_cookie, parsed_url);
            }
        }

        let body = response
            .bytes()
            .await
            .map_err(|e| NetworkError::ReadBodyFailed(e.to_string()))?;

        trace!("收到响应，状态码: {}, final_url: {}", status, final_url);

        Ok(HttpResponse {
            status,
            headers,
            body: body.to_vec(),
            final_url,
        })
    }

    // ═════════════════════════════════════════════════════════════
    // 公开方法
    // ═════════════════════════════════════════════════════════════

    /// 异步 GET 请求，不更新 Referer（资源请求）。
    pub async fn fetch(&self, url: &str) -> Result<HttpResponse, NetworkError> {
        debug!("[fetch] GET {} (timeout: {}s)", url, self.timeout_secs);

        let parsed_url = Url::parse(url).map_err(|e| NetworkError::InvalidUrl(e.to_string()))?;

        let response = self.build_async_request(url, &parsed_url, false).await?;

        Self::parse_async_response_into_http_response(
            response,
            url,
            &parsed_url,
            self.enable_cookies,
            self,
        )
        .await
    }

    /// 异步导航 GET 请求，会更新 Referer。
    pub async fn fetch_navigation(&self, url: &str) -> Result<HttpResponse, NetworkError> {
        debug!(
            "[fetch_navigation] GET {} (timeout: {}s)",
            url, self.timeout_secs
        );

        let parsed_url = Url::parse(url).map_err(|e| NetworkError::InvalidUrl(e.to_string()))?;

        let response = self.build_async_request(url, &parsed_url, true).await?;

        Self::parse_async_response_into_http_response(
            response,
            url,
            &parsed_url,
            self.enable_cookies,
            self,
        )
        .await
    }

    /// 异步获取 HTML 内容，返回 `(body, final_url)` 元组。
    ///
    /// 成功时仅接受 HTTP 200 或 304 状态码。
    pub async fn fetch_html(&self, url: &str) -> Result<(String, String), NetworkError> {
        let response = self.fetch(url).await?;
        if response.status != 200 && response.status != 304 {
            return Err(NetworkError::HttpStatus(response.status));
        }
        let final_url = response.final_url.clone();
        let body = String::from_utf8_lossy(&response.body).into_owned();
        Ok((body, final_url))
    }

    /// 同步获取 HTML 内容（阻塞，用于渲染器线程），返回 `(body, final_url)` 元组。
    ///
    /// 成功时仅接受 HTTP 200 或 304 状态码。
    pub fn fetch_html_blocking(&self, url: &str) -> Result<(String, String), NetworkError> {
        debug!(
            "[fetch_html_blocking] GET {} (timeout: {}s)",
            url, self.timeout_secs
        );

        let parsed_url = Url::parse(url).map_err(|e| NetworkError::InvalidUrl(e.to_string()))?;

        let response =
            self.build_blocking_request(reqwest::Method::GET, url, &parsed_url, None, None, false)?;

        let status = response.status().as_u16();
        if status != 200 && status != 304 {
            return Err(NetworkError::HttpStatus(status));
        }

        let final_url = response.url().to_string();
        let body = response
            .bytes()
            .map_err(|e| NetworkError::ReadBodyFailed(e.to_string()))?;

        trace!("收到响应，状态码: {}, final_url: {}", status, final_url);

        let body_str = String::from_utf8_lossy(&body).into_owned();
        Ok((body_str, final_url))
    }

    /// 同步 HTTP GET 请求（阻塞），返回完整 `HttpResponse`。
    /// 不更新 Referer（资源请求）。
    pub fn get(&self, url: &str) -> Result<HttpResponse, NetworkError> {
        debug!("[get] GET {} (timeout: {}s)", url, self.timeout_secs);

        let parsed_url = Url::parse(url).map_err(|e| NetworkError::InvalidUrl(e.to_string()))?;

        let response =
            self.build_blocking_request(reqwest::Method::GET, url, &parsed_url, None, None, false)?;

        Self::parse_response_into_http_response(response, url)
    }

    /// 同步 HTTP POST 请求（阻塞），返回完整 `HttpResponse`。
    /// 不更新 Referer（资源请求）。
    pub fn post(
        &self,
        url: &str,
        body: &[u8],
        content_type: &str,
    ) -> Result<HttpResponse, NetworkError> {
        debug!("[post] POST {} (timeout: {}s)", url, self.timeout_secs);

        let parsed_url = Url::parse(url).map_err(|e| NetworkError::InvalidUrl(e.to_string()))?;

        let response = self.build_blocking_request(
            reqwest::Method::POST,
            url,
            &parsed_url,
            Some(body.to_vec()),
            Some(content_type),
            false,
        )?;

        Self::parse_response_into_http_response(response, url)
    }

    /// 同步导航 GET 请求（阻塞），会更新 Referer。
    pub fn navigate_blocking(&self, url: &str) -> Result<HttpResponse, NetworkError> {
        debug!(
            "[navigate_blocking] GET {} (timeout: {}s)",
            url, self.timeout_secs
        );

        let parsed_url = Url::parse(url).map_err(|e| NetworkError::InvalidUrl(e.to_string()))?;

        let response =
            self.build_blocking_request(reqwest::Method::GET, url, &parsed_url, None, None, true)?;

        Self::parse_response_into_http_response(response, url)
    }
}

// ═════════════════════════════════════════════════════════════════
// Trait 实现
// ═════════════════════════════════════════════════════════════════

impl Clone for NetworkClient {
    fn clone(&self) -> Self {
        Self {
            timeout_secs: self.timeout_secs,
            enable_cookies: self.enable_cookies,
            custom_ua: self.custom_ua.clone(),
            cookie_jar: std::sync::Arc::clone(&self.cookie_jar),
        }
    }
}

impl Default for NetworkClient {
    fn default() -> Self {
        Self::new()
    }
}

// ═════════════════════════════════════════════════════════════════
// 测试
// ═════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_path() {
        assert_eq!(NetworkClient::default_path("/"), "/");
        assert_eq!(NetworkClient::default_path("/foo/bar"), "/foo/");
        assert_eq!(NetworkClient::default_path("/foo/bar/baz"), "/foo/bar/");
        // RFC 6265 示例
        // 请求 URI = /foo/bar, 不设置 Path, default-path = /foo/
        assert_eq!(NetworkClient::default_path("/foo/bar"), "/foo/");
    }

    #[test]
    fn test_cookie_no_expiry() {
        let client = NetworkClient::new().with_cookies_disabled();
        // 不触发实际网络，仅测试解析逻辑
        let url = Url::parse("https://example.com/").unwrap();
        client.parse_set_cookie("session=abc123; Path=/", &url);
        // 禁用 cookie 时应无操作
        assert!(client.get_cookies(&url).is_empty());
    }

    #[test]
    fn test_cookie_name_value_with_equals() {
        // 确保值中的 "=" 不被截断
        // 由于 parse_set_cookie 需要 enable_cookies=true
        let client = NetworkClient::new();
        let url = Url::parse("https://example.com/").unwrap();
        client.parse_set_cookie("token=abc=def==; Path=/", &url);
        let cookies = client.get_cookies(&url);
        assert!(
            cookies.contains("token=abc=def=="),
            "cookie value should preserve '=': {}",
            cookies
        );
    }

    #[test]
    fn test_cookie_max_age_zero_removes() {
        let client = NetworkClient::new();
        let url = Url::parse("https://example.com/").unwrap();
        // 先添加一个 cookie
        client.parse_set_cookie("test=value; Path=/; Max-Age=100", &url);
        let cookies = client.get_cookies(&url);
        assert!(
            cookies.contains("test=value"),
            "cookie should exist: {}",
            cookies
        );

        // 用 Max-Age=0 删除
        client.parse_set_cookie("test=value; Path=/; Max-Age=0", &url);
        let cookies = client.get_cookies(&url);
        assert!(
            !cookies.contains("test=value"),
            "cookie should be removed: {}",
            cookies
        );
    }

    #[test]
    fn test_cookie_domain_attribute() {
        let client = NetworkClient::new();
        // 从 sub.example.com 设置 Domain=example.com
        let url = Url::parse("https://sub.example.com/path/").unwrap();
        client.parse_set_cookie("user=alice; Domain=example.com; Path=/", &url);

        // 对 example.com 应该可以访问此 cookie
        let url2 = Url::parse("https://example.com/other").unwrap();
        let cookies = client.get_cookies(&url2);
        assert!(
            cookies.contains("user=alice"),
            "cookie should match domain: {}",
            cookies
        );
    }

    #[test]
    fn test_referer_only_navigation() {
        let client = NetworkClient::new();
        // 开始无 referer
        assert!(client.get_referer().is_none());

        // 导航更新 referer
        client.update_referer("https://page1.com/");
        assert_eq!(client.get_referer().unwrap(), "https://page1.com/");

        // 再次导航
        client.update_referer("https://page2.com/");
        assert_eq!(client.get_referer().unwrap(), "https://page2.com/");
    }

    #[test]
    fn test_with_own_cookie_jar_independence() {
        let shared_client = NetworkClient::new();
        let own_client = NetworkClient::new().with_own_cookie_jar();

        let url = Url::parse("https://example.com/").unwrap();

        // 向共享 jar 添加 cookie
        shared_client.parse_set_cookie("shared=cookie; Path=/", &url);

        // 共享客户端能看到
        assert!(shared_client.get_cookies(&url).contains("shared=cookie"));

        // 独立客户端看不到
        assert!(!own_client.get_cookies(&url).contains("shared=cookie"));
    }
}
