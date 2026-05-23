//! GUI Window Manager — 跨平台窗口共享状态
//!
//! 管理窗口间共享的像素缓冲区和滚动事件状态。
//! 由 eframe 事件循环驱动。

use log::info;
use std::sync::{Arc, Mutex};

// ── 常量 ──

const TOOLBAR_HEIGHT: u32 = 80;
const STATUS_BAR_HEIGHT: u32 = 28;

// ── 共享状态 ──

pub struct SharedState {
    pub rgba_pixels: Option<(u32, u32, Vec<u8>)>,
    pub main_size: (u32, u32),
    pub needs_redraw: bool,
    pub pending_scroll: Option<(f32, f32)>,
}

impl SharedState {
    pub fn new() -> Self {
        Self {
            rgba_pixels: None,
            main_size: (1280, 720),
            needs_redraw: true,
            pending_scroll: None,
        }
    }
}

// ── 窗口管理器 ──

pub struct WindowManager {
    pub state: Arc<Mutex<SharedState>>,
}

impl WindowManager {
    /// 创建窗口管理器
    pub fn create(screen_w: u32, screen_h: u32) -> Self {
        let main_w = screen_w;
        let main_h = screen_h
            .saturating_sub(TOOLBAR_HEIGHT)
            .saturating_sub(STATUS_BAR_HEIGHT);
        
        info!("创建窗口管理器: {}x{}", main_w, main_h);
        
        let state = Arc::new(Mutex::new(SharedState::new()));
        {
            let mut s = state.lock().unwrap();
            s.main_size = (main_w, main_h);
        }
        Self { state }
    }
    
    pub fn update_pixels(&self, w: u32, h: u32, rgba: Vec<u8>) {
        let mut s = self.state.lock().unwrap();
        s.rgba_pixels = Some((w, h, rgba));
        s.needs_redraw = true;
    }
}
