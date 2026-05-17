//! RFC 6265 兼容的 Cookie jar
//!
//! 提供 Cookie 解析、存储、匹配和持久化功能。
//! 使用 `std::time` 代替 `chrono` 以最小化依赖。

use lazy_static::lazy_static;
use log::{debug, warn};
use std::collections::HashMap;
use std::fmt;
use std::fs;
use std::io::{Read, Write};
use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use url::Url;

// ---------------------------------------------------------------------------
// 类型别名
// ---------------------------------------------------------------------------

/// Unix 时间戳（毫秒）
type TimestampMs = u64;

// ---------------------------------------------------------------------------
// SameSite
// ---------------------------------------------------------------------------

/// SameSite 属性（RFC 6265bis）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SameSite {
    /// 仅限同站请求发送
    Strict,
    /// 同站请求和部分顶级导航发送
    Lax,
    /// 所有请求都发送（需要 Secure 标志）
    None,
}

impl SameSite {
    fn from_str(s: &str) -> Self {
        match s.trim().to_lowercase().as_str() {
            "strict" => SameSite::Strict,
            "lax" => SameSite::Lax,
            "none" => SameSite::None,
            _ => {
                warn!("未知 SameSite 值: '{}', 默认使用 Lax", s.trim());
                SameSite::Lax
            }
        }
    }

    fn to_str(&self) -> &'static str {
        match self {
            SameSite::Strict => "Strict",
            SameSite::Lax => "Lax",
            SameSite::None => "None",
        }
    }
}

// ---------------------------------------------------------------------------
// CookieExpires
// ---------------------------------------------------------------------------

/// Cookie 过期策略
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CookieExpires {
    /// 会话 cookie（浏览器关闭时删除）
    Session,
    /// 在指定的绝对时间过期
    At(SystemTime),
    /// 从创建时起经过指定时长后过期
    MaxAge(Duration),
}

impl CookieExpires {
    /// 检查 cookie 是否已过期（返回 `true` 表示已过期）
    pub fn is_expired(&self, now: SystemTime) -> bool {
        match self {
            CookieExpires::Session => false, // 会话 cookie 不过期（在会话结束时清除）
            CookieExpires::At(at) => now >= *at,
            CookieExpires::MaxAge(_age) => {
                // 使用当前时间判断：如果 now > created + age，则过期
                // 但我们没有 created 时间，所以这个方法由调用方提供 created
                // 这个简化的版本始终返回 false，由外部逻辑处理
                false
            }
        }
    }

    /// 根据创建时间判断 MaxAge cookie 是否过期
    pub fn is_expired_at(&self, created: SystemTime, now: SystemTime) -> bool {
        match self {
            CookieExpires::Session => false,
            CookieExpires::At(at) => now >= *at,
            CookieExpires::MaxAge(age) => {
                if let Some(expires) = created.checked_add(*age) {
                    now >= expires
                } else {
                    false
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Cookie
// ---------------------------------------------------------------------------

/// 单个 Cookie（RFC 6265）
#[derive(Debug, Clone)]
pub struct Cookie {
    /// Cookie 名称
    pub name: String,
    /// Cookie 值
    pub value: String,
    /// 作用域域名（RFC 6265 §5.1.3）
    pub domain: String,
    /// 作用域路径（RFC 6265 §5.1.4）
    pub path: String,
    /// 过期策略
    pub expires: CookieExpires,
    /// 仅 HTTPS 发送
    pub secure: bool,
    /// 禁止 JavaScript 访问（HttpOnly）
    pub http_only: bool,
    /// SameSite 属性
    pub same_site: SameSite,
    /// 创建时间（Unix 毫秒时间戳）
    pub created: TimestampMs,
}

impl Cookie {
    /// 创建一个新的会话 cookie
    pub fn new(name: String, value: String, domain: String, path: String) -> Self {
        let created = now_ms();
        Cookie {
            name,
            value,
            domain,
            path,
            expires: CookieExpires::Session,
            secure: false,
            http_only: false,
            same_site: SameSite::Lax,
            created,
        }
    }

    /// 检查 cookie 是否已过期
    pub fn is_expired(&self) -> bool {
        let now = SystemTime::now();
        match &self.expires {
            CookieExpires::Session => false,
            CookieExpires::At(at) => now >= *at,
            CookieExpires::MaxAge(age) => {
                let created_time = UNIX_EPOCH + Duration::from_millis(self.created);
                if let Some(expires) = created_time.checked_add(*age) {
                    now >= expires
                } else {
                    false
                }
            }
        }
    }
}

impl fmt::Display for Cookie {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}={}", self.name, self.value)?;
        if !self.domain.is_empty() {
            write!(f, "; Domain={}", self.domain)?;
        }
        if !self.path.is_empty() {
            write!(f, "; Path={}", self.path)?;
        }
        match &self.expires {
            CookieExpires::Session => {}
            CookieExpires::MaxAge(age) => {
                write!(f, "; Max-Age={}", age.as_secs())?;
            }
            CookieExpires::At(at) => {
                if let Some(ts) = at.duration_since(UNIX_EPOCH).ok() {
                    // 格式化为 RFC 1123 格式
                    let secs = ts.as_secs();
                    if let Some(rfc1123) = rfc1123_from_unix(secs) {
                        write!(f, "; Expires={}", rfc1123)?;
                    }
                }
            }
        }
        if self.secure {
            write!(f, "; Secure")?;
        }
        if self.http_only {
            write!(f, "; HttpOnly")?;
        }
        if self.same_site != SameSite::Lax {
            write!(f, "; SameSite={}", self.same_site.to_str())?;
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// CookieJar
// ---------------------------------------------------------------------------

/// RFC 6265 兼容的 Cookie 容器
///
/// 以域名为键，每个域名下维护一个 Cookie 列表。
#[derive(Debug, Clone)]
pub struct CookieJar {
    cookies: HashMap<String, Vec<Cookie>>,
}

impl CookieJar {
    /// 创建空的 CookieJar
    pub fn new() -> Self {
        CookieJar {
            cookies: HashMap::new(),
        }
    }

    // -----------------------------------------------------------------------
    // RFC 6265 匹配规则
    // -----------------------------------------------------------------------

    /// RFC 6265 §5.1.3：域名匹配
    ///
    /// 如果 cookie 的作用域域名 `cookie_domain` 是请求域名 `domain` 的后缀，
    /// 或者两者完全相同，则匹配成功。
    ///
    /// 例如，cookie 作用于 "example.com" 可以匹配 "sub.example.com"。
    /// 注意：cookie 域名不能是公共后缀（如 ".com"），但此处不校验该规则。
    pub fn domain_match(domain: &str, cookie_domain: &str) -> bool {
        let domain = domain.trim_matches('.').to_lowercase();
        let cookie_domain = cookie_domain.trim_matches('.').to_lowercase();

        if domain == cookie_domain {
            return true;
        }

        // cookie_domain 必须是 domain 的后缀，且 domain 比 cookie_domain 长，
        // 并且 domain 中在 cookie_domain 前面紧跟着一个 '.'
        if domain.ends_with(&cookie_domain) {
            let dot_idx = domain.len().saturating_sub(cookie_domain.len()).wrapping_sub(1);
            if domain.as_bytes().get(dot_idx) == Some(&b'.') {
                return true;
            }
        }

        false
    }

    /// RFC 6265 §5.1.4：路径匹配
    ///
    /// 如果请求的 URI 路径 `path` 以 cookie 的路径 `cookie_path` 开头，则匹配成功。
    ///
    /// 例如，cookie 路径为 "/foo" 可以匹配 "/foo/bar" 和 "/foo" 本身。
    pub fn path_match(path: &str, cookie_path: &str) -> bool {
        // 空路径不应匹配任何内容（RFC 6265 中路径属性不会为空）
        if cookie_path.is_empty() {
            return false;
        }
        // RFC 6265: 如果 cookie_path 等于 path 的前 cookie_path.len() 个字符，则匹配
        path.starts_with(cookie_path)
            // 特殊规则：如果 cookie 路径不是 "/"，那么 path 要么等于 cookie_path，
            // 要么在 cookie_path 后面跟着一个 "/"
            && (cookie_path == "/"
                || path.len() == cookie_path.len()
                || path.as_bytes().get(cookie_path.len()) == Some(&b'/'))
    }

    // -----------------------------------------------------------------------
    // 解析 Set-Cookie 头（RFC 6265 §5.2）
    // -----------------------------------------------------------------------

    /// 解析 `Set-Cookie` 响应头，返回可选的 `Cookie`。
    ///
    /// 遵循 RFC 6265 §5.2 的解析算法。
    pub fn parse_set_cookie(header: &str, url: &Url) -> Option<Cookie> {
        // 步骤 1-2：将 header 按 ';' 分割成 cookie-pair 和 attribute 列表
        let parts: Vec<&str> = header.split(';').collect();
        if parts.is_empty() {
            return None;
        }

        // 步骤 3：解析 cookie-name 和 cookie-value
        let name_value = parts[0].trim();
        let (name, value) = match name_value.split_once('=') {
            Some((n, v)) => (n.trim().to_string(), v.trim().to_string()),
            None => return None,
        };

        // RFC 6265 §4.1.1：cookie-name 不能为空
        if name.is_empty() {
            return None;
        }

        // 初始化默认值
        let mut domain = String::new();
        let mut path = String::new();
        let mut expires = CookieExpires::Session;
        let mut secure = false;
        let mut http_only = false;
        let mut same_site = SameSite::Lax;

        // 步骤 4：解析属性列表
        for attr in &parts[1..] {
            let attr = attr.trim();
            if attr.is_empty() {
                continue;
            }

            let (attr_name, attr_value) = match attr.split_once('=') {
                Some((n, v)) => (n.trim().to_lowercase(), Some(v.trim())),
                None => (attr.to_lowercase(), None),
            };

            match attr_name.as_str() {
                "domain" => {
                    if let Some(val) = attr_value {
                        let val = val.trim_matches('.');
                        if !val.is_empty() {
                            domain = val.to_lowercase();
                        }
                    }
                }
                "path" => {
                    if let Some(val) = attr_value {
                        let val = val.trim();
                        if val.starts_with('/') {
                            path = val.to_string();
                        } else if !val.is_empty() {
                            path = format!("/{}", val);
                        }
                    }
                }
                "expires" => {
                    if let Some(val) = attr_value {
                        if let Some(ts) = parse_rfc1123(val.trim()) {
                            let system_time = UNIX_EPOCH + Duration::from_secs(ts);
                            expires = CookieExpires::At(system_time);
                        }
                    }
                }
                "max-age" => {
                    if let Some(val) = attr_value {
                        // 解析 Max-Age 值（秒）
                        let val = val.trim();
                        if let Ok(secs) = val.parse::<i64>() {
                            if secs <= 0 {
                                // Max-Age <= 0 表示立即过期
                                // 返回 None 表示不存储该 cookie
                                return None;
                            }
                            expires = CookieExpires::MaxAge(Duration::from_secs(secs as u64));
                        }
                    }
                }
                "secure" => {
                    secure = true;
                }
                "httponly" => {
                    http_only = true;
                }
                "samesite" => {
                    if let Some(val) = attr_value {
                        same_site = SameSite::from_str(val);
                    }
                }
                _ => {}
            }
        }

        // 步骤 5-6：域名处理
        let host = url.host_str().unwrap_or("");
        if domain.is_empty() {
            // 未指定 Domain 属性，默认使用请求域名
            domain = host.to_lowercase();
        } else {
            // 指定了 Domain 属性，需要进行域匹配校验
            // RFC 6265 §5.2.3：域名必须与当前请求域名匹配
            // 这里我们只做简单校验：如果指定域名不是 host 的后缀，则拒绝
            if !CookieJar::domain_match(host, &domain) {
                warn!(
                    "Set-Cookie 域名不匹配: cookie_domain={}, request_host={}",
                    domain, host
                );
                return None;
            }
        }

        // 步骤 7-8：路径处理
        if path.is_empty() {
            // 未指定 Path 属性，使用默认路径
            let url_path = url.path();
            if url_path.is_empty() || !url_path.starts_with('/') {
                path = "/".to_string();
            } else {
                // 取请求路径的目录部分
                path = match url_path.rfind('/') {
                    Some(idx) if idx == 0 => "/".to_string(),
                    Some(idx) => url_path[..idx].to_string(),
                    None => "/".to_string(),
                };
            }
        }

        let created = now_ms();

        Some(Cookie {
            name,
            value,
            domain,
            path,
            expires,
            secure,
            http_only,
            same_site,
            created,
        })
    }

    // -----------------------------------------------------------------------
    // 生成 Cookie 请求头（RFC 6265 §5.4）
    // -----------------------------------------------------------------------

    /// 为指定的 URL 生成 `Cookie` 头的值。
    ///
    /// 只返回未过期、域名匹配、路径匹配且满足 Secure 要求的 Cookie。
    /// 结果按路径长度降序排列，路径相同时按创建时间升序排列。
    pub fn cookies_for_url(&self, url: &Url) -> String {
        let host = url.host_str().unwrap_or("").to_lowercase();
        let path = url.path();
        let is_secure = url.scheme() == "https";
        let now = SystemTime::now();

        let mut matched: Vec<&Cookie> = Vec::new();

        // 遍历所有域名的 cookie
        for (_, cookies) in self.cookies.iter() {
            for cookie in cookies {
                // 跳过过期 cookie
                match &cookie.expires {
                    CookieExpires::Session => {}
                    CookieExpires::At(at) => {
                        if now >= *at {
                            continue;
                        }
                    }
                    CookieExpires::MaxAge(age) => {
                        let created_time = UNIX_EPOCH + Duration::from_millis(cookie.created);
                        if let Some(expires) = created_time.checked_add(*age) {
                            if now >= expires {
                                continue;
                            }
                        }
                    }
                }

                // 域名匹配
                if !CookieJar::domain_match(&host, &cookie.domain) {
                    continue;
                }

                // 路径匹配
                if !CookieJar::path_match(path, &cookie.path) {
                    continue;
                }

                // Secure 匹配：如果 cookie 标记为 secure，仅通过 HTTPS 发送
                if cookie.secure && !is_secure {
                    continue;
                }

                // SameSite 检查：跨站请求时 Strict 不发送
                // 简化处理：这里我们只做基本检查，完整的 SameSite 逻辑需要区分顶级导航
                // 我们假设本函数仅用于常规请求（非顶级导航），因此 Strict 也跳过
                // 完整的实现需要请求上下文信息

                matched.push(cookie);
            }
        }

        // 按路径长度降序排列，路径相同时按创建时间升序
        matched.sort_by(|a, b| {
            b.path
                .len()
                .cmp(&a.path.len())
                .then_with(|| a.created.cmp(&b.created))
        });

        // 格式化为 "name1=value1; name2=value2"
        let cookie_strings: Vec<String> = matched
            .iter()
            .map(|c| format!("{}={}", c.name, c.value))
            .collect();

        cookie_strings.join("; ")
    }

    // -----------------------------------------------------------------------
    // Cookie 存储操作
    // -----------------------------------------------------------------------

    /// 插入一个 cookie。
    ///
    /// 如果已存在相同（name, domain, path）的 cookie，则替换之。
    pub fn insert(&mut self, cookie: Cookie) {
        let domain = cookie.domain.clone();
        let name = cookie.name.clone();
        let path = cookie.path.clone();

        let cookies = self.cookies.entry(domain.clone()).or_insert_with(Vec::new);

        // 寻找并替换同名同域同路径的 cookie
        if let Some(pos) = cookies.iter().position(|c| {
            c.name == name && c.domain == domain && c.path == path
        }) {
            cookies[pos] = cookie;
        } else {
            cookies.push(cookie);
        }
    }

    /// 移除所有已过期的 cookie。
    pub fn remove_expired(&mut self) {
        let now = SystemTime::now();

        for cookies in self.cookies.values_mut() {
            cookies.retain(|c| {
                match &c.expires {
                    CookieExpires::Session => true,
                    CookieExpires::At(at) => now < *at,
                    CookieExpires::MaxAge(age) => {
                        let created_time = UNIX_EPOCH + Duration::from_millis(c.created);
                        if let Some(expires) = created_time.checked_add(*age) {
                            now < expires
                        } else {
                            true
                        }
                    }
                }
            });
        }

        // 清理空域名的条目
        self.cookies.retain(|_, cookies| !cookies.is_empty());
    }

    /// 清除指定域名下的所有 cookie（包括子域名）。
    pub fn clear_for_origin(&mut self, origin: &str) {
        let origin = origin.trim_matches('.').to_lowercase();

        // 移除精确匹配和子域名匹配的条目
        let keys_to_remove: Vec<String> = self
            .cookies
            .keys()
            .filter(|domain| CookieJar::domain_match(domain, &origin))
            .cloned()
            .collect();

        for key in &keys_to_remove {
            self.cookies.remove(key);
        }
    }

    /// 清除所有 cookie。
    pub fn clear_all(&mut self) {
        self.cookies.clear();
    }

    /// 返回 cookie jar 中的 cookie 总数。
    pub fn len(&self) -> usize {
        self.cookies.values().map(|v| v.len()).sum()
    }

    /// 返回 cookie jar 是否为空。
    pub fn is_empty(&self) -> bool {
        self.cookies.values().all(|v| v.is_empty())
    }

    /// 返回所有 cookie 的引用列表。
    pub fn all_cookies(&self) -> Vec<&Cookie> {
        self.cookies
            .values()
            .flat_map(|v| v.iter())
            .collect()
    }

    // -----------------------------------------------------------------------
    // JSON 序列化 / 反序列化
    // -----------------------------------------------------------------------

    /// 将 CookieJar 序列化为 JSON 字符串。
    pub fn to_json(&self) -> String {
        let mut json = String::from("{\"cookies\":[");

        let mut first = true;
        for (_domain, cookies) in &self.cookies {
            for cookie in cookies {
                if !first {
                    json.push(',');
                }
                first = false;

                json.push('{');
                json.push_str(&format!(
                    r#""name":{},"value":{},"domain":{},"path":{},"secure":{},"http_only":{},"same_site":{},"created":{}"#,
                    json_escape(&cookie.name),
                    json_escape(&cookie.value),
                    json_escape(&cookie.domain),
                    json_escape(&cookie.path),
                    if cookie.secure { "true" } else { "false" },
                    if cookie.http_only { "true" } else { "false" },
                    json_escape(SameSite::to_str(&cookie.same_site)),
                    cookie.created,
                ));

                // 序列化 expires
                json.push_str(",\"expires\":");
                match &cookie.expires {
                    CookieExpires::Session => {
                        json.push_str(r#"{"type":"session"}"#);
                    }
                    CookieExpires::At(at) => {
                        if let Ok(d) = at.duration_since(UNIX_EPOCH) {
                            json.push_str(&format!(
                                r#"{{"type":"at","timestamp_ms":{}}}"#,
                                d.as_millis()
                            ));
                        } else {
                            json.push_str(r#"{"type":"session"}"#);
                        }
                    }
                    CookieExpires::MaxAge(age) => {
                        json.push_str(&format!(
                            r#"{{"type":"max_age","secs":{}}}"#,
                            age.as_secs()
                        ));
                    }
                }

                json.push('}');
            }
        }

        json.push_str("]}");
        json
    }

    /// 从 JSON 字符串解析 CookieJar。
    pub fn from_json(json: &str) -> Self {
        let mut jar = CookieJar::new();

        // 简单 JSON 解析（仅处理我们的格式）
        // 查找 "cookies":[ ... ]
        let json = json.trim();
        if !json.starts_with('{') {
            return jar;
        }

        // 提取 cookies 数组
        let cookies_str = match extract_json_array(json, "cookies") {
            Some(s) => s,
            None => return jar,
        };

        if cookies_str.is_empty() {
            return jar;
        }

        // 解析每个 cookie 对象
        let mut depth = 0;
        let mut start = 0;
        let mut in_string = false;
        let mut escape = false;

        for (i, ch) in cookies_str.char_indices() {
            if escape {
                escape = false;
                continue;
            }
            if ch == '\\' && in_string {
                escape = true;
                continue;
            }
            if ch == '"' {
                in_string = !in_string;
                continue;
            }
            if in_string {
                continue;
            }

            match ch {
                '{' => {
                    if depth == 0 {
                        start = i;
                    }
                    depth += 1;
                }
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        let obj_str = &cookies_str[start..=i];
                        if let Some(cookie) = parse_cookie_json(obj_str) {
                            jar.insert(cookie);
                        }
                    }
                }
                _ => {}
            }
        }

        jar
    }

    // -----------------------------------------------------------------------
    // 文件持久化
    // -----------------------------------------------------------------------

    /// 将 CookieJar 原子地保存到文件（先写入 .tmp，再 rename）。
    pub fn save_to_file(&self, path: &Path) -> Result<(), String> {
        let json = self.to_json();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("无法创建目录: {}", e))?;
        }
        // 写入 .tmp 文件，然后 rename，保证原子性
        let tmp_path = path.with_extension("tmp");
        let mut file = fs::File::create(&tmp_path).map_err(|e| format!("创建临时文件失败: {}", e))?;
        file.write_all(json.as_bytes())
            .map_err(|e| format!("写入临时文件失败: {}", e))?;
        drop(file);
        fs::rename(&tmp_path, path).map_err(|e| format!("重命名文件失败: {}", e))?;
        debug!("Cookie jar 已保存到 {:?} (原子写入)", path);
        Ok(())
    }

    /// 从文件加载 CookieJar。
    pub fn load_from_file(path: &Path) -> Self {
        let mut file = match fs::File::open(path) {
            Ok(f) => f,
            Err(e) => {
                debug!("Cookie 文件不存在 ({}), 使用空 jar", e);
                return CookieJar::new();
            }
        };

        let mut contents = String::new();
        if let Err(e) = file.read_to_string(&mut contents) {
            warn!("读取 Cookie 文件失败: {}, 使用空 jar", e);
            return CookieJar::new();
        }

        let jar = CookieJar::from_json(&contents);
        debug!("Cookie jar 已从 {:?} 加载, 共 {} 个 cookie", path, jar.len());
        jar
    }
}

impl Default for CookieJar {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for CookieJar {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let total = self.len();
        writeln!(f, "CookieJar ({} cookies):", total)?;
        for (domain, cookies) in &self.cookies {
            writeln!(f, "  [{}] {} cookie(s):", domain, cookies.len())?;
            for cookie in cookies {
                writeln!(f, "    {}", cookie)?;
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 全局 CookieJar 单例
// ---------------------------------------------------------------------------

lazy_static! {
    /// 全局共享的 CookieJar，提供线程安全的访问。
    pub static ref COOKIE_JAR: Mutex<CookieJar> = Mutex::new(CookieJar::new());
}

/// 默认 Cookie 持久化路径：在 `data_dir` 下的 `cookies.json`
pub fn default_cookie_path(data_dir: &Path) -> std::path::PathBuf {
    data_dir.join("cookies.json")
}

// ---------------------------------------------------------------------------
// 时间工具函数
// ---------------------------------------------------------------------------

/// 返回当前 Unix 时间戳（毫秒）
fn now_ms() -> TimestampMs {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as TimestampMs
}

/// 月份名称到数字的映射
const MONTH_MAP: &[(&str, u64)] = &[
    ("Jan", 1),
    ("Feb", 2),
    ("Mar", 3),
    ("Apr", 4),
    ("May", 5),
    ("Jun", 6),
    ("Jul", 7),
    ("Aug", 8),
    ("Sep", 9),
    ("Oct", 10),
    ("Nov", 11),
    ("Dec", 12),
];

/// 星期名称
const _DAY_NAMES: &[&str] = &["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];

/// 解析 RFC 1123 格式的日期字符串为 Unix 时间戳（秒）。
///
/// 格式示例: "Wed, 21 Oct 2015 07:28:00 GMT"
fn parse_rfc1123(s: &str) -> Option<u64> {
    let s = s.trim();
    // 去除末尾的 " GMT"
    let s = s.strip_suffix(" GMT").or_else(|| s.strip_suffix(" gmt"))?;

    // 分割逗号前的部分和日期部分
    // 格式: [Wdy,] DD Mon YYYY HH:MM:SS
    let after_weekday = if let Some(idx) = s.find(',') {
        s[idx + 1..].trim()
    } else {
        s.trim()
    };

    let parts: Vec<&str> = after_weekday.split_whitespace().collect();
    if parts.len() < 4 {
        return None;
    }

    // parts[0] = day, parts[1] = month, parts[2] = year, parts[3] = time
    let day: u64 = parts[0].parse().ok()?;
    let month_str = parts[1];
    let year: u64 = parts[2].parse().ok()?;

    let month = MONTH_MAP
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(month_str))?
        .1;

    // 解析时间 HH:MM:SS
    let time_parts: Vec<&str> = parts[3].split(':').collect();
    if time_parts.len() < 3 {
        return None;
    }
    let hour: u64 = time_parts[0].parse().ok()?;
    let minute: u64 = time_parts[1].parse().ok()?;
    let second: u64 = time_parts[2].parse().ok()?;

    // 计算 Unix 时间戳（简单近似，不考虑闰秒）
    let days_since_epoch = days_from_year_month_day(year, month, day);
    let total_secs = days_since_epoch * 86400 + hour * 3600 + minute * 60 + second;

    Some(total_secs)
}

/// 计算从 Unix 纪元（1970-01-01）到给定日期的天数
fn days_from_year_month_day(year: u64, month: u64, day: u64) -> u64 {
    // 计算从 1970 年到 year 年的天数
    let days_from_year = |y: u64| -> u64 {
        if y == 0 {
            return 0;
        }
        let y = y - 1;
        y * 365 + y / 4 - y / 100 + y / 400
    };

    let days_in_months: [u64; 12] = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];

    // 判断给定年份是否为闰年
    let is_leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;

    let month_days: u64 = days_in_months[..((month - 1) as usize)]
        .iter()
        .sum::<u64>()
        + if month > 2 && is_leap { 1 } else { 0 };

    let total_days = days_from_year(year) + month_days + day - 1;
    total_days - days_from_year(1970)
}

/// 将 Unix 时间戳（秒）格式化为 RFC 1123 日期字符串
fn rfc1123_from_unix(secs: u64) -> Option<String> {
    // 计算年月日
    let days = secs / 86400;
    let time_secs = secs % 86400;
    let hour = time_secs / 3600;
    let minute = (time_secs % 3600) / 60;
    let second = time_secs % 60;

    let (year, month, day) = days_to_ymd(days)?;

    let month_name = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ][(month - 1) as usize];

    let day_name = match secs / 86400 % 7 {
        0 => "Thu",
        1 => "Fri",
        2 => "Sat",
        3 => "Sun",
        4 => "Mon",
        5 => "Tue",
        6 => "Wed",
        _ => unreachable!(),
    };

    Some(format!(
        "{}, {:02} {} {:04} {:02}:{:02}:{:02} GMT",
        day_name, day, month_name, year, hour, minute, second
    ))
}

/// 将天数转换为 (年, 月, 日)
fn days_to_ymd(days: u64) -> Option<(u64, u64, u64)> {
    // 近似年份
    let mut year = 1970;
    let mut remaining = days as i64;

    loop {
        let days_in_year = if is_leap_year(year) { 366 } else { 365 };
        if remaining < days_in_year {
            break;
        }
        remaining -= days_in_year;
        year += 1;

        // 防止无限循环（支持到 9999 年）
        if year > 9999 {
            return None;
        }
    }

    let is_leap = is_leap_year(year);
    let month_days: [i64; 12] = [
        31,
        if is_leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];

    let mut month = 1u64;
    for &md in &month_days {
        if remaining < md {
            break;
        }
        remaining -= md;
        month += 1;
    }

    let day = (remaining + 1) as u64;
    Some((year, month, day))
}

/// 判断给定年份是否为闰年
fn is_leap_year(year: u64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

// ---------------------------------------------------------------------------
// JSON 工具函数
// ---------------------------------------------------------------------------

/// 对字符串进行 JSON 转义
fn json_escape(s: &str) -> String {
    let mut escaped = String::with_capacity(s.len() + 2);
    escaped.push('"');
    for ch in s.chars() {
        match ch {
            '"' => escaped.push_str("\\\""),
            '\\' => escaped.push_str("\\\\"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                escaped.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => escaped.push(c),
        }
    }
    escaped.push('"');
    escaped
}

/// 提取 JSON 对象中指定键的数组内容（包含方括号内的字符）。
///
/// 例如，对于 `{"cookies":[...]}`，提取 `[...]` 内部的字符串。
fn extract_json_array<'a>(json: &'a str, key: &str) -> Option<&'a str> {
    // 查找 "key":
    let search = &format!(r#""{}":"#, key);
    let start = json.find(search.as_str())?;
    let after_key = &json[start + search.len()..];

    // 跳过空白
    let after_key = after_key.trim_start();

    // 找到 '['
    if !after_key.starts_with('[') {
        return None;
    }

    // 找到匹配的 ']'
    let mut depth = 0u32;
    let mut in_string = false;
    let mut escape = false;
    for (i, ch) in after_key.char_indices() {
        if escape {
            escape = false;
            continue;
        }
        if ch == '\\' && in_string {
            escape = true;
            continue;
        }
        if ch == '"' {
            in_string = !in_string;
            continue;
        }
        if in_string {
            continue;
        }
        match ch {
            '[' => depth += 1,
            ']' => {
                depth -= 1;
                if depth == 0 {
                    // 返回方括号内的内容
                    return Some(&after_key[1..i]);
                }
            }
            _ => {}
        }
    }

    None
}

/// 从 JSON 对象字符串中解析一个 cookie。
///
/// 期望格式：`{"name":"...","value":"...","domain":"...","path":"...",...}`
fn parse_cookie_json(obj: &str) -> Option<Cookie> {
    let obj = obj.trim();
    if !obj.starts_with('{') || !obj.ends_with('}') {
        return None;
    }

    let inner = &obj[1..obj.len() - 1];

    let name = extract_json_str(inner, "name")?;
    let value = extract_json_str(inner, "value")?;
    let domain = extract_json_str(inner, "domain")?;
    let path = extract_json_str(inner, "path")?;
    let secure = extract_json_bool(inner, "secure").unwrap_or(false);
    let http_only = extract_json_bool(inner, "http_only").unwrap_or(false);
    let same_site_str = extract_json_str(inner, "same_site").unwrap_or_else(|| "Lax".to_string());
    let created = extract_json_u64(inner, "created").unwrap_or_else(now_ms);

    let same_site = SameSite::from_str(&same_site_str);

    // 解析 expires
    let expires = parse_expires_json(inner);

    Some(Cookie {
        name,
        value,
        domain,
        path,
        expires,
        secure,
        http_only,
        same_site,
        created,
    })
}

/// 从 JSON 键值区域中提取字符串值。
fn extract_json_str<'a>(s: &'a str, key: &str) -> Option<String> {
    let search = &format!(r#""{}":"#, key);
    let start = s.find(search.as_str())?;
    let after_key = &s[start + search.len()..];

    // 跳过空白
    let after_key = after_key.trim_start();

    // 必须以 '"' 开头
    let after_key = after_key.strip_prefix('"')?;

    // 查找结束的 '"'（考虑转义）
    let mut escape = false;
    let mut end = 0;
    for (i, ch) in after_key.char_indices() {
        if escape {
            escape = false;
            continue;
        }
        if ch == '\\' {
            escape = true;
            continue;
        }
        if ch == '"' {
            end = i;
            break;
        }
    }

    Some(json_unescape(&after_key[..end]))
}

/// 从 JSON 键值区域中提取布尔值。
fn extract_json_bool(s: &str, key: &str) -> Option<bool> {
    let search = &format!(r#""{}":"#, key);
    let start = s.find(search.as_str())?;
    let after_key = &s[start + search.len()..];
    let after_key = after_key.trim_start();

    if after_key.starts_with("true") {
        Some(true)
    } else if after_key.starts_with("false") {
        Some(false)
    } else {
        None
    }
}

/// 从 JSON 键值区域中提取 u64 值。
fn extract_json_u64(s: &str, key: &str) -> Option<u64> {
    let search = &format!(r#""{}":"#, key);
    let start = s.find(search.as_str())?;
    let after_key = &s[start + search.len()..];
    let after_key = after_key.trim_start();

    let num_str: String = after_key.chars().take_while(|c| c.is_ascii_digit()).collect();
    if num_str.is_empty() {
        return None;
    }
    num_str.parse::<u64>().ok()
}

/// 解析 JSON 中的 `expires` 对象。
fn parse_expires_json(s: &str) -> CookieExpires {
    // 查找 "expires":
    let search = r#""expires":"#;
    let start = match s.find(search) {
        Some(idx) => idx,
        None => return CookieExpires::Session,
    };
    let after_key = &s[start + search.len()..].trim_start();

    if !after_key.starts_with('{') {
        return CookieExpires::Session;
    }

    // 找到匹配的 '}'
    let mut depth = 0u32;
    let mut in_string = false;
    let mut escape = false;
    let mut end = 0;
    for (i, ch) in after_key.char_indices() {
        if escape {
            escape = false;
            continue;
        }
        if ch == '\\' && in_string {
            escape = true;
            continue;
        }
        if ch == '"' {
            in_string = !in_string;
            continue;
        }
        if in_string {
            continue;
        }
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    end = i + 1;
                    break;
                }
            }
            _ => {}
        }
    }

    if end == 0 {
        return CookieExpires::Session;
    }

    let obj = &after_key[..end];
    let inner = &obj[1..obj.len() - 1];

    let type_str = extract_json_str(inner, "type").unwrap_or_default();

    match type_str.as_str() {
        "at" => {
            if let Some(ts_ms) = extract_json_u64(inner, "timestamp_ms") {
                let dur = Duration::from_millis(ts_ms);
                CookieExpires::At(UNIX_EPOCH + dur)
            } else {
                CookieExpires::Session
            }
        }
        "max_age" => {
            if let Some(secs) = extract_json_u64(inner, "secs") {
                CookieExpires::MaxAge(Duration::from_secs(secs))
            } else {
                CookieExpires::Session
            }
        }
        _ => CookieExpires::Session,
    }
}

/// JSON 字符串反转义（仅处理必要的转义序列）。
fn json_unescape(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars();

    while let Some(ch) = chars.next() {
        if ch == '\\' {
            match chars.next() {
                Some('"') => result.push('"'),
                Some('\\') => result.push('\\'),
                Some('/') => result.push('/'),
                Some('n') => result.push('\n'),
                Some('r') => result.push('\r'),
                Some('t') => result.push('\t'),
                Some('u') => {
                    // 简单的 \uXXXX 支持
                    let hex: String = chars.by_ref().take(4).collect();
                    if let Ok(code) = u32::from_str_radix(&hex, 16) {
                        if let Some(c) = char::from_u32(code) {
                            result.push(c);
                        }
                    }
                }
                Some(c) => result.push(c),
                None => result.push('\\'),
            }
        } else {
            result.push(ch);
        }
    }

    result
}

// ---------------------------------------------------------------------------
// 测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn test_domain_match_exact() {
        assert!(CookieJar::domain_match("example.com", "example.com"));
        assert!(CookieJar::domain_match("EXAMPLE.COM", "example.com"));
        assert!(CookieJar::domain_match("example.com", "EXAMPLE.COM"));
    }

    #[test]
    fn test_domain_match_subdomain() {
        assert!(CookieJar::domain_match("sub.example.com", "example.com"));
        assert!(CookieJar::domain_match("a.b.example.com", "example.com"));
        assert!(CookieJar::domain_match("deep.sub.example.com", "example.com"));
    }

    #[test]
    fn test_domain_match_no_match() {
        assert!(!CookieJar::domain_match("example.com", "other.com"));
        assert!(!CookieJar::domain_match("notexample.com", "example.com"));
        assert!(!CookieJar::domain_match("example.com", "notexample.com"));
    }

    #[test]
    fn test_path_match_exact() {
        assert!(CookieJar::path_match("/", "/"));
        assert!(CookieJar::path_match("/foo", "/foo"));
        assert!(CookieJar::path_match("/foo/bar", "/foo/bar"));
    }

    #[test]
    fn test_path_match_prefix() {
        assert!(CookieJar::path_match("/foo/bar", "/foo"));
        assert!(CookieJar::path_match("/foo/bar/baz", "/foo"));
        assert!(CookieJar::path_match("/foo/", "/foo"));
    }

    #[test]
    fn test_path_match_no_match() {
        assert!(!CookieJar::path_match("/foobar", "/foo"));
        assert!(!CookieJar::path_match("/bar/foo", "/foo"));
        assert!(!CookieJar::path_match("/fo", "/foo"));
    }

    #[test]
    fn test_path_match_root() {
        assert!(CookieJar::path_match("/anything", "/"));
        assert!(CookieJar::path_match("/", "/"));
    }

    #[test]
    fn test_cookie_creation() {
        let cookie = Cookie::new(
            "session_id".to_string(),
            "abc123".to_string(),
            "example.com".to_string(),
            "/".to_string(),
        );
        assert_eq!(cookie.name, "session_id");
        assert_eq!(cookie.value, "abc123");
        assert_eq!(cookie.domain, "example.com");
        assert_eq!(cookie.path, "/");
        assert_eq!(cookie.expires, CookieExpires::Session);
        assert!(!cookie.secure);
        assert!(!cookie.http_only);
    }

    #[test]
    fn test_cookie_display() {
        let cookie = Cookie::new(
            "test".to_string(),
            "value".to_string(),
            "example.com".to_string(),
            "/".to_string(),
        );
        let display = format!("{}", cookie);
        assert!(display.contains("test=value"));
        assert!(display.contains("Domain=example.com"));
        assert!(display.contains("Path=/"));
    }

    #[test]
    fn test_cookie_expiry() {
        let mut cookie = Cookie::new(
            "test".to_string(),
            "value".to_string(),
            "example.com".to_string(),
            "/".to_string(),
        );

        // 未过期
        assert!(!cookie.is_expired());

        // 设置为过去的时间
        let past = UNIX_EPOCH + Duration::from_secs(1000);
        cookie.expires = CookieExpires::At(past);
        assert!(cookie.is_expired());

        // 设置为将来的时间
        let future = SystemTime::now() + Duration::from_secs(3600);
        cookie.expires = CookieExpires::At(future);
        assert!(!cookie.is_expired());
    }

    #[test]
    fn test_jar_insert_and_len() {
        let mut jar = CookieJar::new();
        assert!(jar.is_empty());

        let cookie = Cookie::new(
            "a".to_string(),
            "1".to_string(),
            "example.com".to_string(),
            "/".to_string(),
        );
        jar.insert(cookie);
        assert_eq!(jar.len(), 1);
        assert!(!jar.is_empty());
    }

    #[test]
    fn test_jar_insert_replace() {
        let mut jar = CookieJar::new();

        jar.insert(Cookie::new(
            "a".to_string(),
            "1".to_string(),
            "example.com".to_string(),
            "/".to_string(),
        ));
        jar.insert(Cookie::new(
            "a".to_string(),
            "2".to_string(),
            "example.com".to_string(),
            "/".to_string(),
        ));
        assert_eq!(jar.len(), 1);

        // 不同的路径不会替换
        jar.insert(Cookie::new(
            "a".to_string(),
            "3".to_string(),
            "example.com".to_string(),
            "/foo".to_string(),
        ));
        assert_eq!(jar.len(), 2);
    }

    #[test]
    fn test_jar_clear_all() {
        let mut jar = CookieJar::new();
        jar.insert(Cookie::new(
            "a".to_string(),
            "1".to_string(),
            "example.com".to_string(),
            "/".to_string(),
        ));
        jar.insert(Cookie::new(
            "b".to_string(),
            "2".to_string(),
            "other.com".to_string(),
            "/".to_string(),
        ));
        assert_eq!(jar.len(), 2);
        jar.clear_all();
        assert!(jar.is_empty());
    }

    #[test]
    fn test_jar_clear_for_origin() {
        let mut jar = CookieJar::new();
        jar.insert(Cookie::new(
            "a".to_string(),
            "1".to_string(),
            "example.com".to_string(),
            "/".to_string(),
        ));
        jar.insert(Cookie::new(
            "b".to_string(),
            "2".to_string(),
            "sub.example.com".to_string(),
            "/".to_string(),
        ));
        jar.insert(Cookie::new(
            "c".to_string(),
            "3".to_string(),
            "other.com".to_string(),
            "/".to_string(),
        ));
        assert_eq!(jar.len(), 3);

        jar.clear_for_origin("example.com");
        assert_eq!(jar.len(), 1); // only other.com remains
    }

    #[test]
    fn test_remove_expired() {
        let mut jar = CookieJar::new();

        // 一个永不过期的会话 cookie
        jar.insert(Cookie::new(
            "session".to_string(),
            "1".to_string(),
            "example.com".to_string(),
            "/".to_string(),
        ));

        // 一个已过期的 cookie
        let mut expired = Cookie::new(
            "expired".to_string(),
            "2".to_string(),
            "example.com".to_string(),
            "/".to_string(),
        );
        let past = UNIX_EPOCH + Duration::from_secs(1000);
        expired.expires = CookieExpires::At(past);
        jar.insert(expired);

        assert_eq!(jar.len(), 2);
        jar.remove_expired();
        assert_eq!(jar.len(), 1);
    }

    #[test]
    fn test_parse_set_cookie_basic() {
        let url = Url::parse("http://example.com/path").unwrap();
        let cookie = CookieJar::parse_set_cookie("test=value", &url);
        assert!(cookie.is_some());
        let cookie = cookie.unwrap();
        assert_eq!(cookie.name, "test");
        assert_eq!(cookie.value, "value");
        assert_eq!(cookie.domain, "example.com");
        assert_eq!(cookie.path, "/");
    }

    #[test]
    fn test_parse_set_cookie_full() {
        let url = Url::parse("http://example.com/path/page").unwrap();
        let cookie = CookieJar::parse_set_cookie(
            "session=abc123; Domain=example.com; Path=/path; Secure; HttpOnly; SameSite=Strict",
            &url,
        );
        assert!(cookie.is_some());
        let cookie = cookie.unwrap();
        assert_eq!(cookie.name, "session");
        assert_eq!(cookie.value, "abc123");
        assert_eq!(cookie.domain, "example.com");
        assert_eq!(cookie.path, "/path");
        assert!(cookie.secure);
        assert!(cookie.http_only);
        assert_eq!(cookie.same_site, SameSite::Strict);
    }

    #[test]
    fn test_parse_set_cookie_expires() {
        let url = Url::parse("http://example.com/").unwrap();
        // "Wed, 21 Oct 2025 07:28:00 GMT"
        let cookie = CookieJar::parse_set_cookie(
            "test=value; Expires=Wed, 21 Oct 2025 07:28:00 GMT",
            &url,
        );
        assert!(cookie.is_some());
        let cookie = cookie.unwrap();
        match &cookie.expires {
            CookieExpires::At(time) => {
                let dur = time.duration_since(UNIX_EPOCH).unwrap();
                // 2025-10-21 07:28:00 UTC = some large timestamp
                assert!(dur.as_secs() > 1700000000);
            }
            _ => panic!("Expected CookieExpires::At"),
        }
    }

    #[test]
    fn test_parse_set_cookie_max_age() {
        let url = Url::parse("http://example.com/").unwrap();
        let cookie = CookieJar::parse_set_cookie("test=value; Max-Age=3600", &url);
        assert!(cookie.is_some());
        let cookie = cookie.unwrap();
        match &cookie.expires {
            CookieExpires::MaxAge(age) => {
                assert_eq!(*age, Duration::from_secs(3600));
            }
            _ => panic!("Expected CookieExpires::MaxAge"),
        }
    }

    #[test]
    fn test_cookies_for_url() {
        let mut jar = CookieJar::new();
        let url = Url::parse("http://example.com/foo/bar").unwrap();

        jar.insert(Cookie::new(
            "a".to_string(),
            "1".to_string(),
            "example.com".to_string(),
            "/".to_string(),
        ));
        jar.insert(Cookie::new(
            "b".to_string(),
            "2".to_string(),
            "example.com".to_string(),
            "/foo".to_string(),
        ));
        jar.insert(Cookie::new(
            "c".to_string(),
            "3".to_string(),
            "other.com".to_string(),
            "/".to_string(),
        ));

        let header = jar.cookies_for_url(&url);
        assert!(header.contains("a=1"));
        assert!(header.contains("b=2"));
        assert!(!header.contains("c=3"));
    }

    #[test]
    fn test_cookies_for_url_secure() {
        let mut jar = CookieJar::new();

        let mut secure_cookie = Cookie::new(
            "secure_test".to_string(),
            "val".to_string(),
            "example.com".to_string(),
            "/".to_string(),
        );
        secure_cookie.secure = true;
        jar.insert(secure_cookie);
        jar.insert(Cookie::new(
            "normal".to_string(),
            "val".to_string(),
            "example.com".to_string(),
            "/".to_string(),
        ));

        // HTTP 请求不应包含 secure cookie
        let http_url = Url::parse("http://example.com/").unwrap();
        let header = jar.cookies_for_url(&http_url);
        assert!(!header.contains("secure_test"));
        assert!(header.contains("normal"));

        // HTTPS 请求应包含 secure cookie
        let https_url = Url::parse("https://example.com/").unwrap();
        let header = jar.cookies_for_url(&https_url);
        assert!(header.contains("secure_test"));
    }

    #[test]
    fn test_cookies_for_url_path_sorting() {
        let mut jar = CookieJar::new();

        // 创建不同路径的 cookie
        jar.insert(Cookie::new(
            "root".to_string(),
            "1".to_string(),
            "example.com".to_string(),
            "/".to_string(),
        ));
        std::thread::sleep(std::time::Duration::from_millis(10));
        jar.insert(Cookie::new(
            "specific".to_string(),
            "2".to_string(),
            "example.com".to_string(),
            "/foo/bar".to_string(),
        ));

        let url = Url::parse("http://example.com/foo/bar/baz").unwrap();
        let header = jar.cookies_for_url(&url);
        // specific 的路径更长，应该排在前面
        let parts: Vec<&str> = header.split("; ").collect();
        assert_eq!(parts[0], "specific=2");
    }

    #[test]
    fn test_all_cookies() {
        let mut jar = CookieJar::new();
        jar.insert(Cookie::new(
            "a".to_string(),
            "1".to_string(),
            "a.com".to_string(),
            "/".to_string(),
        ));
        jar.insert(Cookie::new(
            "b".to_string(),
            "2".to_string(),
            "b.com".to_string(),
            "/".to_string(),
        ));
        assert_eq!(jar.all_cookies().len(), 2);
    }

    #[test]
    fn test_json_roundtrip() {
        let mut jar = CookieJar::new();

        let mut cookie = Cookie::new(
            "session".to_string(),
            "abc123".to_string(),
            "example.com".to_string(),
            "/".to_string(),
        );
        cookie.secure = true;
        cookie.http_only = true;
        cookie.same_site = SameSite::Strict;
        jar.insert(cookie);

        let mut max_age_cookie = Cookie::new(
            "persistent".to_string(),
            "xyz".to_string(),
            "example.com".to_string(),
            "/app".to_string(),
        );
        max_age_cookie.expires = CookieExpires::MaxAge(Duration::from_secs(86400));
        jar.insert(max_age_cookie);

        let json = jar.to_json();
        let jar2 = CookieJar::from_json(&json);

        assert_eq!(jar.len(), jar2.len());

        // Compare cookies
        let all1 = jar.all_cookies();
        let all2 = jar2.all_cookies();
        for (c1, c2) in all1.iter().zip(all2.iter()) {
            assert_eq!(c1.name, c2.name);
            assert_eq!(c1.value, c2.value);
            assert_eq!(c1.domain, c2.domain);
            assert_eq!(c1.path, c2.path);
            assert_eq!(c1.secure, c2.secure);
            assert_eq!(c1.http_only, c2.http_only);
        }
    }

    #[test]
    fn test_file_save_load() {
        let dir = std::env::temp_dir().join("cookie_test");
        let path = dir.join("cookies.json");

        let mut jar = CookieJar::new();
        jar.insert(Cookie::new(
            "test".to_string(),
            "value".to_string(),
            "example.com".to_string(),
            "/".to_string(),
        ));

        assert!(jar.save_to_file(&path).is_ok());

        let loaded = CookieJar::load_from_file(&path);
        assert_eq!(jar.len(), loaded.len());

        // 清理
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_dir(&dir);
    }

    #[test]
    fn test_empty_json() {
        let jar = CookieJar::new();
        let json = jar.to_json();
        assert_eq!(json, "{\"cookies\":[]}");

        let jar2 = CookieJar::from_json(&json);
        assert!(jar2.is_empty());
    }

    #[test]
    fn test_parse_rfc1123() {
        // "Wed, 21 Oct 2015 07:28:00 GMT" = 1445412480
        let ts = parse_rfc1123("Wed, 21 Oct 2015 07:28:00 GMT");
        assert!(ts.is_some());
        assert_eq!(ts.unwrap(), 1445412480);
    }

    #[test]
    fn test_rfc1123_roundtrip() {
        let ts = 1445412480;
        let formatted = rfc1123_from_unix(ts);
        assert!(formatted.is_some());
        let parsed = parse_rfc1123(&formatted.unwrap());
        assert_eq!(parsed, Some(ts));
    }

    #[test]
    fn test_domain_match_trailing_dots() {
        assert!(CookieJar::domain_match("example.com.", "example.com"));
        assert!(CookieJar::domain_match("example.com", ".example.com"));
        assert!(CookieJar::domain_match(".example.com.", "example.com."));
    }

    #[test]
    fn test_path_match_edge_cases() {
        // Root path matches everything
        assert!(CookieJar::path_match("/anything/at/all", "/"));
        // Self matching
        assert!(CookieJar::path_match("/foo/bar", "/foo/bar"));
        // Prefix with slash boundary
        assert!(CookieJar::path_match("/foo/bar", "/foo"));
        // Not a proper prefix (no slash boundary)
        assert!(!CookieJar::path_match("/foobar", "/foo"));
        // Empty path (should not normally happen)
        assert!(!CookieJar::path_match("/foo", ""));
    }
}
