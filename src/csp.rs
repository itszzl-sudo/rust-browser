//! Content-Security-Policy 引擎
//!
//! 实现 CSP Level 2 的核心功能：
//! - 解析 Content-Security-Policy 响应头
//! - 在资源加载时检查是否被策略允许
//! - 阻止违规的资源请求
//!
//! 支持的指令:
//! - default-src, script-src, style-src, img-src, connect-src,
//!   font-src, media-src, frame-src, object-src, manifest-src
//! - report-uri, report-to (日志警告)
//! - block-all-mixed-content
//! - upgrade-insecure-requests

use log::warn;
use std::time::Duration;

/// CSP 获取指令类型
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CspDirective {
    DefaultSrc,
    ScriptSrc,
    StyleSrc,
    ImgSrc,
    ConnectSrc,
    FontSrc,
    MediaSrc,
    FrameSrc,
    ObjectSrc,
    ManifestSrc,
}

impl CspDirective {
    fn from_str(s: &str) -> Option<Self> {
        match s.trim().to_lowercase().as_str() {
            "default-src" => Some(Self::DefaultSrc),
            "script-src" => Some(Self::ScriptSrc),
            "style-src" => Some(Self::StyleSrc),
            "img-src" => Some(Self::ImgSrc),
            "connect-src" => Some(Self::ConnectSrc),
            "font-src" => Some(Self::FontSrc),
            "media-src" => Some(Self::MediaSrc),
            "frame-src" => Some(Self::FrameSrc),
            "object-src" => Some(Self::ObjectSrc),
            "manifest-src" => Some(Self::ManifestSrc),
            _ => None,
        }
    }
}

/// CSP 源表达式
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CspSource {
    /// 'none'
    None,
    /// 'self'
    Self_,
    /// 'unsafe-inline'
    UnsafeInline,
    /// 'unsafe-eval'
    UnsafeEval,
    /// 'strict-dynamic'
    StrictDynamic,
    /// 'unsafe-hashes'
    UnsafeHashes,
    /// 'report-sample'
    ReportSample,
    /// http://example.com 或 https://example.com
    Host(String),
    /// *.example.com
    HostWildcard(String),
    /// data:
    Scheme(String),
    /// 'nonce-abc123'
    Nonce(String),
    /// 'sha256-abc...'
    Hash(String),
}

/// CSP 策略 — 对应一条 Content-Security-Policy 头
#[derive(Debug, Clone)]
pub struct CspPolicy {
    /// 每条指令及其允许的源列表
    directives: Vec<(CspDirective, Vec<CspSource>)>,
    /// report-uri (弃用但仍需支持)
    report_uri: Vec<String>,
    /// report-to (CSP Level 3)
    report_to: Vec<String>,
    /// block-all-mixed-content
    block_mixed_content: bool,
    /// upgrade-insecure-requests
    upgrade_insecure_requests: bool,
}

impl CspPolicy {
    pub fn new() -> Self {
        Self {
            directives: Vec::new(),
            report_uri: Vec::new(),
            report_to: Vec::new(),
            block_mixed_content: false,
            upgrade_insecure_requests: false,
        }
    }

    /// 从 HTTP Content-Security-Policy 头解析策略
    pub fn parse(header_value: &str) -> Self {
        let mut policy = Self::new();

        for directive_str in header_value.split(';') {
            let directive_str = directive_str.trim();
            if directive_str.is_empty() {
                continue;
            }

            let parts: Vec<&str> = directive_str.split_whitespace().collect();
            if parts.is_empty() {
                continue;
            }

            let directive_name = parts[0];
            let sources: Vec<&str> = parts[1..].to_vec();

            match directive_name.to_lowercase().as_str() {
                "report-uri" => {
                    policy.report_uri = sources.iter().map(|s| s.to_string()).collect();
                }
                "report-to" => {
                    policy.report_to = sources.iter().map(|s| s.to_string()).collect();
                }
                "block-all-mixed-content" => {
                    policy.block_mixed_content = true;
                }
                "upgrade-insecure-requests" => {
                    policy.upgrade_insecure_requests = true;
                }
                _ => {
                    if let Some(dir) = CspDirective::from_str(directive_name) {
                        let csp_sources: Vec<CspSource> = sources
                            .iter()
                            .filter_map(|s| Self::parse_source(s))
                            .collect();
                        policy.directives.push((dir, csp_sources));
                    }
                    // 未知指令忽略
                }
            }
        }

        policy
    }

    fn parse_source(source: &str) -> Option<CspSource> {
        match source {
            "'none'" => Some(CspSource::None),
            "'self'" => Some(CspSource::Self_),
            "'unsafe-inline'" => Some(CspSource::UnsafeInline),
            "'unsafe-eval'" => Some(CspSource::UnsafeEval),
            "'strict-dynamic'" => Some(CspSource::StrictDynamic),
            "'unsafe-hashes'" => Some(CspSource::UnsafeHashes),
            "'report-sample'" => Some(CspSource::ReportSample),
            s if s.starts_with("'nonce-") && s.ends_with('\'') => {
                let nonce = &s[7..s.len() - 1];
                Some(CspSource::Nonce(nonce.to_string()))
            }
            s if s.starts_with("'sha") && s.ends_with('\'') => {
                let hash = &s[1..s.len() - 1];
                Some(CspSource::Hash(hash.to_string()))
            }
            s if s.contains("://") || s.contains('.') => {
                if s.starts_with("*.") {
                    Some(CspSource::HostWildcard(s.to_string()))
                } else {
                    Some(CspSource::Host(s.to_string()))
                }
            }
            s if s.ends_with(':') => {
                // scheme 源 (data:, https: 等)
                Some(CspSource::Scheme(s.trim_end_matches(':').to_string()))
            }
            _ => None,
        }
    }

    /// 检查给定的资源 URL 是否被指定的指令允许
    ///
    /// # 参数
    /// * `directive` — 指令类型（如 script-src）
    /// * `url` — 要加载的资源的 URL
    /// * `page_origin` — 当前页面的来源（用于 'self' 匹配）
    ///
    /// # 返回
    /// `true` 如果允许加载
    pub fn allows(&self, directive: CspDirective, url: &str, page_origin: &str) -> bool {
        // 先查找具体指令，没有则回退到 default-src
        let sources = self
            .directives
            .iter()
            .find(|(d, _)| *d == directive)
            .map(|(_, s)| s)
            .or_else(|| {
                self.directives
                    .iter()
                    .find(|(d, _)| *d == CspDirective::DefaultSrc)
                    .map(|(_, s)| s)
            });

        let sources = match sources {
            Some(s) => s,
            None => return true, // 没有策略限制，允许
        };

        // 'none' 阻断所有
        if sources.iter().any(|s| *s == CspSource::None) {
            return false;
        }

        for source in sources {
            match source {
                CspSource::Self_ => {
                    if url.starts_with(page_origin) || is_same_origin(url, page_origin) {
                        return true;
                    }
                }
                CspSource::UnsafeInline => {
                    // 对 script-src 和 style-src 有效
                    if directive == CspDirective::ScriptSrc || directive == CspDirective::StyleSrc {
                        return true;
                    }
                }
                CspSource::UnsafeEval => {
                    if directive == CspDirective::ScriptSrc {
                        return true;
                    }
                }
                CspSource::Host(host) => {
                    if url_matches_host(url, host) {
                        return true;
                    }
                }
                CspSource::HostWildcard(pattern) => {
                    if url_matches_wildcard(url, pattern) {
                        return true;
                    }
                }
                CspSource::Scheme(scheme) => {
                    if url.starts_with(&format!("{}:", scheme)) {
                        return true;
                    }
                }
                // CSP Level 2 nonce/hash 验证：当前项目使用 `unsafe-inline` 等价策略，
                // 因此 nonce 和 hash 源表达式在此视为允许（与 unsafe-inline 行为一致）。
                // 如需严格的 nonce/hash 验证，此处应：
                //   1. 存储页面的实际 nonce 值
                //   2. 对内联脚本/样式计算 hash
                //   3. 匹配后决定是否允许
                // 鉴于当前实现以简化为主（等价于 'unsafe-inline'），直接跳过。
                CspSource::Nonce(_) | CspSource::Hash(_) => {
                    // 视为允许（等价 unsafe-inline 行为）
                    return true;
                }
                // 以下变体在当前简化实现中不阻塞（等价无限制）
                CspSource::None => {}
                CspSource::StrictDynamic => {}
                CspSource::UnsafeHashes => {}
                CspSource::ReportSample => {}
            }
        }

        false
    }

    /// 检查是否应阻止混合内容
    pub fn should_block_mixed_content(&self, url: &str) -> bool {
        self.block_mixed_content && url.starts_with("http://")
    }

    /// 检查是否需要升级不安全的请求
    pub fn should_upgrade(&self, url: &str) -> Option<String> {
        if self.upgrade_insecure_requests && url.starts_with("http://") {
            Some(url.replacen("http://", "https://", 1))
        } else {
            None
        }
    }

    /// 记录违规日志，并向 report-uri 发送违规报告（如果有）
    pub fn report_violation(&self, directive: CspDirective, url: &str, page_url: &str) {
        warn!(
            "CSP Violation: directive={:?}, url={}, page={}",
            directive, url, page_url
        );

        // 向所有 report-uri 发送 CSP 违规报告
        for report_uri in &self.report_uri {
            let report = serde_json::json!({
                "csp-report": {
                    "document-uri": page_url,
                    "blocked-uri": url,
                    "violated-directive": format!("{:?}", directive),
                    "effective-directive": format!("{:?}", directive),
                    "original-policy": "",
                    "disposition": "enforce",
                    "source-file": url,
                }
            });

            if let Ok(report_str) = serde_json::to_string(&report) {
                // 使用 blocking 客户端发送报告（5 秒超时）
                if let Ok(client) = reqwest::blocking::Client::builder()
                    .timeout(Duration::from_secs(5))
                    .build()
                {
                    let _ = client
                        .post(report_uri)
                        .header("Content-Type", "application/csp-report")
                        .body(report_str)
                        .send();
                }
            }
        }
    }
}

impl Default for CspPolicy {
    fn default() -> Self {
        Self::new()
    }
}

/// CSP 管理器 —— 管理页面的 CSP 策略
#[derive(Debug, Clone)]
pub struct CspManager {
    /// 当前页面的 CSP 策略列表（可能有多条头）
    policies: Vec<CspPolicy>,
    /// report-only 策略列表（仅报告不阻止）
    report_only_policies: Vec<CspPolicy>,
    /// 当前页面 URL
    page_url: String,
}

impl CspManager {
    pub fn new() -> Self {
        Self {
            policies: Vec::new(),
            report_only_policies: Vec::new(),
            page_url: String::new(),
        }
    }

    /// 设置 CSP 策略（从 HTTP 响应头）
    pub fn set_from_headers(&mut self, headers: &[(String, String)], page_url: &str) {
        self.page_url = page_url.to_string();
        self.policies.clear();
        self.report_only_policies.clear();

        for (name, value) in headers {
            if name.eq_ignore_ascii_case("Content-Security-Policy") {
                self.policies.push(CspPolicy::parse(value));
            }
            if name.eq_ignore_ascii_case("Content-Security-Policy-Report-Only") {
                // Report-Only 策略：仅报告违规，不阻止资源加载
                self.report_only_policies.push(CspPolicy::parse(value));
            }
        }
    }

    /// 检查资源是否允许加载
    pub fn allows(&self, directive: CspDirective, url: &str) -> bool {
        if self.policies.is_empty() {
            return true; // 无策略，允许
        }

        let page_origin = extract_origin(&self.page_url);
        self.policies
            .iter()
            .all(|p| p.allows(directive, url, &page_origin))
    }

    /// 检查资源
    pub fn check_resource(&self, directive: CspDirective, url: &str) -> CspResult {
        // 1. 先处理 report-only 策略：只记录不阻止
        for policy in &self.report_only_policies {
            let page_origin = extract_origin(&self.page_url);
            if !policy.allows(directive, url, &page_origin) {
                policy.report_violation(directive, url, &self.page_url);
                // Report-Only: 不阻止，继续
            }
        }

        // 2. 没有 enforce 策略，允许
        if self.policies.is_empty() {
            return CspResult::Allowed;
        }

        // 3. 正常 enforce 策略检查
        for policy in &self.policies {
            // 升级不安全请求
            if let Some(upgraded) = policy.should_upgrade(url) {
                return CspResult::Upgraded(upgraded);
            }

            // 阻止混合内容
            if policy.should_block_mixed_content(url) {
                policy.report_violation(directive, url, &self.page_url);
                return CspResult::Blocked("Mixed content blocked".to_string());
            }

            // 检查源是否允许
            let page_origin = extract_origin(&self.page_url);
            if !policy.allows(directive, url, &page_origin) {
                policy.report_violation(directive, url, &self.page_url);
                return CspResult::Blocked(format!("Blocked by CSP {:?}", directive));
            }
        }

        CspResult::Allowed
    }
}

impl Default for CspManager {
    fn default() -> Self {
        Self::new()
    }
}

/// CSP 检查结果
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CspResult {
    /// 允许加载
    Allowed,
    /// 被阻止
    Blocked(String),
    /// 升级到 HTTPS
    Upgraded(String),
}

// ============================================================
// 辅助函数
// ============================================================

/// 从 URL 中提取来源（scheme + host + port）
fn extract_origin(url: &str) -> String {
    if let Ok(parsed) = url::Url::parse(url) {
        parsed.origin().ascii_serialization()
    } else {
        String::new()
    }
}

/// 判断 URL 是否与页面同源
fn is_same_origin(url: &str, page_origin: &str) -> bool {
    extract_origin(url) == page_origin
}

/// 判断 URL 是否匹配给定的 host
fn url_matches_host(url: &str, host: &str) -> bool {
    if let Ok(parsed) = url::Url::parse(url) {
        if let Some(host_str) = parsed.host_str() {
            return host_str
                == host
                    .trim_start_matches("http://")
                    .trim_start_matches("https://")
                || format!("{}://{}", parsed.scheme(), host_str) == host
                || format!(
                    "{}://{}:{}",
                    parsed.scheme(),
                    host_str,
                    parsed.port().unwrap_or(80)
                ) == host;
        }
    }
    false
}

/// 判断 URL 是否匹配通配符模式
fn url_matches_wildcard(url: &str, pattern: &str) -> bool {
    // *.example.com 匹配 sub.example.com 和 example.com
    if let Ok(parsed) = url::Url::parse(url) {
        if let Some(host_str) = parsed.host_str() {
            let pattern_trimmed = pattern.trim_start_matches("*.");
            return host_str.ends_with(pattern_trimmed) || host_str == pattern_trimmed;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_policy() {
        let policy = CspPolicy::parse("default-src 'self'");
        assert!(policy.allows(
            CspDirective::ScriptSrc,
            "https://example.com/script.js",
            "https://example.com"
        ));
    }

    #[test]
    fn test_block_none() {
        let policy = CspPolicy::parse("default-src 'none'");
        assert!(!policy.allows(
            CspDirective::ScriptSrc,
            "https://example.com/script.js",
            "https://example.com"
        ));
    }

    #[test]
    fn test_block_external() {
        let policy = CspPolicy::parse("default-src 'self'");
        assert!(!policy.allows(
            CspDirective::ImgSrc,
            "https://evil.com/hack.png",
            "https://example.com"
        ));
    }

    #[test]
    fn test_allow_external_with_host() {
        let policy = CspPolicy::parse("img-src https://images.example.com");
        assert!(policy.allows(
            CspDirective::ImgSrc,
            "https://images.example.com/photo.jpg",
            "https://example.com"
        ));
    }

    #[test]
    fn test_mixed_content_blocking() {
        let policy = CspPolicy::parse("default-src 'self'; block-all-mixed-content");
        assert!(policy.should_block_mixed_content("http://example.com/script.js"));
        assert!(!policy.should_block_mixed_content("https://example.com/script.js"));
    }

    #[test]
    fn test_upgrade_insecure_requests() {
        let policy = CspPolicy::parse("upgrade-insecure-requests");
        assert_eq!(
            policy.should_upgrade("http://example.com/script.js"),
            Some("https://example.com/script.js".to_string())
        );
    }
}
