//! Painter - 使用 tiny-skia 绘制
//!
//! 将布局结果绘制到像素图。提供高性能的 2D 绘制原语。

use crate::css::values::Color;
use log::trace;
use tiny_skia::{Paint, PathBuilder, Pixmap, Rect, Stroke, Transform};

pub struct Painter {
    pixmap: Pixmap,
    background: Color,
}

impl Painter {
    pub fn new(width: u32, height: u32) -> Option<Self> {
        let pixmap = Pixmap::new(width, height)?;
        trace!("创建绘制器 ({}x{})", width, height);
        Some(Self {
            pixmap,
            background: Color::WHITE,
        })
    }

    pub fn set_background(&mut self, color: Color) {
        self.background = color;
    }

    pub fn paint(&mut self) {
        let c = self.background.to_rgba();
        self.pixmap
            .fill(tiny_skia::Color::from_rgba8(c[0], c[1], c[2], c[3]));
    }

    // ═══════════════════════════════════════════════════
    // 矩形填充
    // ═══════════════════════════════════════════════════

    pub fn draw_rect(&mut self, x: f32, y: f32, w: f32, h: f32, color: &Color) {
        if w <= 0.0 || h <= 0.0 {
            return;
        }
        let rgba = color.to_rgba();
        let mut paint = Paint::default();
        paint.set_color_rgba8(rgba[0], rgba[1], rgba[2], rgba[3]);
        if let Some(rect) = Rect::from_xywh(x, y, w, h) {
            self.pixmap
                .fill_rect(rect, &paint, Transform::identity(), None);
        }
    }

    /// 绘制圆角矩形填充
    pub fn draw_rounded_rect(&mut self, x: f32, y: f32, w: f32, h: f32, r: f32, color: &Color) {
        if w <= 0.0 || h <= 0.0 {
            return;
        }
        let rgba = color.to_rgba();
        let mut paint = Paint::default();
        paint.set_color_rgba8(rgba[0], rgba[1], rgba[2], rgba[3]);
        paint.anti_alias = true;
        if let Some(path) = rounded_rect_path(x, y, w, h, r) {
            self.pixmap.fill_path(
                &path,
                &paint,
                FillRule::Winding,
                Transform::identity(),
                None,
            );
        }
    }

    // ═══════════════════════════════════════════════════
    // 边框
    // ═══════════════════════════════════════════════════

    /// 绘制矩形边框（四条边）
    pub fn draw_border(&mut self, x: f32, y: f32, w: f32, h: f32, bw: f32, color: &Color) {
        if w <= 0.0 || h <= 0.0 || bw <= 0.0 {
            return;
        }
        let rgba = color.to_rgba();
        let mut paint = Paint::default();
        paint.set_color_rgba8(rgba[0], rgba[1], rgba[2], rgba[3]);
        paint.anti_alias = true;
        if let Some(r) = Rect::from_xywh(x, y, w, bw) {
            self.pixmap
                .fill_rect(r, &paint, Transform::identity(), None);
        }
        if let Some(r) = Rect::from_xywh(x, y + h - bw, w, bw) {
            self.pixmap
                .fill_rect(r, &paint, Transform::identity(), None);
        }
        if let Some(r) = Rect::from_xywh(x, y, bw, h) {
            self.pixmap
                .fill_rect(r, &paint, Transform::identity(), None);
        }
        if let Some(r) = Rect::from_xywh(x + w - bw, y, bw, h) {
            self.pixmap
                .fill_rect(r, &paint, Transform::identity(), None);
        }
    }

    /// 绘制圆角矩形边框
    pub fn draw_rounded_border(
        &mut self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        r: f32,
        bw: f32,
        color: &Color,
    ) {
        if w <= 0.0 || h <= 0.0 || bw <= 0.0 {
            return;
        }
        let rgba = color.to_rgba();
        let mut paint = Paint::default();
        paint.set_color_rgba8(rgba[0], rgba[1], rgba[2], rgba[3]);
        paint.anti_alias = true;
        let stroke = Stroke {
            width: bw,
            ..Default::default()
        };
        if let Some(path) = rounded_rect_path(x, y, w, h, r) {
            self.pixmap
                .stroke_path(&path, &paint, &stroke, Transform::identity(), None);
        }
    }

    /// 绘制矩形边框（用于 hover 高亮等场景，别名）
    pub fn draw_rect_border(&mut self, x: f32, y: f32, w: f32, h: f32, bw: f32, color: &Color) {
        self.draw_border(x, y, w, h, bw, color);
    }

    /// 绘制分割线（水平）
    pub fn draw_hr(&mut self, x: f32, y: f32, w: f32, color: &Color) {
        self.draw_rect(x, y, w, 1.0, color);
    }

    // ═══════════════════════════════════════════════════
    // 渐变
    // ═══════════════════════════════════════════════════

    /// 绘制水平渐变矩形
    pub fn draw_gradient_h(&mut self, x: f32, y: f32, w: f32, h: f32, c1: &Color, c2: &Color) {
        if w <= 0.0 || h <= 0.0 {
            return;
        }
        use tiny_skia::LinearGradient;
        let s = c1.to_rgba();
        let e = c2.to_rgba();
        let stop0 =
            tiny_skia::GradientStop::new(0.0, tiny_skia::Color::from_rgba8(s[0], s[1], s[2], s[3]));
        let stop1 =
            tiny_skia::GradientStop::new(1.0, tiny_skia::Color::from_rgba8(e[0], e[1], e[2], e[3]));
        if let Some(grad) = LinearGradient::new(
            tiny_skia::Point::from_xy(x, y),
            tiny_skia::Point::from_xy(x + w, y),
            vec![stop0, stop1],
            tiny_skia::SpreadMode::Pad,
            tiny_skia::Transform::identity(),
        ) {
            let mut paint = Paint::default();
            paint.shader = grad;
            if let Some(rect) = Rect::from_xywh(x, y, w, h) {
                self.pixmap
                    .fill_rect(rect, &paint, Transform::identity(), None);
            }
        }
    }

    // ═══════════════════════════════════════════════════
    // 工具
    // ═══════════════════════════════════════════════════

    pub fn pixmap(&self) -> &Pixmap {
        &self.pixmap
    }
    pub fn pixmap_mut(&mut self) -> &mut Pixmap {
        &mut self.pixmap
    }

    pub fn to_png(&self) -> Vec<u8> {
        self.pixmap.encode_png().unwrap_or_default()
    }

    pub fn save_png(&self, path: &str) -> Result<(), Box<dyn std::error::Error>> {
        self.pixmap.save_png(path)?;
        Ok(())
    }

    pub fn set_viewport(&mut self, width: u32, height: u32) {
        if let Some(new_pixmap) = Pixmap::new(width, height) {
            self.pixmap = new_pixmap;
        }
    }
}

/// 构建圆角矩形路径
fn rounded_rect_path(x: f32, y: f32, w: f32, h: f32, r: f32) -> Option<tiny_skia::Path> {
    let r = r.min(w / 2.0).min(h / 2.0);
    let mut pb = PathBuilder::new();
    if r <= 0.0 {
        if let Some(rect) = tiny_skia::Rect::from_xywh(x, y, w, h) {
            pb.push_rect(rect);
        }
    } else {
        pb.move_to(x + r, y);
        pb.line_to(x + w - r, y);
        pb.cubic_to(x + w - r * 0.5, y, x + w, y + r * 0.5, x + w, y + r);
        pb.line_to(x + w, y + h - r);
        pb.cubic_to(
            x + w,
            y + h - r * 0.5,
            x + w - r * 0.5,
            y + h,
            x + w - r,
            y + h,
        );
        pb.line_to(x + r, y + h);
        pb.cubic_to(x + r * 0.5, y + h, x, y + h - r * 0.5, x, y + h - r);
        pb.line_to(x, y + r);
        pb.cubic_to(x, y + r * 0.5, x + r * 0.5, y, x + r, y);
    }
    pb.close();
    pb.finish()
}

use tiny_skia::FillRule;
