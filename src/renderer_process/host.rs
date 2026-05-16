//! RendererProcessHost - 渲染器进程宿主（在浏览器进程中管理）
//!
//! 负责创建和管理渲染器线程的生命周期。
//! 每个渲染器线程对应一个标签页，拥有独立的 Mojo IPC 通道。
//!
//! # 生命周期
//!
//! 1. 浏览器进程调用 `BrowserProcessHost::spawn_renderer()` 创建新渲染器
//! 2. 该方法创建三条 Mojo 管道并启动渲染器线程
//! 3. 渲染器线程初始化 `Renderer` 实例并进入消息循环
//! 4. 浏览器进程通过 Mojo 管道发送导航/输入/尺寸消息
//! 5. 渲染器处理消息并将渲染结果帧通过 `RenderResult` 管道发回
//! 6. 标签页关闭时，浏览器端关闭 Mojo 管道，渲染器线程退出循环
//!
//! # 配置
//!
//! 通过 [`RendererProcessConfig`] 设置渲染器的初始参数。

use log::info;

/// 渲染器进程的配置参数
///
/// 用于在创建渲染器时指定初始状态。
#[derive(Debug, Clone)]
pub struct RendererProcessConfig {
    /// 渲染器唯一标识
    pub id: u64,
    /// 初始导航 URL
    pub url: String,
    /// 视口宽度（px）
    pub width: u32,
    /// 视口高度（px）
    pub height: u32,
}

impl RendererProcessConfig {
    /// 创建新的渲染器配置
    ///
    /// # 参数
    ///
    /// * `id` - 渲染器唯一标识
    /// * `url` - 初始导航 URL
    /// * `width` - 视口宽度
    /// * `height` - 视口高度
    pub fn new(id: u64, url: &str, width: u32, height: u32) -> Self {
        info!(
            "RendererProcessConfig #{}: {} ({}x{})",
            id, url, width, height
        );
        Self {
            id,
            url: url.to_string(),
            width,
            height,
        }
    }

    /// 更新视口尺寸
    pub fn set_viewport(&mut self, width: u32, height: u32) {
        self.width = width;
        self.height = height;
    }

    /// 更新目标 URL
    pub fn set_url(&mut self, url: &str) {
        self.url = url.to_string();
    }
}

/// 渲染器进程宿主句柄
///
/// 在浏览器进程中代表一个渲染器实例的管理句柄。
/// 包含渲染器的配置信息和基本控制方法。
pub struct RendererProcessHost {
    /// 渲染器配置
    config: RendererProcessConfig,
}

impl RendererProcessHost {
    /// 创建新的渲染器进程宿主
    ///
    /// # 参数
    ///
    /// * `config` - 渲染器的初始配置
    pub fn new(config: RendererProcessConfig) -> Self {
        info!(
            "创建 RendererProcessHost #{}: {} ({}x{})",
            config.id, config.url, config.width, config.height
        );
        Self { config }
    }

    /// 返回渲染器 ID
    pub fn id(&self) -> u64 {
        self.config.id
    }

    /// 返回当前 URL
    pub fn url(&self) -> &str {
        &self.config.url
    }

    /// 返回视口宽度
    pub fn width(&self) -> u32 {
        self.config.width
    }

    /// 返回视口高度
    pub fn height(&self) -> u32 {
        self.config.height
    }

    /// 返回配置引用
    pub fn config(&self) -> &RendererProcessConfig {
        &self.config
    }

    /// 更新视口尺寸
    pub fn set_viewport(&mut self, width: u32, height: u32) {
        self.config.set_viewport(width, height);
    }

    /// 更新目标 URL
    pub fn set_url(&mut self, url: &str) {
        self.config.set_url(url);
    }
}

// ==========================================================================
// Tests
// ==========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_renderer_process_config_creation() {
        let config = RendererProcessConfig::new(1, "about:blank", 800, 600);
        assert_eq!(config.id, 1);
        assert_eq!(config.url, "about:blank");
        assert_eq!(config.width, 800);
        assert_eq!(config.height, 600);
    }

    #[test]
    fn test_renderer_process_config_set_viewport() {
        let mut config = RendererProcessConfig::new(1, "https://example.com", 800, 600);
        config.set_viewport(1920, 1080);
        assert_eq!(config.width, 1920);
        assert_eq!(config.height, 1080);
    }

    #[test]
    fn test_renderer_process_config_set_url() {
        let mut config = RendererProcessConfig::new(1, "about:blank", 800, 600);
        config.set_url("https://rust-lang.org");
        assert_eq!(config.url, "https://rust-lang.org");
    }

    #[test]
    fn test_renderer_process_host_creation() {
        let config = RendererProcessConfig::new(42, "about:blank", 1024, 768);
        let host = RendererProcessHost::new(config);
        assert_eq!(host.id(), 42);
        assert_eq!(host.width(), 1024);
        assert_eq!(host.height(), 768);
    }

    #[test]
    fn test_renderer_process_host_update() {
        let config = RendererProcessConfig::new(7, "about:blank", 800, 600);
        let mut host = RendererProcessHost::new(config);
        host.set_viewport(1280, 720);
        host.set_url("https://news.ycombinator.com");
        assert_eq!(host.width(), 1280);
        assert_eq!(host.height(), 720);
        assert_eq!(host.url(), "https://news.ycombinator.com");
    }
}
