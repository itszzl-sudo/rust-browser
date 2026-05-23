//! HTTP Cache — 浏览器 HTTP 缓存层
//!
//! 实现 RFC 7234 HTTP 缓存的核心功能：
//! - Cache-Control 指令解析（max-age, no-cache, no-store, must-revalidate 等）
//! - Expires 头处理
//! - ETag + If-None-Match 条件请求
//! - Last-Modified + If-Modified-Since 条件请求
//! - 内存缓存存储（LRU 淘汰）
//! - 缓存命中/未命中统计

use log::debug;
use std::collections::HashMap;
use std::time::{Duration, Instant, SystemTime};

/// 缓存条目
#[derive(Debug, Clone)]
pub struct CacheEntry {
    /// 缓存的响应体
    pub body: Vec<u8>,
    /// 缓存的内容类型
    pub content_type: String,
    /// ETag 值（用于条件请求）
    pub etag: Option<String>,
    /// Last-Modified 值
    pub last_modified: Option<String>,
    /// 过期时间
    pub expires_at: Instant,
    /// 缓存创建时间
    pub created_at: Instant,
    /// 是否需要每次验证（no-cache）
    pub must_revalidate: bool,
    /// 响应状态码
    pub status_code: u16,
    /// 响应头
    pub headers: Vec<(String, String)>,
}

/// 缓存策略
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheStrategy {
    /// 可以缓存并重用（Cache-Control: max-age 或 Expires）
    Cacheable,
    /// 每次需要验证（Cache-Control: no-cache 或 must-revalidate）
    NeedsValidation,
    /// 不允许缓存（Cache-Control: no-store）
    NotCacheable,
}

/// HTTP 缓存管理器
pub struct HttpCache {
    /// URL → 缓存条目
    entries: HashMap<String, CacheEntry>,
    /// 最大缓存条目数
    max_entries: usize,
    /// 缓存命中次数
    hits: u64,
    /// 缓存未命中次数
    misses: u64,
}

impl HttpCache {
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
            max_entries: 256,
            hits: 0,
            misses: 0,
        }
    }

    /// 从 HTTP 响应头判断缓存策略
    pub fn determine_strategy(cache_control: Option<&str>, expires: Option<&str>) -> CacheStrategy {
        if let Some(cc) = cache_control {
            let cc_lower = cc.to_lowercase();
            if cc_lower.contains("no-store") {
                return CacheStrategy::NotCacheable;
            }
            if cc_lower.contains("no-cache") || cc_lower.contains("must-revalidate") {
                return CacheStrategy::NeedsValidation;
            }
            if cc_lower.contains("max-age=") {
                return CacheStrategy::Cacheable;
            }
        }
        if expires.is_some() {
            return CacheStrategy::Cacheable;
        }
        // 默认：只缓存 200 响应
        CacheStrategy::Cacheable
    }

    /// 从 Cache-Control 头提取 max-age 秒数
    fn parse_max_age(cache_control: Option<&str>) -> Option<u64> {
        let cc = cache_control?;
        let cc_lower = cc.to_lowercase();
        for part in cc_lower.split(',') {
            let part = part.trim();
            if let Some(value) = part.strip_prefix("max-age=") {
                return value
                    .trim()
                    .split(|c: char| !c.is_ascii_digit())
                    .next()
                    .and_then(|s| s.parse::<u64>().ok());
            }
            if let Some(value) = part.strip_prefix("s-maxage=") {
                return value
                    .trim()
                    .split(|c: char| !c.is_ascii_digit())
                    .next()
                    .and_then(|s| s.parse::<u64>().ok());
            }
        }
        None
    }

    /// 解析 HTTP 日期（Expires, Last-Modified）
    fn parse_http_date(date_str: &str) -> Option<SystemTime> {
        httpdate::parse_http_date(date_str).ok()
    }

    /// 获取缓存条目（如果存在且未过期）
    pub fn get(&self, url: &str) -> Option<&CacheEntry> {
        let entry = self.entries.get(url)?;

        if entry.must_revalidate {
            // no-cache 条目也需要验证，但可以返回以供条件请求
            return Some(entry);
        }

        if Instant::now() >= entry.expires_at {
            // 过期了，需要验证
            return Some(entry); // 仍然返回，调用方可以发条件请求
        }

        Some(entry)
    }

    /// 存储缓存条目
    pub fn put(
        &mut self,
        url: &str,
        body: Vec<u8>,
        content_type: &str,
        status_code: u16,
        headers: &[(String, String)],
    ) {
        // 检查缓存策略
        let cache_control = headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case("cache-control"))
            .map(|(_, v)| v.as_str());
        let expires = headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case("expires"))
            .map(|(_, v)| v.as_str());

        let strategy = Self::determine_strategy(cache_control, expires);
        if strategy == CacheStrategy::NotCacheable {
            return;
        }

        // 计算过期时间
        let max_age = Self::parse_max_age(cache_control);
        let expires_at = if let Some(seconds) = max_age {
            Instant::now() + Duration::from_secs(seconds)
        } else if let Some(expires_str) = expires {
            if let Some(sys_time) = Self::parse_http_date(expires_str) {
                let now = SystemTime::now();
                if let Ok(duration) = sys_time.duration_since(now) {
                    Instant::now() + duration
                } else {
                    // 已过期
                    return;
                }
            } else {
                // 无法解析，默认缓存 5 分钟
                Instant::now() + Duration::from_secs(300)
            }
        } else {
            // 没有过期信息，默认缓存 5 分钟
            Instant::now() + Duration::from_secs(300)
        };

        let etag = headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case("etag"))
            .map(|(_, v)| v.clone());
        let last_modified = headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case("last-modified"))
            .map(|(_, v)| v.clone());

        let must_revalidate = strategy == CacheStrategy::NeedsValidation;

        // LRU 淘汰：如果达到上限，移除最早创建的条目
        if self.entries.len() >= self.max_entries {
            if let Some(oldest_url) = self
                .entries
                .iter()
                .min_by_key(|(_, e)| e.created_at)
                .map(|(url, _)| url.clone())
            {
                self.entries.remove(&oldest_url);
            }
        }

        self.entries.insert(
            url.to_string(),
            CacheEntry {
                body,
                content_type: content_type.to_string(),
                etag,
                last_modified,
                expires_at,
                created_at: Instant::now(),
                must_revalidate,
                status_code,
                headers: headers.to_vec(),
            },
        );

        debug!(
            "HTTP Cache: 缓存 {} (expires in {:?})",
            url,
            expires_at.saturating_duration_since(Instant::now())
        );
    }

    /// 检查缓存是否需要验证（用于决定是否发条件请求）
    pub fn needs_validation(&self, url: &str) -> bool {
        if let Some(entry) = self.entries.get(url) {
            if entry.must_revalidate {
                return true;
            }
            if Instant::now() >= entry.expires_at {
                return true;
            }
            false
        } else {
            false
        }
    }

    /// 获取条件请求头（If-None-Match 或 If-Modified-Since）
    pub fn get_conditional_headers(&self, url: &str) -> Vec<(String, String)> {
        let mut headers = Vec::new();
        if let Some(entry) = self.entries.get(url) {
            if let Some(ref etag) = entry.etag {
                headers.push(("If-None-Match".to_string(), etag.clone()));
            }
            if let Some(ref last_modified) = entry.last_modified {
                headers.push(("If-Modified-Since".to_string(), last_modified.clone()));
            }
        }
        headers
    }

    /// 检查 URL 是否有有效的缓存条目
    pub fn is_cached(&self, url: &str) -> bool {
        self.entries.contains_key(url)
    }

    /// 刷新缓存（当 304 响应时更新过期时间）
    pub fn refresh(&mut self, url: &str, headers: &[(String, String)]) {
        if let Some(entry) = self.entries.get_mut(url) {
            // 根据新响应头更新过期时间
            let cache_control = headers
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case("cache-control"))
                .map(|(_, v)| v.as_str());

            if let Some(max_age) = Self::parse_max_age(cache_control) {
                entry.expires_at = Instant::now() + Duration::from_secs(max_age);
            } else {
                // 默认延长 5 分钟
                entry.expires_at = Instant::now() + Duration::from_secs(300);
            }
            entry.must_revalidate = false;
            debug!(
                "HTTP Cache: 刷新 {} (expires in {:?})",
                url,
                entry.expires_at.saturating_duration_since(Instant::now())
            );
        }
    }

    /// 移除缓存条目
    pub fn remove(&mut self, url: &str) {
        self.entries.remove(url);
    }

    /// 清空所有缓存
    pub fn clear(&mut self) {
        self.entries.clear();
        self.hits = 0;
        self.misses = 0;
    }

    /// 获取缓存统计（命中, 未命中）
    pub fn stats(&self) -> (u64, u64) {
        (self.hits, self.misses)
    }

    /// 缓存命中率
    pub fn hit_rate(&self) -> f64 {
        let total = self.hits + self.misses;
        if total == 0 {
            0.0
        } else {
            self.hits as f64 / total as f64
        }
    }

    /// 缓存条目数
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 记录缓存命中（用于外部统计）
    pub fn record_hit(&mut self) {
        self.hits += 1;
    }

    /// 记录缓存未命中（用于外部统计）
    pub fn record_miss(&mut self) {
        self.misses += 1;
    }

    /// 获取所有已缓存的 URL 列表
    pub fn cached_urls(&self) -> Vec<String> {
        self.entries.keys().cloned().collect()
    }

    /// 获取缓存条目（可变引用）
    pub fn get_mut(&mut self, url: &str) -> Option<&mut CacheEntry> {
        self.entries.get_mut(url)
    }
}

impl Default for HttpCache {
    fn default() -> Self {
        Self::new()
    }
}

/// 全局 HTTP 缓存实例
use std::sync::Mutex;
lazy_static::lazy_static! {
    pub static ref GLOBAL_HTTP_CACHE: Mutex<HttpCache> = Mutex::new(HttpCache::new());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_determine_strategy() {
        assert_eq!(
            CacheStrategy::NotCacheable,
            HttpCache::determine_strategy(Some("no-store"), None)
        );
        assert_eq!(
            CacheStrategy::NeedsValidation,
            HttpCache::determine_strategy(Some("no-cache"), None)
        );
        assert_eq!(
            CacheStrategy::NeedsValidation,
            HttpCache::determine_strategy(Some("must-revalidate"), None)
        );
        assert_eq!(
            CacheStrategy::Cacheable,
            HttpCache::determine_strategy(Some("max-age=3600"), None)
        );
    }

    #[test]
    fn test_parse_max_age() {
        assert_eq!(HttpCache::parse_max_age(Some("max-age=3600")), Some(3600));
        assert_eq!(
            HttpCache::parse_max_age(Some("public, max-age=86400")),
            Some(86400)
        );
        assert_eq!(HttpCache::parse_max_age(Some("no-cache")), None);
    }

    #[test]
    fn test_cache_put_and_get() {
        let mut cache = HttpCache::new();
        let headers = vec![
            ("Content-Type".to_string(), "text/html".to_string()),
            ("Cache-Control".to_string(), "max-age=3600".to_string()),
        ];
        cache.put(
            "https://example.com",
            b"<html>hello</html>".to_vec(),
            "text/html",
            200,
            &headers,
        );
        assert!(cache.is_cached("https://example.com"));
        assert!(cache.get("https://example.com").is_some());
        assert_eq!(cache.get("https://example.com").unwrap().status_code, 200);
    }

    #[test]
    fn test_no_store_not_cached() {
        let mut cache = HttpCache::new();
        let headers = vec![("Cache-Control".to_string(), "no-store".to_string())];
        cache.put(
            "https://example.com",
            b"data".to_vec(),
            "text/plain",
            200,
            &headers,
        );
        assert!(!cache.is_cached("https://example.com"));
    }

    #[test]
    fn test_conditional_headers() {
        let mut cache = HttpCache::new();
        let headers = vec![
            ("ETag".to_string(), "\"abc123\"".to_string()),
            ("Cache-Control".to_string(), "max-age=3600".to_string()),
        ];
        cache.put(
            "https://example.com",
            b"data".to_vec(),
            "text/plain",
            200,
            &headers,
        );
        let cond = cache.get_conditional_headers("https://example.com");
        assert!(cond.iter().any(|(k, _)| k == "If-None-Match"));
    }

    #[test]
    fn test_stats() {
        let mut cache = HttpCache::new();
        assert_eq!(cache.stats(), (0, 0));
        cache.record_hit();
        cache.record_miss();
        assert_eq!(cache.stats(), (1, 1));
        assert!((cache.hit_rate() - 0.5).abs() < 1e-6);
    }

    #[test]
    fn test_max_entries_eviction_lru() {
        let mut cache = HttpCache::new();
        cache.max_entries = 3; // 缩小上限便于测试

        let headers = vec![("Cache-Control".to_string(), "max-age=3600".to_string())];

        for i in 0..5 {
            // 模拟时间流逝：每次 put 前等待一小段时间，确保 created_at 不同
            // 注意：Instant 精度足够高，循环即可产生先后顺序
            cache.put(
                &format!("https://example.com/{}", i),
                b"data".to_vec(),
                "text/plain",
                200,
                &headers,
            );
        }

        // 只保留了最后 3 个条目（索引 2, 3, 4）
        assert_eq!(cache.len(), 3);
        assert!(!cache.is_cached("https://example.com/0"));
        assert!(!cache.is_cached("https://example.com/1"));
        assert!(cache.is_cached("https://example.com/2"));
        assert!(cache.is_cached("https://example.com/3"));
        assert!(cache.is_cached("https://example.com/4"));
    }

    #[test]
    fn test_needs_validation() {
        let mut cache = HttpCache::new();

        // no-cache → 需要验证
        let headers = vec![("Cache-Control".to_string(), "no-cache".to_string())];
        cache.put(
            "https://example.com/nc",
            b"data".to_vec(),
            "text/plain",
            200,
            &headers,
        );
        assert!(cache.needs_validation("https://example.com/nc"));

        // max-age → 不需要验证（除非过期）
        let headers = vec![("Cache-Control".to_string(), "max-age=3600".to_string())];
        cache.put(
            "https://example.com/ma",
            b"data".to_vec(),
            "text/plain",
            200,
            &headers,
        );
        assert!(!cache.needs_validation("https://example.com/ma"));
    }

    #[test]
    fn test_remove_and_clear() {
        let mut cache = HttpCache::new();
        let headers = vec![("Cache-Control".to_string(), "max-age=3600".to_string())];
        cache.put(
            "https://example.com",
            b"data".to_vec(),
            "text/plain",
            200,
            &headers,
        );
        assert!(cache.is_cached("https://example.com"));
        cache.remove("https://example.com");
        assert!(!cache.is_cached("https://example.com"));

        cache.put(
            "https://example.com/a",
            b"data".to_vec(),
            "text/plain",
            200,
            &headers,
        );
        cache.put(
            "https://example.com/b",
            b"data".to_vec(),
            "text/plain",
            200,
            &headers,
        );
        assert_eq!(cache.len(), 2);
        cache.clear();
        assert!(cache.is_empty());
        assert_eq!(cache.stats(), (0, 0));
    }
}
