//! 浏览器引擎 - 核心浏览器功能实现
//!
//! 使用 obscura-net, kuchiki, Taffy 布局和 tiny-skia 渲染

use crate::{renderer::Renderer, NetworkClient, DomWrapper};
use log::{debug, info, warn};
use std::path::Path;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum BrowserError {
    #[error("初始化失败: {0}")]
    InitError(String),
    #[error("导航失败: {0}")]
    NavigationError(String),
    #[error("渲染失败: {0}")]
    RenderError(String),
    #[error("页面未加载")]
    PageNotLoaded,
    #[error("网络请求失败: {0}")]
    NetworkError(String),
}

#[derive(Clone)]
pub struct Document {
    pub dom: DomWrapper,
    pub title: Option<String>,
    pub url: String,
}

impl std::fmt::Debug for Document {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Document")
            .field("title", &self.title)
            .field("url", &self.url)
            .finish()
    }
}

impl Document {
    pub fn from_html(html: &str, url: &str) -> Self {
        let dom = DomWrapper::from_html(html, Some(url));
        let title = dom.title();

        Self {
            dom,
            title,
            url: url.to_string(),
        }
    }

    pub fn get_dom(&self) -> &DomWrapper {
        &self.dom
    }
}

lazy_static::lazy_static! {
    static ref RUNTIME: tokio::runtime::Runtime = tokio::runtime::Runtime::new().unwrap();
}

pub struct BrowserEngine {
    renderer: Renderer,
    document: Option<Document>,
    width: u32,
    height: u32,
    title: Option<String>,
    current_url: Option<String>,
    network_client: NetworkClient,
}

impl BrowserEngine {
    pub fn new(width: u32, height: u32) -> Result<Self, BrowserError> {
        info!("初始化浏览器引擎 ({}x{})", width, height);

        let renderer = Renderer::new(width, height);
        let network_client = NetworkClient::new();

        Ok(Self {
            renderer,
            document: None,
            width,
            height,
            title: None,
            current_url: None,
            network_client,
        })
    }

    pub fn navigate(&mut self, url: &str) -> Result<(), BrowserError> {
        info!("导航到: {}", url);

        if url.starts_with("file://") || url.ends_with(".html") || url.ends_with(".htm") {
            return self.load_local_file(url);
        }

        let html = RUNTIME.block_on(self.network_client.fetch_html(url))
            .map_err(|e| BrowserError::NetworkError(e.to_string()))?;

        let doc = Document::from_html(&html, url);
        self.document = Some(doc);
        self.current_url = Some(url.to_string());
        self.title = self.document.as_ref().and_then(|d| d.title.clone());
        info!("页面加载成功");
        Ok(())
    }

    pub async fn navigate_async(&mut self, url: &str) -> Result<(), BrowserError> {
        info!("异步导航到: {}", url);

        if url.starts_with("file://") || url.ends_with(".html") || url.ends_with(".htm") {
            return self.load_local_file(url);
        }

        let html = self.network_client.fetch_html(url).await
            .map_err(|e| BrowserError::NetworkError(e.to_string()))?;

        let doc = Document::from_html(&html, url);
        self.document = Some(doc);
        self.current_url = Some(url.to_string());
        self.title = self.document.as_ref().and_then(|d| d.title.clone());
        info!("页面加载成功");
        Ok(())
    }

    pub fn load_html(&mut self, html: &str, url: &str) -> Result<(), BrowserError> {
        info!("加载 HTML 内容: {}", url);

        let doc = Document::from_html(html, url);
        self.document = Some(doc);
        self.current_url = Some(url.to_string());
        self.title = self.document.as_ref().and_then(|d| d.title.clone());
        Ok(())
    }

    fn load_local_file(&mut self, path: &str) -> Result<(), BrowserError> {
        let path = path.trim_start_matches("file://");

        match std::fs::read_to_string(path) {
            Ok(html) => {
                let doc = Document::from_html(&html, path);
                self.document = Some(doc);
                self.current_url = Some(path.to_string());
                self.title = self.document.as_ref().and_then(|d| d.title.clone());
                info!("本地文件加载成功");
                Ok(())
            }
            Err(e) => {
                let blank_html = r#"<!DOCTYPE html>
<html>
<head><title>空白页面</title></head>
<body style="background: white; display: flex; align-items: center; justify-content: center; height: 100vh; margin: 0;">
    <h1 style="color: #333;">空白页面</h1>
</body>
</html>"#;
                let doc = Document::from_html(blank_html, "about:blank");
                self.document = Some(doc);
                self.current_url = Some("about:blank".to_string());
                self.title = Some("空白页面".to_string());
                warn!("文件加载失败: {}，显示空白页面", e);
                Ok(())
            }
        }
    }

    pub fn title(&self) -> Option<&str> {
        self.title.as_deref()
    }

    pub fn url(&self) -> &str {
        self.current_url.as_deref().unwrap_or("about:blank")
    }

    pub fn viewport(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    pub fn set_viewport(&mut self, width: u32, height: u32) {
        debug!("设置视口: {}x{}", width, height);
        self.width = width;
        self.height = height;
        self.renderer.set_viewport(width, height);
    }

    pub fn dom(&self) -> Option<&Document> {
        self.document.as_ref()
    }

    pub fn render_to_image(&mut self) -> Result<Vec<u8>, BrowserError> {
        if self.document.is_none() {
            return Err(BrowserError::PageNotLoaded);
        }

        self.renderer
            .render(&self.document)
            .map_err(|e| BrowserError::RenderError(e.to_string()))
    }

    pub fn screenshot(&mut self, path: &Path) -> Result<(), BrowserError> {
        let image_data = self.render_to_image()?;

        let img = image::load_from_memory(&image_data)
            .map_err(|e| BrowserError::RenderError(e.to_string()))?;

        img.save(path)
            .map_err(|e| BrowserError::RenderError(e.to_string()))?;

        Ok(())
    }

    pub fn execute_js(&self, _script: &str) -> Result<String, BrowserError> {
        if self.document.is_none() {
            return Err(BrowserError::PageNotLoaded);
        }
        Ok("undefined".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_browser_creation() {
        let browser = BrowserEngine::new(800, 600);
        assert!(browser.is_ok());
    }

    #[test]
    fn test_viewport() {
        let browser = BrowserEngine::new(800, 600).unwrap();
        assert_eq!(browser.viewport(), (800, 600));
    }
}
