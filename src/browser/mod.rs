//! Browser 模块 - 浏览器核心功能
//!
//! 包含页面管理、导航、标签页、Chrome UI 等

pub mod engine;
pub mod page;
pub mod tabs;
pub mod ui;

pub use engine::{BrowserEngine, Document, BrowserError};
pub use page::Page;
pub use tabs::{Tab, TabManager};
pub use ui::{ChromeUi, ChromeUiConfig, ChromeUiState, ChromeButton, MouseEvent, MouseEventType};

// Re-export Browser for convenience
use crate::css::stylesheet::Stylesheet;
use log::{debug, info};
use std::path::Path;

/// 默认首页
pub const DEFAULT_HOME_URL: &str = "https://www.baidu.com";

/// 浏览器主类
pub struct Browser {
    /// 浏览器引擎（每个标签页一个）
    engines: Vec<BrowserEngine>,
    /// 标签页管理器
    tab_manager: TabManager,
    /// Chrome UI 绘制器
    chrome_ui: ChromeUi,
    /// 全局样式表
    global_stylesheet: Stylesheet,
    /// 视口尺寸
    width: u32,
    height: u32,
}

impl Browser {
    /// 创建新的浏览器实例
    pub fn new() -> Result<Self, BrowserError> {
        info!("创建浏览器实例");

        let width = 1280;
        let height = 800;
        
        let engine = BrowserEngine::new(width, height)?;
        let chrome_ui = ChromeUi::new(width, height)
            .ok_or_else(|| BrowserError::InitError("无法创建 UI".to_string()))?;

        let tab_manager = TabManager::new();
        let engines = vec![engine];

        let browser = Self {
            engines,
            tab_manager,
            chrome_ui,
            global_stylesheet: Stylesheet::default(),
            width,
            height,
        };

        // 注意：不自动加载页面，由调用者决定加载什么

        Ok(browser)
    }

    /// 加载默认首页
    pub fn load_default(&mut self) -> Result<(), BrowserError> {
        info!("加载默认首页: {}", DEFAULT_HOME_URL);
        self.navigate(DEFAULT_HOME_URL)
    }

    /// 加载指定 URL（支持中文域名转换）
    pub fn load_url(&mut self, url: &str) -> Result<(), BrowserError> {
        // 处理中文域名
        let processed_url = Self::process_url(url);
        self.navigate(&processed_url)
    }

    /// 处理 URL（添加协议、转换中文域名等）
    fn process_url(url: &str) -> String {
        let url = url.trim();
        
        // 如果已经是完整 URL，直接返回
        if url.starts_with("http://") || url.starts_with("https://") {
            return url.to_string();
        }
        
        // 如果是本地文件
        if url.ends_with(".html") || url.ends_with(".htm") {
            return url.to_string();
        }
        
        // 尝试作为 URL 处理（添加 https://）
        // 对于中文域名，需要 URL 编码
        let has_cjk = url.chars().any(|c| {
            let code = c as u32;
            // CJK Unified Ideographs Range: 4E00-9FFF
            (0x4E00..=0x9FFF).contains(&code)
        });
        
        if has_cjk {
            // 包含中文字符，需要编码
            let encoded = url.replace(' ', "%20");
            format!("https://{}", encoded)
        } else {
            format!("https://{}", url.replace(' ', "%20"))
        }
    }

    /// 创建带视口的浏览器
    pub fn with_viewport(mut self, width: u32, height: u32) -> Self {
        debug!("设置视口: {}x{}", width, height);
        self.width = width;
        self.height = height;
        
        for engine in &mut self.engines {
            engine.set_viewport(width, height);
        }
        
        self.chrome_ui.set_size(width, height);
        self
    }

    /// 添加全局样式表
    pub fn add_stylesheet(&mut self, css: &str) -> Result<(), BrowserError> {
        let stylesheet = Stylesheet::parse(css)
            .map_err(|e| BrowserError::InitError(e))?;
        
        self.global_stylesheet = stylesheet;
        
        for engine in &mut self.engines {
            engine.add_stylesheet("global", css);
        }
        
        Ok(())
    }

    /// 导航到 URL
    pub fn navigate(&mut self, url: &str) -> Result<(), BrowserError> {
        info!("导航: {}", url);
        
        // 确保活跃引擎存在
        self.ensure_active_engine()?;
        
        let active_idx = self.tab_manager.active_index();
        if active_idx >= self.engines.len() {
            self.engines.push(BrowserEngine::new(self.width, self.height)?);
        }
        
        self.engines[active_idx].navigate(url)?;
        
        // 更新标签页信息
        let title = self.engines[active_idx].title().map(String::from);
        self.tab_manager.update_active_tab(url, title);
        
        Ok(())
    }

    /// 确保活跃引擎存在
    fn ensure_active_engine(&mut self) -> Result<(), BrowserError> {
        let active_idx = self.tab_manager.active_index();
        
        while self.engines.len() <= active_idx {
            self.engines.push(BrowserEngine::new(self.width, self.height)?);
        }
        
        Ok(())
    }

    /// 创建新标签页
    pub fn new_tab(&mut self) -> Result<(), BrowserError> {
        info!("创建新标签页");
        
        self.tab_manager.new_tab();
        self.ensure_active_engine()?;
        
        Ok(())
    }

    /// 关闭当前标签页
    pub fn close_tab(&mut self) -> Result<(), BrowserError> {
        let active_idx = self.tab_manager.active_index();
        self.tab_manager.close_tab(active_idx)?;
        
        // 移除对应的引擎
        if active_idx < self.engines.len() {
            self.engines.remove(active_idx);
        }
        
        Ok(())
    }

    /// 切换到指定标签页
    pub fn switch_to_tab(&mut self, index: usize) {
        self.tab_manager.switch_to(index);
    }

    /// 获取标签页列表
    pub fn tabs(&self) -> &[Tab] {
        self.tab_manager.tabs()
    }

    /// 获取活跃标签页
    pub fn active_tab(&self) -> Option<&Tab> {
        self.tab_manager.active_tab()
    }

    /// 后退
    pub fn go_back(&mut self) -> Result<(), BrowserError> {
        if let Some(url) = self.tab_manager.go_back() {
            let active_idx = self.tab_manager.active_index();
            if active_idx < self.engines.len() {
                self.engines[active_idx].navigate(&url)?;
            }
        }
        Ok(())
    }

    /// 前进
    pub fn go_forward(&mut self) -> Result<(), BrowserError> {
        if let Some(url) = self.tab_manager.go_forward() {
            let active_idx = self.tab_manager.active_index();
            if active_idx < self.engines.len() {
                self.engines[active_idx].navigate(&url)?;
            }
        }
        Ok(())
    }

    /// 刷新
    pub fn reload(&mut self) -> Result<(), BrowserError> {
        if let Some(url) = self.tab_manager.reload_url() {
            let active_idx = self.tab_manager.active_index();
            if active_idx < self.engines.len() {
                self.engines[active_idx].navigate(&url)?;
            }
        }
        Ok(())
    }

    /// 执行 JavaScript
    pub fn evaluate(&self, script: &str) -> Result<String, BrowserError> {
        let active_idx = self.tab_manager.active_index();
        if active_idx < self.engines.len() {
            self.engines[active_idx].execute_js(script)
        } else {
            Ok("undefined".to_string())
        }
    }

    /// 获取页面标题
    pub fn title(&self) -> Option<&str> {
        let active_idx = self.tab_manager.active_index();
        if active_idx < self.engines.len() {
            self.engines[active_idx].title()
        } else {
            None
        }
    }

    /// 获取当前 URL
    pub fn url(&self) -> &str {
        self.tab_manager.active_tab()
            .map(|t| t.url.as_str())
            .unwrap_or("about:blank")
    }

    /// 获取视口尺寸
    pub fn viewport(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// 获取 UI 高度
    pub fn ui_height(&self) -> u32 {
        self.chrome_ui.ui_height()
    }

    /// 获取内容区域
    pub fn content_area(&self) -> (u32, u32, u32, u32) {
        self.chrome_ui.content_area()
    }

    /// 截取截图
    pub fn screenshot(&mut self, path: &Path) -> Result<(), BrowserError> {
        debug!("截图: {:?}", path);

        // 渲染完整 UI
        let image_data = self.render_full()?;
        
        let img = image::load_from_memory(&image_data)
            .map_err(|e| BrowserError::RenderError(e.to_string()))?;
        
        img.save(path)
            .map_err(|e| BrowserError::RenderError(e.to_string()))?;

        Ok(())
    }

    /// 获取 UI 状态
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

    /// 渲染完整 UI（包含页面内容）
    pub fn render_full(&mut self) -> Result<Vec<u8>, BrowserError> {
        // 获取页面渲染
        let active_idx = self.tab_manager.active_index();
        
        // 渲染页面
        let page_image = if active_idx < self.engines.len() {
            self.engines[active_idx].render_to_image().unwrap_or_else(|_| vec![0u8; 100])
        } else {
            vec![0u8; 100]
        };
        
        // 加载页面图像
        let page_pixmap = if let Ok(img) = image::load_from_memory(&page_image) {
            let rgb_image = img.to_rgb8();
            let (w, h) = rgb_image.dimensions();
            let mut pixmap = tiny_skia::Pixmap::new(w, h).ok_or_else(|| 
                BrowserError::RenderError("无法创建像素图".to_string()))?;
            
            for y in 0..h {
                for x in 0..w {
                    let pixel = rgb_image.get_pixel(x, y);
                    let color = tiny_skia::Color::from_rgba8(pixel[0], pixel[1], pixel[2], 255);
                    let _ = pixmap.fill(color);
                }
            }
            pixmap
        } else {
            let mut p = tiny_skia::Pixmap::new(1, 1).ok_or_else(|| 
                BrowserError::RenderError("无法创建像素图".to_string()))?;
            let _ = p.fill(tiny_skia::Color::from_rgba8(255, 255, 255, 255));
            p
        };
        
        // 渲染 UI
        let ui_state = self.ui_state();
        self.chrome_ui.render(&self.tab_manager, &ui_state);
        
        // 合成
        self.chrome_ui.composite_page(&page_pixmap, 0, self.ui_height());
        
        Ok(self.chrome_ui.to_png())
    }

    /// 获取渲染后的图像数据
    pub fn render(&mut self) -> Result<Vec<u8>, BrowserError> {
        self.render_full()
    }

    /// 获取 Chrome UI 像素图
    pub fn chrome_pixmap(&self) -> &tiny_skia::Pixmap {
        self.chrome_ui.pixmap()
    }

    /// 获取页面像素图
    pub fn page_pixmap(&mut self) -> Option<tiny_skia::Pixmap> {
        let active_idx = self.tab_manager.active_index();
        if active_idx < self.engines.len() {
            if let Ok(data) = self.engines[active_idx].render_to_image() {
                if let Ok(img) = image::load_from_memory(&data) {
                    let rgb_image = img.to_rgb8();
                    let (w, h) = rgb_image.dimensions();
                    let mut pixmap = tiny_skia::Pixmap::new(w, h)?;
                    
                    for y in 0..h {
                        for x in 0..w {
                            let pixel = rgb_image.get_pixel(x, y);
                            let color = tiny_skia::Color::from_rgba8(pixel[0], pixel[1], pixel[2], 255);
                            let _ = pixmap.fill(color);
                        }
                    }
                    return Some(pixmap);
                }
            }
        }
        None
    }
}

impl Default for Browser {
    fn default() -> Self {
        Self::new().expect("创建浏览器失败")
    }
}
