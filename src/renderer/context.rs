//! Render Context - 渲染上下文
//!
//! 管理渲染所需的所有状态和资源

use crate::DomWrapper;
use log::{debug, info};
use parking_lot::RwLock;
use std::sync::Arc;
use tiny_skia::Pixmap;

pub struct RenderContext {
    width: u32,
    height: u32,
    pixmap: Arc<RwLock<Option<Pixmap>>>,
    dom: Arc<RwLock<Option<DomWrapper>>>,
    scale: f32,
    debug: bool,
}

impl RenderContext {
    pub fn new(width: u32, height: u32) -> Self {
        info!("创建渲染上下文 ({}x{})", width, height);

        Self {
            width,
            height,
            pixmap: Arc::new(RwLock::new(None)),
            dom: Arc::new(RwLock::new(None)),
            scale: 1.0,
            debug: false,
        }
    }

    pub fn set_viewport(&mut self, width: u32, height: u32) {
        debug!("设置视口: {}x{}", width, height);
        self.width = width;
        self.height = height;
    }

    pub fn viewport(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    pub fn pixmap(&self) -> Arc<RwLock<Option<Pixmap>>> {
        self.pixmap.clone()
    }

    pub fn create_pixmap(&self) -> Option<Pixmap> {
        Pixmap::new(self.width, self.height)
    }

    pub fn dom(&self) -> Arc<RwLock<Option<DomWrapper>>> {
        self.dom.clone()
    }

    pub fn set_dom(&self, dom: DomWrapper) {
        *self.dom.write() = Some(dom);
    }

    pub fn set_scale(&mut self, scale: f32) {
        debug!("设置缩放: {}", scale);
        self.scale = scale.clamp(0.1, 10.0);
    }

    pub fn scale(&self) -> f32 {
        self.scale
    }

    pub fn set_debug(&mut self, debug: bool) {
        debug!("调试模式: {}", debug);
        self.debug = debug;
    }

    pub fn is_debug(&self) -> bool {
        self.debug
    }
}

impl Default for RenderContext {
    fn default() -> Self {
        Self::new(1280, 720)
    }
}
