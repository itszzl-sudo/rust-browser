//! Render Context - 渲染上下文
//!
//! 管理渲染所需的所有状态和资源

use crate::css::stylesheet::Stylesheet;
use crate::dom::node::{DomTree, NodeType};
use log::{debug, info, trace};
use parking_lot::RwLock;
use std::sync::Arc;
use tiny_skia::Pixmap;

/// 渲染上下文
pub struct RenderContext {
    /// 视口宽度
    width: u32,
    /// 视口高度
    height: u32,
    /// 像素图
    pixmap: Arc<RwLock<Option<Pixmap>>>,
    /// DOM 树
    dom: Arc<RwLock<DomTree>>,
    /// 样式表
    stylesheet: Arc<RwLock<Stylesheet>>,
    /// 缩放比例
    scale: f32,
    /// 调试模式
    debug: bool,
}

impl RenderContext {
    /// 创建新的渲染上下文
    pub fn new(width: u32, height: u32) -> Self {
        info!("创建渲染上下文 ({}x{})", width, height);

        Self {
            width,
            height,
            pixmap: Arc::new(RwLock::new(None)),
            dom: Arc::new(RwLock::new(DomTree::new())),
            stylesheet: Arc::new(RwLock::new(Stylesheet::default())),
            scale: 1.0,
            debug: false,
        }
    }

    /// 设置视口尺寸
    pub fn set_viewport(&mut self, width: u32, height: u32) {
        debug!("设置视口: {}x{}", width, height);
        self.width = width;
        self.height = height;
    }

    /// 获取视口尺寸
    pub fn viewport(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// 获取像素图
    pub fn pixmap(&self) -> Arc<RwLock<Option<Pixmap>>> {
        self.pixmap.clone()
    }

    /// 创建像素图
    pub fn create_pixmap(&self) -> Option<Pixmap> {
        Pixmap::new(self.width, self.height)
    }

    /// 获取 DOM 树
    pub fn dom(&self) -> Arc<RwLock<DomTree>> {
        self.dom.clone()
    }

    /// 设置 DOM 树
    pub fn set_dom(&self, dom: DomTree) {
        *self.dom.write() = dom;
    }

    /// 获取样式表
    pub fn stylesheet(&self) -> Arc<RwLock<Stylesheet>> {
        self.stylesheet.clone()
    }

    /// 设置样式表
    pub fn set_stylesheet(&self, stylesheet: Stylesheet) {
        *self.stylesheet.write() = stylesheet;
    }

    /// 设置缩放
    pub fn set_scale(&mut self, scale: f32) {
        debug!("设置缩放: {}", scale);
        self.scale = scale.clamp(0.1, 10.0);
    }

    /// 获取缩放
    pub fn scale(&self) -> f32 {
        self.scale
    }

    /// 启用/禁用调试模式
    pub fn set_debug(&mut self, debug: bool) {
        debug!("调试模式: {}", debug);
        self.debug = debug;
    }

    /// 是否调试模式
    pub fn is_debug(&self) -> bool {
        self.debug
    }
}

impl Default for RenderContext {
    fn default() -> Self {
        Self::new(1280, 720)
    }
}
