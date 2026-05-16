//! Painter - 使用 tiny-skia 绘制
//!
//! 将布局结果绘制到像素图

use crate::css::values::Color;
use crate::renderer::layout::LayoutEngine;
use log::{debug, trace};
use tiny_skia::{Paint, Pixmap, Rect, Transform};

pub struct Painter {
    pixmap: Pixmap,
    layout_engine: LayoutEngine,
    background: Color,
}

impl Painter {
    pub fn new(width: u32, height: u32) -> Option<Self> {
        let pixmap = Pixmap::new(width, height)?;
        let layout_engine = LayoutEngine::new();

        debug!("创建绘制器 ({}x{})", width, height);

        Some(Self {
            pixmap,
            layout_engine,
            background: Color::WHITE,
        })
    }

    pub fn set_background(&mut self, color: Color) {
        self.background = color;
    }

    pub fn paint(&mut self) {
        trace!("开始绘制");
        self.fill_background();
    }

    fn fill_background(&mut self) {
        let color = self.background.to_rgba();

        self.pixmap.fill(tiny_skia::Color::from_rgba8(color[0], color[1], color[2], color[3]));
    }

    pub fn draw_rect(&mut self, x: f32, y: f32, width: f32, height: f32, color: &Color) {
        let rgba = color.to_rgba();

        let mut paint = Paint::default();
        paint.set_color_rgba8(rgba[0], rgba[1], rgba[2], rgba[3]);

        if let Some(rect) = Rect::from_xywh(x, y, width, height) {
            self.pixmap.fill_rect(rect, &paint, Transform::identity(), None);
        }
    }

    pub fn paint_rect(&mut self, x: f32, y: f32, width: f32, height: f32, color: &Color) {
        self.draw_rect(x, y, width, height, color);
    }

    pub fn layout_engine(&self) -> &LayoutEngine {
        &self.layout_engine
    }

    pub fn layout_engine_mut(&mut self) -> &mut LayoutEngine {
        &mut self.layout_engine
    }

    pub fn pixmap(&self) -> &Pixmap {
        &self.pixmap
    }

    pub fn to_png(&self) -> Vec<u8> {
        self.pixmap.encode_png().unwrap_or_default()
    }

    pub fn save_png(&self, path: &str) -> Result<(), Box<dyn std::error::Error>> {
        self.pixmap.save_png(path)?;
        Ok(())
    }
}
