//! Compositor — 合成层
//!
//! 负责将浏览器原生 UI 层与 Web 页面渲染层合并为最终输出。
//! 类似 Chrome 的 Compositor + ARC 的"双独立图层"。
//!
//! # 架构
//!
//! ```text
//! ┌─────────────────────────────────────────────┐
//! │              CompositorFrame                │
//! │  ┌─────────────────────────────────────┐    │
//! │  │  Web 页面渲染 (固定视口，不感知resize) │   │
//! │  └─────────────────────────────────────┘    │
//! │  ┌─────────────────────────────────────┐    │
//! │  │  原生 UI 层 (标签栏/地址栏/侧栏)     │   │
//! │  └─────────────────────────────────────┘    │
//! └─────────────────────────────────────────────┘
//! ```
//!
//! 核心原则：
//! 1. Web 页面始终按固定视口渲染，不因窗口 resize 触发全局重布局
//! 2. UI 层走独立的 tiny-skia 绘制，与 Web 渲染上下文彻底隔离
//! 3. 合成层仅在最终输出时合并两层 RGBA

use tiny_skia;

/// 合成层帧 — 包含 UI 层和 Web 层的像素数据
pub struct CompositorFrame {
    /// Web 页面渲染结果 (RGBA)
    pub web_rgba: Option<Vec<u8>>,
    /// Web 页面尺寸
    pub web_width: u32,
    pub web_height: u32,
    /// UI 层渲染结果 (RGBA)
    pub ui_rgba: Option<Vec<u8>>,
    /// UI 层尺寸
    pub ui_width: u32,
    pub ui_height: u32,
    /// 最终合成结果
    pub final_pixmap: tiny_skia::Pixmap,
}

impl CompositorFrame {
    /// 创建新的合成帧
    pub fn new(total_width: u32, total_height: u32) -> Self {
        let pixmap = tiny_skia::Pixmap::new(total_width, total_height)
            .unwrap_or_else(|| tiny_skia::Pixmap::new(1, 1).unwrap());
        Self {
            web_rgba: None,
            web_width: 0,
            web_height: 0,
            ui_rgba: None,
            ui_width: 0,
            ui_height: 0,
            final_pixmap: pixmap,
        }
    }

    /// 设置 Web 页面内容
    pub fn set_web_layer(&mut self, rgba: Vec<u8>, width: u32, height: u32) {
        self.web_rgba = Some(rgba);
        self.web_width = width;
        self.web_height = height;
    }

    /// 设置 UI 层内容
    pub fn set_ui_layer(&mut self, rgba: Vec<u8>, width: u32, height: u32) {
        self.ui_rgba = Some(rgba);
        self.ui_width = width;
        self.ui_height = height;
    }

    /// 合成所有图层到最终输出
    pub fn composite(&mut self) {
        // 1. 清空为白色背景
        self.final_pixmap.fill(tiny_skia::Color::WHITE);

        // 2. 绘制 Web 页面层（下层）
        if self.web_rgba.is_some() {
            let rgba = self.web_rgba.take().unwrap();
            let sw = self.web_width;
            let sh = self.web_height;
            self.blit_rgba(&rgba, sw, sh, 0, 0);
            self.web_rgba = Some(rgba);
        }

        // 3. 绘制 UI 层（上层，覆盖在 Web 内容之上）
        if self.ui_rgba.is_some() {
            let rgba = self.ui_rgba.take().unwrap();
            let sw = self.ui_width;
            let sh = self.ui_height;
            self.blit_rgba(&rgba, sw, sh, 0, 0);
            self.ui_rgba = Some(rgba);
        }
    }

    /// 将 RGBA 数据 blit 到 final_pixmap 的指定位置
    fn blit_rgba(&mut self, rgba: &[u8], src_w: u32, src_h: u32, dst_x: i32, dst_y: i32) {
        let fw = self.final_pixmap.width() as i32;
        let fh = self.final_pixmap.height() as i32;

        for y in 0..src_h as i32 {
            for x in 0..src_w as i32 {
                let px = dst_x + x;
                let py = dst_y + y;
                if px < 0 || px >= fw || py < 0 || py >= fh {
                    continue;
                }
                let si = ((y * src_w as i32 + x) * 4) as usize;
                if si + 3 >= rgba.len() {
                    continue;
                }
                let r = rgba[si];
                let g = rgba[si + 1];
                let b = rgba[si + 2];
                let a = rgba[si + 3];
                let di = ((py * fw + px) * 4) as usize;
                // alpha 混合
                if a == 255 {
                    self.final_pixmap.data_mut()[di] = r;
                    self.final_pixmap.data_mut()[di + 1] = g;
                    self.final_pixmap.data_mut()[di + 2] = b;
                    self.final_pixmap.data_mut()[di + 3] = 255;
                } else if a > 0 {
                    let alpha = a as f32 / 255.0;
                    let inv_alpha = 1.0 - alpha;
                    self.final_pixmap.data_mut()[di] = (r as f32 * alpha
                        + self.final_pixmap.data_mut()[di] as f32 * inv_alpha)
                        as u8;
                    self.final_pixmap.data_mut()[di + 1] = (g as f32 * alpha
                        + self.final_pixmap.data_mut()[di + 1] as f32 * inv_alpha)
                        as u8;
                    self.final_pixmap.data_mut()[di + 2] = (b as f32 * alpha
                        + self.final_pixmap.data_mut()[di + 2] as f32 * inv_alpha)
                        as u8;
                    self.final_pixmap.data_mut()[di + 3] = 255;
                }
            }
        }
    }

    /// 获取最终合成的 PNG
    pub fn to_png(&self) -> Vec<u8> {
        self.final_pixmap.encode_png().unwrap_or_default()
    }

    /// 获取最终合成的 RGBA 像素
    pub fn to_rgba(&self) -> Vec<u8> {
        self.final_pixmap.data().to_vec()
    }
}
