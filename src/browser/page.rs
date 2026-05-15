//! Page 模块 - 页面管理和状态
//!
//! 管理页面的生命周期和状态

use log::{debug, info};

/// 页面加载状态
#[derive(Debug, Clone, PartialEq)]
pub enum LoadState {
    /// 空闲状态
    Idle,
    /// 正在加载
    Loading,
    /// 加载完成
    Loaded,
    /// 加载失败
    Error(String),
}

impl Default for LoadState {
    fn default() -> Self {
        Self::Idle
    }
}

/// 页面信息
#[derive(Debug, Clone)]
pub struct Page {
    /// 页面 URL
    url: String,
    /// 页面标题
    title: Option<String>,
    /// 加载状态
    state: LoadState,
    /// 缩放比例
    zoom: f32,
    /// 是否可编辑
    editable: bool,
}

impl Page {
    /// 创建新页面
    pub fn new(url: &str) -> Self {
        info!("创建新页面: {}", url);
        Self {
            url: url.to_string(),
            title: None,
            state: LoadState::Idle,
            zoom: 1.0,
            editable: false,
        }
    }

    /// 获取 URL
    pub fn url(&self) -> &str {
        &self.url
    }

    /// 设置标题
    pub fn set_title(&mut self, title: &str) {
        debug!("设置页面标题: {}", title);
        self.title = Some(title.to_string());
    }

    /// 获取标题
    pub fn title(&self) -> Option<&str> {
        self.title.as_deref()
    }

    /// 设置加载状态
    pub fn set_state(&mut self, state: LoadState) {
        debug!("页面状态变更: {:?}", state);
        self.state = state;
    }

    /// 获取加载状态
    pub fn state(&self) -> &LoadState {
        &self.state
    }

    /// 设置缩放
    pub fn set_zoom(&mut self, zoom: f32) {
        debug!("设置缩放: {}", zoom);
        self.zoom = zoom.clamp(0.1, 10.0);
    }

    /// 获取缩放
    pub fn zoom(&self) -> f32 {
        self.zoom
    }

    /// 放大
    pub fn zoom_in(&mut self) {
        self.set_zoom(self.zoom * 1.25);
    }

    /// 缩小
    pub fn zoom_out(&mut self) {
        self.set_zoom(self.zoom / 1.25);
    }

    /// 重置缩放
    pub fn reset_zoom(&mut self) {
        self.zoom = 1.0;
    }

    /// 设置可编辑
    pub fn set_editable(&mut self, editable: bool) {
        debug!("设置可编辑: {}", editable);
        self.editable = editable;
    }

    /// 是否可编辑
    pub fn is_editable(&self) -> bool {
        self.editable
    }
}

impl Default for Page {
    fn default() -> Self {
        Self::new("about:blank")
    }
}
