//! 页面加载器 —— 预扫描 + 并行下载 + 流式渲染
//!
//! 优化目标：从 5 秒加速到 < 1 秒看到网页
//!
//! # 策略
//!
//! HTML 下载完成后：
//!   ① 预扫描 HTML（正则提取资源 URL）← O(n)，不构建 DOM
//!   ② 启动并行下载：JS + CSS + 图片（并发，非阻塞）
//!   ③ 同时构建 DOM（kuchiki 解析）
//!   ─── 以上三步并行 ───
//!   ④ 等待 JS + CSS 下载完成
//!   ⑤ 执行 JS（可能修改 DOM）
//!   ⑥ 应用 CSS → layout → render → 显示

use crate::network::NetworkClient;
use lazy_static::lazy_static;
use log::{debug, info, warn};
use regex::Regex;
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

/// 预扫描结果：从 HTML 中提取的所有资源 URL
#[derive(Debug, Default, Clone)]
pub struct ResourceUrls {
    /// <script src="...">
    pub scripts: Vec<String>,
    /// <link rel="stylesheet" href="...">
    pub stylesheets: Vec<String>,
    /// <img src="...">
    pub images: Vec<String>,
    /// <link rel="preload" href="...">
    pub preloads: Vec<String>,
}

/// 下载完成的资源
#[derive(Debug, Clone)]
pub struct FetchedResource {
    pub url: String,
    pub bytes: Vec<u8>,
    pub content_type: String,
}

/// 资源下载结果缓存
#[derive(Default)]
pub struct ResourceCache {
    /// URL → 下载内容
    map: HashMap<String, FetchedResource>,
    /// 正在下载中的 URL（用于去重）
    inflight: Vec<String>,
}

impl ResourceCache {
    /// 检查是否已缓存
    pub fn has(&self, url: &str) -> bool {
        self.map.contains_key(url)
    }

    /// 获取缓存
    pub fn get(&self, url: &str) -> Option<&FetchedResource> {
        self.map.get(url)
    }

    /// 插入缓存
    pub fn insert(&mut self, url: String, resource: FetchedResource) {
        self.map.insert(url, resource);
    }
}

/// 页面加载器
pub struct PageLoader {
    client: NetworkClient,
    /// 资源缓存（线程安全，供并行下载写入、渲染线程读取）
    cache: Arc<Mutex<ResourceCache>>,
    /// 待下载资源计数
    pending: Arc<AtomicUsize>,
}

impl PageLoader {
    pub fn new() -> Self {
        Self {
            client: NetworkClient::new(),
            cache: Arc::new(Mutex::new(ResourceCache::default())),
            pending: Arc::new(AtomicUsize::new(0)),
        }
    }

    /// 预扫描 HTML，提取所有资源 URL
    ///
    /// 使用正则快速扫描，不构建 DOM。O(n) 时间，n = HTML 长度。
    pub fn prescan(html: &str, base_url: &str) -> ResourceUrls {
        let mut urls = ResourceUrls::default();
        let base = base_url.trim_end_matches('/');

        lazy_static! {
            static ref SCRIPT_RE: Regex = Regex::new(r##"<script[^>]*src="([^"]+)""##).unwrap();
            static ref STYLESHEET_RE_1: Regex =
                Regex::new(r##"<link[^>]*rel="stylesheet"[^>]*href="([^"]+)""##).unwrap();
            static ref STYLESHEET_RE_2: Regex =
                Regex::new(r##"<link[^>]*href="([^"]+)"[^>]*rel="stylesheet""##).unwrap();
            static ref IMG_RE: Regex = Regex::new(r##"<img[^>]*src="([^"]+)""##).unwrap();
            static ref PRELOAD_RE: Regex =
                Regex::new(r##"<link[^>]*rel="preload"[^>]*href="([^"]+)""##).unwrap();
        }

        for cap in SCRIPT_RE.captures_iter(html) {
            if let Some(m) = cap.get(1) {
                urls.scripts.push(resolve_url(m.as_str(), base));
            }
        }
        for cap in STYLESHEET_RE_1.captures_iter(html) {
            if let Some(m) = cap.get(1) {
                urls.stylesheets.push(resolve_url(m.as_str(), base));
            }
        }
        for cap in STYLESHEET_RE_2.captures_iter(html) {
            if let Some(m) = cap.get(1) {
                urls.stylesheets.push(resolve_url(m.as_str(), base));
            }
        }
        for cap in IMG_RE.captures_iter(html) {
            if let Some(m) = cap.get(1) {
                urls.images.push(resolve_url(m.as_str(), base));
            }
        }
        for cap in PRELOAD_RE.captures_iter(html) {
            if let Some(m) = cap.get(1) {
                urls.preloads.push(resolve_url(m.as_str(), base));
            }
        }

        debug!(
            "预扫描: {} 个脚本, {} 个样式表, {} 张图片",
            urls.scripts.len(),
            urls.stylesheets.len(),
            urls.images.len()
        );
        urls
    }

    /// 并行下载所有资源
    ///
    /// 启动 tokio 任务并发下载，不阻塞当前线程。
    /// 下载完成后写入缓存。
    pub fn fetch_all(&self, urls: &ResourceUrls) {
        let total =
            urls.scripts.len() + urls.stylesheets.len() + urls.images.len() + urls.preloads.len();
        if total == 0 {
            return;
        }
        self.pending.store(total, Ordering::SeqCst);
        info!("并行下载 {} 个资源", total);

        // 复制一份 URLs，避免引用问题
        let all_urls: Vec<String> = urls
            .scripts
            .iter()
            .chain(urls.stylesheets.iter())
            .chain(urls.images.iter())
            .chain(urls.preloads.iter())
            .cloned()
            .collect();

        for url in all_urls {
            let cache = Arc::clone(&self.cache);
            let pending = Arc::clone(&self.pending);
            let client = self.client.clone();

            // 先检查是否已缓存
            {
                let c = cache.lock().unwrap();
                if c.has(&url) {
                    pending.fetch_sub(1, Ordering::SeqCst);
                    continue;
                }
            }

            // 标记为正在下载
            {
                let mut c = cache.lock().unwrap();
                if c.inflight.contains(&url) {
                    pending.fetch_sub(1, Ordering::SeqCst);
                    continue; // 去重
                }
                c.inflight.push(url.clone());
            }

            // 启动异步下载（使用 tokio::spawn 在后台执行）
            tokio::spawn(async move {
                let start = Instant::now();
                match client.fetch(&url).await {
                    Ok(response) => {
                        let resource = FetchedResource {
                            url: url.clone(),
                            bytes: response.body,
                            content_type: response
                                .headers
                                .get("content-type")
                                .cloned()
                                .unwrap_or_default(),
                        };
                        let mut c = cache.lock().unwrap();
                        c.insert(url.clone(), resource);
                        debug!("下载完成: {} ({}ms)", url, start.elapsed().as_millis());
                    }
                    Err(e) => {
                        warn!("下载失败 ({}): {}", url, e);
                    }
                }
                pending.fetch_sub(1, Ordering::SeqCst);
            });
        }
    }

    /// 等待所有资源下载完成（超时控制）
    pub fn wait_for_all(&self, timeout_ms: u64) {
        let start = Instant::now();
        while self.pending.load(Ordering::SeqCst) > 0 {
            if start.elapsed().as_millis() > timeout_ms as u128 {
                warn!(
                    "资源下载超时 ({}ms), 剩余 {} 个",
                    timeout_ms,
                    self.pending.load(Ordering::SeqCst)
                );
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }

    /// 获取缓存引用
    pub fn cache(&self) -> &Arc<Mutex<ResourceCache>> {
        &self.cache
    }

    /// 从缓存中取出脚本内容
    pub fn get_script(&self, url: &str) -> Option<String> {
        let c = self.cache.lock().ok()?;
        c.get(url)
            .map(|r| String::from_utf8_lossy(&r.bytes).to_string())
    }

    /// 从缓存中取出 CSS 内容
    pub fn get_css(&self, url: &str) -> Option<String> {
        self.get_script(url) // 逻辑相同
    }

    /// 从缓存中取出图片字节
    pub fn get_image(&self, url: &str) -> Option<Vec<u8>> {
        let c = self.cache.lock().ok()?;
        c.get(url).map(|r| r.bytes.clone())
    }
}

impl Default for PageLoader {
    fn default() -> Self {
        Self::new()
    }
}

/// 解析相对 URL 为绝对 URL
fn resolve_url(href: &str, base: &str) -> String {
    if href.starts_with("http://") || href.starts_with("https://") {
        href.to_string()
    } else if href.starts_with("//") {
        format!("https:{}", href)
    } else if href.starts_with('/') {
        // 从 base 中提取 origin（scheme + host + port）
        if let Some(pos) = base.find("://") {
            let after_scheme = &base[pos + 3..];
            if let Some(slash) = after_scheme.find('/') {
                let origin = &base[..=pos + 3 + slash - 1];
                format!("{}{}", origin, href)
            } else {
                format!("{}{}", base, href)
            }
        } else {
            format!("{}{}", base, href)
        }
    } else {
        format!(
            "{}/{}",
            base.trim_end_matches('/'),
            href.trim_start_matches("./")
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_prescan_empty() {
        let urls = PageLoader::prescan("<html><body></body></html>", "https://example.com");
        assert!(urls.scripts.is_empty());
        assert!(urls.stylesheets.is_empty());
        assert!(urls.images.is_empty());
    }

    #[test]
    fn test_prescan_scripts() {
        let html = r#"<html><script src="/app.js"></script><script src="https://cdn.com/lib.js"></script></html>"#;
        let urls = PageLoader::prescan(html, "https://example.com/page");
        assert_eq!(urls.scripts.len(), 2);
    }

    #[test]
    fn test_prescan_stylesheets() {
        let html = r#"<html><link rel="stylesheet" href="/style.css"><link href="/print.css" rel="stylesheet"></html>"#;
        let urls = PageLoader::prescan(html, "https://example.com");
        assert_eq!(urls.stylesheets.len(), 2);
    }

    #[test]
    fn test_prescan_images() {
        let html = r#"<html><img src="pic1.png"><img src="/pic2.jpg"></html>"#;
        let urls = PageLoader::prescan(html, "https://example.com/page/");
        assert_eq!(urls.images.len(), 2);
    }

    #[test]
    fn test_resolve_url_absolute() {
        assert_eq!(
            resolve_url("https://cdn.com/a.js", "https://example.com"),
            "https://cdn.com/a.js"
        );
    }

    #[test]
    fn test_resolve_url_relative() {
        assert_eq!(
            resolve_url("/a.js", "https://example.com/page"),
            "https://example.com/a.js"
        );
    }

    #[test]
    fn test_resolve_url_protocol_relative() {
        assert_eq!(
            resolve_url("//cdn.com/a.js", "https://example.com"),
            "https://cdn.com/a.js"
        );
    }

    #[test]
    fn test_loader_creation() {
        let loader = PageLoader::new();
        assert_eq!(loader.pending.load(Ordering::SeqCst), 0);
    }
}
