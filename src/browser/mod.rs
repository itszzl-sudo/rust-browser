//! Browser 模块 - 浏览器核心功能
//!
//! 包含页面管理、导航、标签页等

pub mod engine;
pub mod page;
pub mod tabs;
pub mod ui;

pub use engine::{BrowserEngine, BrowserError, Document};
pub use page::Page;
pub use tabs::{Tab, TabManager};
pub use ui::{ChromeButton, ChromeUiState, MouseEvent, MouseEventType};

use log::{debug, info};
use std::path::Path;

pub const DEFAULT_HOME_URL: &str = "https://www.baidu.com";

const DEFAULT_UI_HEIGHT: u32 = 40;

pub struct Browser {
    engines: Vec<BrowserEngine>,
    tab_manager: TabManager,
    width: u32,
    height: u32,
}

impl Browser {
    pub fn new() -> Result<Self, BrowserError> {
        info!("创建浏览器实例");

        let width = 1280;
        let height = 800;

        let engine = BrowserEngine::new(width, height)?;
        let tab_manager = TabManager::new();
        let engines = vec![engine];

        let browser = Self {
            engines,
            tab_manager,
            width,
            height,
        };

        Ok(browser)
    }

    pub fn load_default(&mut self) -> Result<(), BrowserError> {
        info!("加载默认首页: {}", DEFAULT_HOME_URL);
        self.navigate(DEFAULT_HOME_URL)
    }

    pub fn load_url(&mut self, url: &str) -> Result<(), BrowserError> {
        let processed_url = Self::process_url(url);
        self.navigate(&processed_url)
    }

    fn process_url(url: &str) -> String {
        let url = url.trim();

        if url.starts_with("http://") || url.starts_with("https://") {
            return url.to_string();
        }

        if url.ends_with(".html") || url.ends_with(".htm") {
            return url.to_string();
        }

        let has_cjk = url.chars().any(|c| {
            let code = c as u32;
            (0x4E00..=0x9FFF).contains(&code)
        });

        if has_cjk {
            let encoded = url.replace(' ', "%20");
            format!("https://{}", encoded)
        } else {
            format!("https://{}", url.replace(' ', "%20"))
        }
    }

    pub fn with_viewport(mut self, width: u32, height: u32) -> Self {
        debug!("设置视口: {}x{}", width, height);
        self.width = width;
        self.height = height;

        for engine in &mut self.engines {
            engine.set_viewport(width, height);
        }

        self
    }

    pub fn navigate(&mut self, url: &str) -> Result<(), BrowserError> {
        info!("导航: {}", url);

        self.ensure_active_engine()?;

        let active_idx = self.tab_manager.active_index();
        if active_idx >= self.engines.len() {
            self.engines
                .push(BrowserEngine::new(self.width, self.height)?);
        }

        self.engines[active_idx].navigate(url)?;

        let title = self.engines[active_idx].title().map(String::from);
        self.tab_manager.update_active_tab(url, title);

        Ok(())
    }

    fn ensure_active_engine(&mut self) -> Result<(), BrowserError> {
        let active_idx = self.tab_manager.active_index();

        while self.engines.len() <= active_idx {
            self.engines
                .push(BrowserEngine::new(self.width, self.height)?);
        }

        Ok(())
    }

    pub fn new_tab(&mut self) -> Result<(), BrowserError> {
        info!("创建新标签页");

        self.tab_manager.new_tab();
        self.ensure_active_engine()?;

        Ok(())
    }

    pub fn close_tab(&mut self) -> Result<(), BrowserError> {
        let active_idx = self.tab_manager.active_index();
        self.tab_manager.close_tab(active_idx)?;

        if active_idx < self.engines.len() {
            self.engines.remove(active_idx);
        }

        Ok(())
    }

    pub fn switch_to_tab(&mut self, index: usize) {
        self.tab_manager.switch_to(index);
    }

    pub fn tabs(&self) -> &[Tab] {
        self.tab_manager.tabs()
    }

    pub fn active_tab(&self) -> Option<&Tab> {
        self.tab_manager.active_tab()
    }

    pub fn go_back(&mut self) -> Option<String> {
        self.tab_manager.go_back()
    }

    pub fn go_forward(&mut self) -> Option<String> {
        self.tab_manager.go_forward()
    }

    pub fn reload(&self) -> Option<String> {
        self.tab_manager.reload_url()
    }

    pub fn evaluate(&mut self, script: &str) -> Result<String, BrowserError> {
        let active_idx = self.tab_manager.active_index();
        if active_idx < self.engines.len() {
            self.engines[active_idx].execute_js(script)
        } else {
            Ok("undefined".to_string())
        }
    }

    pub fn title(&self) -> Option<&str> {
        let active_idx = self.tab_manager.active_index();
        if active_idx < self.engines.len() {
            self.engines[active_idx].title()
        } else {
            None
        }
    }

    pub fn favicon(&self) -> Option<&str> {
        let active_idx = self.tab_manager.active_index();
        if active_idx < self.engines.len() {
            self.engines[active_idx].favicon()
        } else {
            None
        }
    }

    pub fn url(&self) -> &str {
        self.tab_manager
            .active_tab()
            .map(|t| t.url.as_str())
            .unwrap_or("about:blank")
    }

    pub fn viewport(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    pub fn ui_height(&self) -> u32 {
        DEFAULT_UI_HEIGHT
    }

    pub fn content_area(&self) -> (u32, u32, u32, u32) {
        (
            0,
            DEFAULT_UI_HEIGHT,
            self.width,
            self.height.saturating_sub(DEFAULT_UI_HEIGHT),
        )
    }

    pub fn screenshot(&mut self, path: &Path) -> Result<(), BrowserError> {
        debug!("截图: {:?}", path);

        let image_data = self.render_full()?;

        let img = image::load_from_memory(&image_data)
            .map_err(|e| BrowserError::RenderError(e.to_string()))?;

        img.save(path)
            .map_err(|e| BrowserError::RenderError(e.to_string()))?;

        Ok(())
    }

    pub fn ui_state(&self) -> ChromeUiState {
        let tab = self.tab_manager.active_tab();

        ChromeUiState {
            can_go_back: self.tab_manager.can_go_back(),
            can_go_forward: self.tab_manager.can_go_forward(),
            is_loading: tab.map(|t| t.is_loading).unwrap_or(false),
            address_bar_text: tab.map(|t| t.url.clone()).unwrap_or_default(),
            is_secure: tab.map(|t| t.url.starts_with("https://")).unwrap_or(false),
            hovered_button: None,
        }
    }

    pub fn render_full(&mut self) -> Result<Vec<u8>, BrowserError> {
        let active_idx = self.tab_manager.active_index();

        // 尝试渲染页面，如果失败直接返回备用图像
        let page_image = if active_idx < self.engines.len() {
            self.engines[active_idx].render_to_image().ok()
        } else {
            None
        };

        if let Some(img_data) = page_image {
            Ok(img_data)
        } else {
            // 使用实际视口尺寸渲染备用图像
            let (width, height) = (self.width, self.height);
            let mut p = tiny_skia::Pixmap::new(width, height)
                .ok_or_else(|| BrowserError::RenderError("无法创建像素图".to_string()))?;
            let _ = p.fill(tiny_skia::Color::from_rgba8(255, 255, 255, 255));
            Ok(p.encode_png().unwrap_or_default())
        }
    }

    pub fn render(&mut self) -> Result<Vec<u8>, BrowserError> {
        self.render_full()
    }
}

impl Default for Browser {
    fn default() -> Self {
        Self::new().expect("创建浏览器失败")
    }
}
