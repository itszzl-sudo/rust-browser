//! Chrome 风格 UI 绘制器

use crate::browser::tabs::TabManager;
use log::{debug, trace};
use tiny_skia::{Paint, Pixmap, Rect, Transform};

/// Chrome UI 配置
#[derive(Debug, Clone)]
pub struct ChromeUiConfig {
    /// Tab 栏高度
    pub tab_bar_height: u32,
    /// 工具栏高度
    pub toolbar_height: u32,
    /// Tab 标签宽度
    pub tab_width: u32,
    /// Tab 最大宽度
    pub tab_max_width: u32,
    /// 新标签页按钮大小
    pub new_tab_button_size: u32,
    /// 按钮大小
    pub button_size: u32,
    /// 地址栏高度
    pub address_bar_height: u32,
    /// Tab 背景色
    pub tab_bg_color: [u8; 4],
    /// 激活 Tab 背景色
    pub active_tab_bg_color: [u8; 4],
    /// Tab 文字颜色
    pub tab_text_color: [u8; 4],
    /// 工具栏背景色
    pub toolbar_bg_color: [u8; 4],
    /// 地址栏背景色
    pub address_bar_bg_color: [u8; 4],
    /// 边框颜色
    pub border_color: [u8; 4],
}

impl Default for ChromeUiConfig {
    fn default() -> Self {
        Self {
            tab_bar_height: 40,
            toolbar_height: 56,
            tab_width: 200,
            tab_max_width: 250,
            new_tab_button_size: 28,
            button_size: 32,
            address_bar_height: 36,
            tab_bg_color: [65, 99, 230, 255],
            active_tab_bg_color: [255, 255, 255, 255],
            tab_text_color: [255, 255, 255, 255],
            toolbar_bg_color: [245, 245, 245, 255],
            address_bar_bg_color: [255, 255, 255, 255],
            border_color: [220, 220, 220, 255],
        }
    }
}

/// Chrome UI 状态
#[derive(Debug, Clone)]
pub struct ChromeUiState {
    /// 前进按钮是否可用
    pub can_go_back: bool,
    /// 后退按钮是否可用
    pub can_go_forward: bool,
    /// 是否正在加载
    pub is_loading: bool,
    /// 地址栏文本
    pub address_bar_text: String,
    /// 是否显示 HTTPS 安全指示
    pub is_secure: bool,
    /// 鼠标悬停的按钮
    pub hovered_button: Option<ChromeButton>,
}

impl Default for ChromeUiState {
    fn default() -> Self {
        Self {
            can_go_back: false,
            can_go_forward: false,
            is_loading: false,
            address_bar_text: String::new(),
            is_secure: false,
            hovered_button: None,
        }
    }
}

/// Chrome 按钮类型
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ChromeButton {
    /// 菜单按钮
    Menu,
    /// 后退按钮
    Back,
    /// 前进按钮
    Forward,
    /// 刷新按钮
    Reload,
    /// 主页按钮
    Home,
    /// 新标签页按钮
    NewTab,
    /// 地址栏
    AddressBar,
}

/// 鼠标事件
#[derive(Debug, Clone)]
pub struct MouseEvent {
    /// 事件类型
    pub event_type: MouseEventType,
    /// x 坐标
    pub x: i32,
    /// y 坐标
    pub y: i32,
}

/// 鼠标事件类型
#[derive(Debug, Clone)]
pub enum MouseEventType {
    /// 移动
    Move,
    /// 点击
    Click,
    /// 按下
    Down,
    /// 释放
    Up,
}

/// Chrome UI 绘制器
pub struct ChromeUi {
    /// 像素图
    pixmap: Pixmap,
    /// 配置
    config: ChromeUiConfig,
    /// 页面区域（排除 UI 后）
    content_area: (u32, u32, u32, u32), // x, y, width, height
}

impl ChromeUi {
    /// 创建新的 Chrome UI 绘制器
    pub fn new(width: u32, height: u32) -> Option<Self> {
        let pixmap = Pixmap::new(width, height)?;
        
        let config = ChromeUiConfig::default();
        let content_area = Self::calculate_content_area(width, height, &config);
        
        debug!("创建 Chrome UI ({})", format!("{}x{}", width, height));
        
        Some(Self {
            pixmap,
            config,
            content_area,
        })
    }

    /// 计算内容区域
    fn calculate_content_area(width: u32, height: u32, config: &ChromeUiConfig) -> (u32, u32, u32, u32) {
        let ui_height = config.tab_bar_height + config.toolbar_height;
        let content_y = ui_height;
        let content_height = if height > ui_height { height - ui_height } else { 0 };
        
        (0, content_y, width, content_height)
    }

    /// 获取内容区域
    pub fn content_area(&self) -> (u32, u32, u32, u32) {
        self.content_area
    }

    /// 获取 UI 高度
    pub fn ui_height(&self) -> u32 {
        self.config.tab_bar_height + self.config.toolbar_height
    }

    /// 设置视口大小
    pub fn set_size(&mut self, width: u32, height: u32) {
        if let Some(pixmap) = Pixmap::new(width, height) {
            self.pixmap = pixmap;
            self.content_area = Self::calculate_content_area(width, height, &self.config);
            debug!("设置 UI 大小: {}x{}", width, height);
        }
    }

    /// 绘制完整 UI
    pub fn render(&mut self, tab_manager: &TabManager, state: &ChromeUiState) {
        trace!("渲染 Chrome UI");
        
        // 清空画布
        self.clear();
        
        // 绘制 Tab 栏
        self.draw_tab_bar(tab_manager);
        
        // 绘制工具栏
        self.draw_toolbar(state);
    }

    /// 清空画布
    fn clear(&mut self) {
        self.pixmap.fill(tiny_skia::Color::from_rgba8(255, 255, 255, 255));
    }

    /// 绘制 Tab 栏
    fn draw_tab_bar(&mut self, tab_manager: &TabManager) {
        let config = &self.config;
        let tab_bar_height = config.tab_bar_height;
        let new_tab_button_size = config.new_tab_button_size;
        let tab_max_width = config.tab_max_width;
        let tabs = tab_manager.tabs().to_vec();
        
        // 绘制 Tab 栏背景
        let mut paint = Paint::default();
        paint.set_color_rgba8(30, 30, 30, 255);
        
        if let Some(rect) = Rect::from_xywh(0.0, 0.0, self.pixmap.width() as f32, tab_bar_height as f32) {
            self.pixmap.fill_rect(rect, &paint, Transform::identity(), None);
        }
        
        // 绘制新标签页按钮
        let new_tab_x = self.pixmap.width() - new_tab_button_size;
        let new_tab_y = (tab_bar_height - new_tab_button_size) / 2;
        self.draw_new_tab_button(new_tab_x as i32, new_tab_y as i32, new_tab_button_size);
        
        // 计算 Tab 区域
        let tab_area_start = new_tab_button_size;
        let tab_area_width = self.pixmap.width().saturating_sub(tab_area_start);
        let tab_width = (tab_area_width / tabs.len().max(1) as u32).min(tab_max_width).max(100);
        
        // 绘制每个 Tab
        for (i, tab) in tabs.iter().enumerate() {
            let x = tab_area_start as i32 + (i as i32) * (tab_width as i32);
            let is_active = tab.is_active;
            
            self.draw_tab(tab, x, tab_width, is_active);
        }
    }

    /// 绘制单个 Tab
    fn draw_tab(&mut self, tab: &crate::browser::tabs::Tab, x: i32, width: u32, is_active: bool) {
        let config = &self.config;
        let height = config.tab_bar_height;
        
        // Tab 背景
        let mut bg_paint = Paint::default();
        if is_active {
            bg_paint.set_color_rgba8(255, 255, 255, 255);
        } else {
            bg_paint.set_color_rgba8(70, 70, 70, 255);
        }
        
        if let Some(rect) = Rect::from_xywh((x + 2) as f32, 4.0, (width - 4) as f32, (height - 4) as f32) {
            self.pixmap.fill_rect(rect, &bg_paint, Transform::identity(), None);
        }
        
        // Tab 标题
        let title = tab.title.as_deref().unwrap_or("新标签页");
        let display_title = if title.len() > 15 { format!("{}...", &title[..12]) } else { title.to_string() };
        
        // 绘制关闭按钮
        let close_x = x + width as i32 - 24;
        let close_y = (height as i32 - 16) / 2;
        self.draw_close_button(close_x, close_y, 16);
        
        // 绘制 Tab 内容
        let text_x = x + 12;
        let text_y = (height as i32 - 16) / 2;
        
        let mut text_paint = Paint::default();
        if is_active {
            text_paint.set_color_rgba8(50, 50, 50, 255);
        } else {
            text_paint.set_color_rgba8(200, 200, 200, 255);
        }
        
        let text_width = (display_title.len() as u32 * 8).min(width - 40);
        
        if let Some(rect) = Rect::from_xywh(text_x as f32, text_y as f32, text_width as f32, 16.0) {
            self.pixmap.fill_rect(rect, &text_paint, Transform::identity(), None);
        }
    }

    /// 绘制关闭按钮
    fn draw_close_button(&mut self, x: i32, y: i32, size: u32) {
        let mut paint = Paint::default();
        paint.set_color_rgba8(150, 150, 150, 255);
        
        if let Some(rect) = Rect::from_xywh(x as f32, y as f32, size as f32, size as f32) {
            self.pixmap.fill_rect(rect, &paint, Transform::identity(), None);
        }
    }

    /// 绘制新标签页按钮
    fn draw_new_tab_button(&mut self, x: i32, y: i32, size: u32) {
        let config = &self.config;
        
        let mut bg_paint = Paint::default();
        bg_paint.set_color_rgba8(50, 50, 50, 255);
        
        if let Some(rect) = Rect::from_xywh(x as f32, y as f32, size as f32, size as f32) {
            self.pixmap.fill_rect(rect, &bg_paint, Transform::identity(), None);
        }
        
        let center = (size / 2) as i32;
        let offset = (size / 4) as i32;
        
        let mut paint = Paint::default();
        paint.set_color_rgba8(180, 180, 180, 255);
        
        if let Some(rect) = Rect::from_xywh((x + center - offset) as f32, (y + center - 1) as f32, (offset * 2) as f32, 2.0) {
            self.pixmap.fill_rect(rect, &paint, Transform::identity(), None);
        }
        
        if let Some(rect) = Rect::from_xywh((x + center - 1) as f32, (y + center - offset) as f32, 2.0, (offset * 2) as f32) {
            self.pixmap.fill_rect(rect, &paint, Transform::identity(), None);
        }
    }

    /// 绘制工具栏
    fn draw_toolbar(&mut self, state: &ChromeUiState) {
        let config = &self.config;
        let toolbar_y = config.tab_bar_height;
        let button_size = config.button_size;
        let address_bar_height = config.address_bar_height;
        
        let mut bg_paint = Paint::default();
        bg_paint.set_color_rgba8(
            config.toolbar_bg_color[0],
            config.toolbar_bg_color[1],
            config.toolbar_bg_color[2],
            config.toolbar_bg_color[3],
        );
        
        if let Some(rect) = Rect::from_xywh(0.0, toolbar_y as f32, self.pixmap.width() as f32, config.toolbar_height as f32) {
            self.pixmap.fill_rect(rect, &bg_paint, Transform::identity(), None);
        }
        
        let button_y = toolbar_y + (config.toolbar_height - button_size) / 2;
        let button_start_x = 8;
        let can_go_back = state.can_go_back;
        let can_go_forward = state.can_go_forward;
        
        // 后退按钮
        self.draw_nav_button(button_start_x, button_y as i32, button_size, can_go_back);
        
        // 前进按钮
        self.draw_nav_button(
            button_start_x + button_size as i32 + 4, 
            button_y as i32, 
            button_size, 
            can_go_forward
        );
        
        // 刷新按钮
        self.draw_nav_button(
            button_start_x + (button_size as i32 + 4) * 2, 
            button_y as i32, 
            button_size, 
            true
        );
        
        // 主页按钮
        self.draw_nav_button(
            button_start_x + (button_size as i32 + 4) * 3, 
            button_y as i32, 
            button_size, 
            true
        );
        
        // 地址栏
        let address_bar_x = button_start_x + (button_size as i32 + 4) * 4 + 12;
        let address_bar_width = self.pixmap.width() - address_bar_x as u32 - 80;
        self.draw_address_bar(
            address_bar_x, 
            (toolbar_y + 10) as i32, 
            address_bar_width, 
            address_bar_height,
            state
        );
        
        // 菜单按钮
        self.draw_nav_button(
            self.pixmap.width() as i32 - 48, 
            button_y as i32, 
            button_size - 8, 
            true
        );
    }

    /// 绘制导航按钮
    fn draw_nav_button(&mut self, x: i32, y: i32, size: u32, enabled: bool) {
        let mut bg_paint = Paint::default();
        if enabled {
            bg_paint.set_color_rgba8(100, 100, 100, 255);
        } else {
            bg_paint.set_color_rgba8(180, 180, 180, 255);
        }
        
        if let Some(rect) = Rect::from_xywh(x as f32, y as f32, size as f32, size as f32) {
            self.pixmap.fill_rect(rect, &bg_paint, Transform::identity(), None);
        }
        
        let mut paint = Paint::default();
        if enabled {
            paint.set_color_rgba8(60, 60, 60, 255);
        } else {
            paint.set_color_rgba8(200, 200, 200, 255);
        }
        
        let center = (size / 3) as i32;
        if let Some(rect) = Rect::from_xywh((x + center) as f32, (y + center) as f32, center as f32, center as f32) {
            self.pixmap.fill_rect(rect, &paint, Transform::identity(), None);
        }
    }

    /// 绘制地址栏
    fn draw_address_bar(&mut self, x: i32, y: i32, width: u32, height: u32, state: &ChromeUiState) {
        let config = &self.config;
        
        let mut bg_paint = Paint::default();
        bg_paint.set_color_rgba8(
            config.address_bar_bg_color[0],
            config.address_bar_bg_color[1],
            config.address_bar_bg_color[2],
            config.address_bar_bg_color[3],
        );
        
        if let Some(rect) = Rect::from_xywh(x as f32, y as f32, width as f32, height as f32) {
            self.pixmap.fill_rect(rect, &bg_paint, Transform::identity(), None);
        }
        
        // 边框
        let mut border_paint = Paint::default();
        border_paint.set_color_rgba8(
            config.border_color[0],
            config.border_color[1],
            config.border_color[2],
            config.border_color[3],
        );
        
        let border_width = 1.0;
        
        if let Some(rect) = Rect::from_xywh(x as f32, y as f32, border_width, height as f32) {
            self.pixmap.fill_rect(rect, &border_paint, Transform::identity(), None);
        }
        if let Some(rect) = Rect::from_xywh((x + width as i32) as f32 - border_width, y as f32, border_width, height as f32) {
            self.pixmap.fill_rect(rect, &border_paint, Transform::identity(), None);
        }
        if let Some(rect) = Rect::from_xywh(x as f32, y as f32, width as f32, border_width) {
            self.pixmap.fill_rect(rect, &border_paint, Transform::identity(), None);
        }
        if let Some(rect) = Rect::from_xywh(x as f32, (y + height as i32) as f32 - border_width, width as f32, border_width) {
            self.pixmap.fill_rect(rect, &border_paint, Transform::identity(), None);
        }
        
        // 安全指示器
        let lock_x = x + 8;
        let lock_y = y + (height as i32 - 16) / 2;
        
        let mut lock_paint = Paint::default();
        if state.is_secure {
            lock_paint.set_color_rgba8(0, 150, 0, 255);
        } else {
            lock_paint.set_color_rgba8(150, 150, 150, 255);
        }
        
        if let Some(rect) = Rect::from_xywh(lock_x as f32, lock_y as f32, 16.0, 16.0) {
            self.pixmap.fill_rect(rect, &lock_paint, Transform::identity(), None);
        }
        
        // 地址文本
        let text_x = if state.is_secure { x + 28 } else { x + 28 };
        let text_y = y + (height as i32 - 16) / 2;
        
        let mut text_paint = Paint::default();
        text_paint.set_color_rgba8(50, 50, 50, 255);
        
        let display_url = if state.address_bar_text.is_empty() {
            "在 Google 上搜索或输入网址".to_string()
        } else {
            state.address_bar_text.clone()
        };
        
        let text_width = (display_url.len() as u32 * 7).min(width - 40);
        if let Some(rect) = Rect::from_xywh(text_x as f32, text_y as f32, text_width as f32, 16.0) {
            self.pixmap.fill_rect(rect, &text_paint, Transform::identity(), None);
        }
    }

    /// 处理鼠标事件，返回点击的按钮类型
    pub fn handle_mouse_event(&self, event: &MouseEvent) -> Option<ChromeButton> {
        let (x, y) = (event.x as u32, event.y as u32);
        let config = &self.config;
        
        if y < config.tab_bar_height {
            return self.hit_test_tab_bar(x);
        }
        
        if y >= config.tab_bar_height && y < config.tab_bar_height + config.toolbar_height {
            return self.hit_test_toolbar(x, config.tab_bar_height);
        }
        
        None
    }

    /// 测试 Tab 栏点击
    fn hit_test_tab_bar(&self, x: u32) -> Option<ChromeButton> {
        let config = &self.config;
        
        let new_tab_x = self.pixmap.width() - config.new_tab_button_size;
        if x >= new_tab_x && x < self.pixmap.width() {
            return Some(ChromeButton::NewTab);
        }
        
        let tab_area_start = config.new_tab_button_size;
        if x >= tab_area_start {
            return Some(ChromeButton::AddressBar);
        }
        
        Some(ChromeButton::Menu)
    }

    /// 测试工具栏点击
    fn hit_test_toolbar(&self, x: u32, _toolbar_y: u32) -> Option<ChromeButton> {
        let config = &self.config;
        
        let back_x = 8;
        if x >= back_x && x < back_x + config.button_size {
            return Some(ChromeButton::Back);
        }
        
        let forward_x = back_x + config.button_size as u32 + 4;
        if x >= forward_x && x < forward_x + config.button_size {
            return Some(ChromeButton::Forward);
        }
        
        let reload_x = forward_x + config.button_size + 4;
        if x >= reload_x && x < reload_x + config.button_size {
            return Some(ChromeButton::Reload);
        }
        
        let home_x = reload_x + config.button_size + 4;
        if x >= home_x && x < home_x + config.button_size {
            return Some(ChromeButton::Home);
        }
        
        let address_bar_x = home_x + config.button_size + 12;
        let address_bar_width = self.pixmap.width() - address_bar_x - 80;
        if x >= address_bar_x && x < address_bar_x + address_bar_width {
            return Some(ChromeButton::AddressBar);
        }
        
        let menu_x = self.pixmap.width() - 48;
        if x >= menu_x {
            return Some(ChromeButton::Menu);
        }
        
        None
    }

    /// 获取像素图
    pub fn pixmap(&self) -> &Pixmap {
        &self.pixmap
    }

    /// 获取 PNG 数据
    pub fn to_png(&self) -> Vec<u8> {
        self.pixmap.encode_png().unwrap_or_default()
    }

    /// 保存为 PNG
    pub fn save_png(&self, path: &str) -> Result<(), Box<dyn std::error::Error>> {
        self.pixmap.save_png(path)?;
        Ok(())
    }

    /// 合成页面内容
    pub fn composite_page(&mut self, page_pixmap: &Pixmap, _content_x: u32, _content_y: u32) {
        let ui_height = self.ui_height();
        let dest_y = ui_height as f32;
        
        let copy_height = page_pixmap.height().min(self.pixmap.height().saturating_sub(ui_height));
        let copy_width = page_pixmap.width().min(self.pixmap.width());
        
        for y in 0..copy_height {
            for x in 0..copy_width {
                if let Some(pixel) = page_pixmap.pixel(x, y) {
                    let mut paint = Paint::default();
                    paint.set_color_rgba8(pixel.red(), pixel.green(), pixel.blue(), pixel.alpha());
                    
                    if let Some(rect) = Rect::from_xywh(x as f32, dest_y + y as f32, 1.0, 1.0) {
                        self.pixmap.fill_rect(rect, &paint, Transform::identity(), None);
                    }
                }
            }
        }
    }
}
