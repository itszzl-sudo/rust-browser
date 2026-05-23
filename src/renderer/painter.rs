//! Painter - 使用 tiny-skia 绘制
//!
//! 将布局结果绘制到像素图。提供高性能的 2D 绘制原语。
//! 支持简单裁剪（overflow: hidden）：通过 clip_rect 记录当前裁剪区域，
//! 绘制时自动限制在裁剪区域内。

use crate::css::values::Color;
use log::trace;
use tiny_skia::{Paint, PathBuilder, Pixmap, Rect, Stroke, Transform};

/// 裁剪矩形
#[derive(Debug, Clone, Copy)]
pub struct ClipRect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

pub struct Painter {
    pixmap: Pixmap,
    background: Color,
    /// 当前裁剪区域（None = 不裁剪）
    pub clip_rect: Option<ClipRect>,
}

impl Painter {
    pub fn new(width: u32, height: u32) -> Option<Self> {
        let pixmap = Pixmap::new(width, height)?;
        trace!("创建绘制器 ({}x{})", width, height);
        Some(Self {
            pixmap,
            background: Color::WHITE,
            clip_rect: None,
        })
    }

    /// 设置裁剪区域
    pub fn set_clip(&mut self, x: f32, y: f32, w: f32, h: f32) {
        self.clip_rect = Some(ClipRect { x, y, w, h });
    }

    /// 清除裁剪区域
    pub fn clear_clip(&mut self) {
        self.clip_rect = None;
    }

    /// 将全局坐标裁剪到裁剪区域内，返回裁剪后的矩形 (x, y, w, h, 是否完全被裁剪掉)
    fn apply_clip(&self, x: f32, y: f32, w: f32, h: f32) -> Option<(f32, f32, f32, f32)> {
        match self.clip_rect {
            Some(clip) => {
                let cx = x.max(clip.x);
                let cy = y.max(clip.y);
                let cw = (x + w).min(clip.x + clip.w) - cx;
                let ch = (y + h).min(clip.y + clip.h) - cy;
                if cw <= 0.0 || ch <= 0.0 {
                    None
                } else {
                    Some((cx, cy, cw, ch))
                }
            }
            None => Some((x, y, w, h)),
        }
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
        let (cx, cy, cw, ch) = match self.apply_clip(x, y, w, h) {
            Some(r) => r,
            None => return,
        };
        let rgba = color.to_rgba();
        let mut paint = Paint::default();
        paint.set_color_rgba8(rgba[0], rgba[1], rgba[2], rgba[3]);
        if let Some(rect) = Rect::from_xywh(cx, cy, cw, ch) {
            self.pixmap
                .fill_rect(rect, &paint, Transform::identity(), None);
        }
    }

    /// 绘制圆角矩形填充
    pub fn draw_rounded_rect(&mut self, x: f32, y: f32, w: f32, h: f32, r: f32, color: &Color) {
        if w <= 0.0 || h <= 0.0 {
            return;
        }
        let (cx, cy, cw, ch) = match self.apply_clip(x, y, w, h) {
            Some(r) => r,
            None => return,
        };
        // 如果裁剪后的区域和原区域不同，退化为用 draw_rect（圆角可能不完整但视觉可接受）
        if (cx - x).abs() > 0.5
            || (cy - y).abs() > 0.5
            || (cw - w).abs() > 0.5
            || (ch - h).abs() > 0.5
        {
            return self.draw_rect(cx, cy, cw, ch, color);
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
        // 边框的每条边独立裁剪
        let rgba = color.to_rgba();
        let mut paint = Paint::default();
        paint.set_color_rgba8(rgba[0], rgba[1], rgba[2], rgba[3]);
        paint.anti_alias = true;
        // 上边
        if let Some((cx, cy, cw, _)) = self.apply_clip(x, y, w, bw) {
            if let Some(r) = Rect::from_xywh(cx, cy, cw, (y + bw) - cy) {
                self.pixmap
                    .fill_rect(r, &paint, Transform::identity(), None);
            }
        }
        // 下边
        if let Some((cx, cy, cw, _)) = self.apply_clip(x, y + h - bw, w, bw) {
            if let Some(r) = Rect::from_xywh(cx, cy, cw, (y + h) - cy) {
                self.pixmap
                    .fill_rect(r, &paint, Transform::identity(), None);
            }
        }
        // 左边
        if let Some((cx, cy, _, ch)) = self.apply_clip(x, y + bw, bw, h - bw * 2.0) {
            if let Some(r) = Rect::from_xywh(cx, cy, (x + bw) - cx, ch) {
                self.pixmap
                    .fill_rect(r, &paint, Transform::identity(), None);
            }
        }
        // 右边
        if let Some((cx, cy, _, ch)) = self.apply_clip(x + w - bw, y + bw, bw, h - bw * 2.0) {
            if let Some(r) = Rect::from_xywh(cx, cy, (x + w) - cx, ch) {
                self.pixmap
                    .fill_rect(r, &paint, Transform::identity(), None);
            }
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
        // 用 draw_border 实现（圆角边框被裁剪时退化为直角）
        if let Some((cx, cy, cw, ch)) = self.apply_clip(x, y, w, h) {
            if (cx - x).abs() > 0.5
                || (cy - y).abs() > 0.5
                || (cw - w).abs() > 0.5
                || (ch - h).abs() > 0.5
            {
                return self.draw_border(cx, cy, cw, ch, bw, color);
            }
        } else {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_painter() {
        let painter = Painter::new(100, 100);
        assert!(painter.is_some());
        let painter = painter.unwrap();
        assert_eq!(painter.pixmap().width(), 100);
        assert_eq!(painter.pixmap().height(), 100);
    }

    #[test]
    fn test_new_painter_zero_size() {
        let painter = Painter::new(0, 100);
        assert!(painter.is_none());
        let painter = Painter::new(100, 0);
        assert!(painter.is_none());
    }

    #[test]
    fn test_set_background_and_paint() {
        let mut painter = Painter::new(10, 10).unwrap();
        painter.set_background(Color::from_hex("#FF0000"));
        painter.paint();
        // 检查左上角像素是红色
        let pixel = painter.pixmap().pixel(0, 0).unwrap();
        assert_eq!(pixel.red(), 255);
        assert_eq!(pixel.green(), 0);
        assert_eq!(pixel.blue(), 0);
        assert_eq!(pixel.alpha(), 255);
    }

    #[test]
    fn test_paint_default_white() {
        let mut painter = Painter::new(10, 10).unwrap();
        painter.paint();
        let pixel = painter.pixmap().pixel(0, 0).unwrap();
        assert_eq!(pixel.red(), 255);
        assert_eq!(pixel.green(), 255);
        assert_eq!(pixel.blue(), 255);
    }

    #[test]
    fn test_draw_rect_fills_region() {
        let mut painter = Painter::new(20, 20).unwrap();
        painter.paint(); // 白色背景
        let red = Color::from_hex("#FF0000");
        painter.draw_rect(2.0, 2.0, 10.0, 10.0, &red);
        // 矩形内部应该是红色
        let pixel = painter.pixmap().pixel(3, 3).unwrap();
        assert_eq!(pixel.red(), 255);
        assert_eq!(pixel.green(), 0);
        assert_eq!(pixel.blue(), 0);
        // 矩形外部应该是白色
        let pixel = painter.pixmap().pixel(0, 0).unwrap();
        assert_eq!(pixel.red(), 255);
        assert_eq!(pixel.green(), 255);
        assert_eq!(pixel.blue(), 255);
    }

    #[test]
    fn test_draw_rect_zero_size_no_op() {
        let mut painter = Painter::new(10, 10).unwrap();
        painter.paint();
        let red = Color::from_hex("#FF0000");
        painter.draw_rect(2.0, 2.0, 0.0, 10.0, &red);
        painter.draw_rect(2.0, 2.0, 10.0, 0.0, &red);
        painter.draw_rect(2.0, 2.0, -1.0, 10.0, &red);
        // 全白）
        let pixel = painter.pixmap().pixel(3, 3).unwrap();
        assert_eq!(pixel.red(), 255);
        assert_eq!(pixel.green(), 255);
        assert_eq!(pixel.blue(), 255);
    }

    #[test]
    fn test_draw_rounded_rect() {
        let mut painter = Painter::new(30, 30).unwrap();
        painter.paint();
        let blue = Color::from_hex("#0000FF");
        painter.draw_rounded_rect(5.0, 5.0, 20.0, 20.0, 5.0, &blue);
        // 内部应该是蓝色
        let pixel = painter.pixmap().pixel(15, 15).unwrap();
        assert_eq!(pixel.red(), 0);
        assert_eq!(pixel.green(), 0);
        assert_eq!(pixel.blue(), 255);
        // 外部应该是白色
        let pixel = painter.pixmap().pixel(0, 0).unwrap();
        assert_eq!(pixel.red(), 255);
    }

    #[test]
    fn test_draw_rounded_rect_zero_size() {
        let mut painter = Painter::new(10, 10).unwrap();
        painter.paint();
        let blue = Color::from_hex("#0000FF");
        painter.draw_rounded_rect(5.0, 5.0, 0.0, 10.0, 5.0, &blue);
        painter.draw_rounded_rect(5.0, 5.0, 10.0, 0.0, 5.0, &blue);
        // 不 panic 即可
    }

    #[test]
    fn test_draw_border_four_sides() {
        let mut painter = Painter::new(20, 20).unwrap();
        painter.paint();
        let black = Color::from_hex("#000000");
        painter.draw_border(2.0, 2.0, 16.0, 16.0, 2.0, &black);
        // 上边框内部
        let pixel = painter.pixmap().pixel(5, 2).unwrap();
        assert_eq!(pixel.red(), 0);
        // 边框内部（没有填充，只是边框）
        let pixel = painter.pixmap().pixel(10, 10).unwrap();
        assert_eq!(pixel.red(), 255);
    }

    #[test]
    fn test_draw_border_zero_bw_no_op() {
        let mut painter = Painter::new(10, 10).unwrap();
        painter.paint();
        let black = Color::from_hex("#000000");
        painter.draw_border(2.0, 2.0, 6.0, 6.0, 0.0, &black);
        let pixel = painter.pixmap().pixel(5, 5).unwrap();
        assert_eq!(pixel.red(), 255);
    }

    #[test]
    fn test_draw_rounded_border() {
        let mut painter = Painter::new(30, 30).unwrap();
        painter.paint();
        let red = Color::from_hex("#FF0000");
        painter.draw_rounded_border(5.0, 5.0, 20.0, 20.0, 5.0, 2.0, &red);
        // 边框附近应该有红色像素
        let pixel = painter.pixmap().pixel(5, 5).unwrap();
        assert_eq!(pixel.red(), 255);
    }

    #[test]
    fn test_draw_rounded_border_zero_size() {
        let mut painter = Painter::new(10, 10).unwrap();
        painter.paint();
        let red = Color::from_hex("#FF0000");
        painter.draw_rounded_border(5.0, 5.0, 0.0, 10.0, 5.0, 2.0, &red);
        painter.draw_rounded_border(5.0, 5.0, 10.0, 0.0, 5.0, 2.0, &red);
        // 不 panic
    }

    #[test]
    fn test_draw_rect_border_alias() {
        let mut painter = Painter::new(20, 20).unwrap();
        painter.paint();
        let black = Color::from_hex("#000000");
        painter.draw_rect_border(2.0, 2.0, 16.0, 16.0, 2.0, &black);
        // 结果应该和 draw_border 一致
        let pixel = painter.pixmap().pixel(5, 2).unwrap();
        assert_eq!(pixel.red(), 0);
    }

    #[test]
    fn test_draw_hr() {
        let mut painter = Painter::new(20, 20).unwrap();
        painter.paint();
        let gray = Color::from_hex("#888888");
        painter.draw_hr(2.0, 10.0, 16.0, &gray);
        // 分割线上应该是灰色
        let pixel = painter.pixmap().pixel(5, 10).unwrap();
        assert_eq!(pixel.red(), 0x88);
        assert_eq!(pixel.green(), 0x88);
        assert_eq!(pixel.blue(), 0x88);
        // 分割线外应该是白色
        let pixel = painter.pixmap().pixel(5, 9).unwrap();
        assert_eq!(pixel.red(), 255);
    }

    #[test]
    fn test_draw_gradient_h() {
        let mut painter = Painter::new(20, 20).unwrap();
        painter.paint();
        let black = Color::from_hex("#000000");
        let white = Color::from_hex("#FFFFFF");
        painter.draw_gradient_h(2.0, 2.0, 16.0, 16.0, &black, &white);
        // 左侧应该是更接近黑色
        let left = painter.pixmap().pixel(3, 10).unwrap();
        // 右侧应该是更接近白色
        let right = painter.pixmap().pixel(16, 10).unwrap();
        // 左侧暗于右侧
        assert!(left.red() < right.red());
    }

    #[test]
    fn test_draw_gradient_h_zero_size() {
        let mut painter = Painter::new(10, 10).unwrap();
        painter.paint();
        let black = Color::from_hex("#000000");
        let white = Color::from_hex("#FFFFFF");
        painter.draw_gradient_h(2.0, 2.0, 0.0, 10.0, &black, &white);
        painter.draw_gradient_h(2.0, 2.0, 10.0, 0.0, &black, &white);
        // 不 panic
    }

    #[test]
    fn test_set_viewport() {
        let mut painter = Painter::new(100, 100).unwrap();
        assert_eq!(painter.pixmap().width(), 100);
        assert_eq!(painter.pixmap().height(), 100);
        painter.set_viewport(50, 50);
        assert_eq!(painter.pixmap().width(), 50);
        assert_eq!(painter.pixmap().height(), 50);
    }

    #[test]
    fn test_set_viewport_failure_keeps_old() {
        let mut painter = Painter::new(100, 100).unwrap();
        painter.set_viewport(0, 50);
        // 如果创建失败，保留原有的
        assert_eq!(painter.pixmap().width(), 100);
    }

    #[test]
    fn test_pixmap_mut() {
        let mut painter = Painter::new(10, 10).unwrap();
        let ppm = painter.pixmap_mut();
        assert_eq!(ppm.width(), 10);
        ppm.fill(tiny_skia::Color::from_rgba8(255, 0, 0, 255));
        let pixel = painter.pixmap().pixel(0, 0).unwrap();
        assert_eq!(pixel.red(), 255);
    }

    #[test]
    fn test_to_png() {
        let painter = Painter::new(5, 5).unwrap();
        let png = painter.to_png();
        assert!(!png.is_empty());
        // PNG header
        assert_eq!(png[0], 0x89);
        assert_eq!(png[1], b'P');
        assert_eq!(png[2], b'N');
        assert_eq!(png[3], b'G');
    }

    #[test]
    fn test_draw_rect_multiple_colors() {
        let mut painter = Painter::new(20, 20).unwrap();
        painter.paint();
        let red = Color::from_hex("#FF0000");
        let blue = Color::from_hex("#0000FF");
        painter.draw_rect(2.0, 2.0, 8.0, 8.0, &red);
        painter.draw_rect(10.0, 10.0, 8.0, 8.0, &blue);
        let pixel = painter.pixmap().pixel(5, 5).unwrap();
        assert_eq!(pixel.red(), 255);
        assert_eq!(pixel.blue(), 0);
        let pixel = painter.pixmap().pixel(15, 15).unwrap();
        assert_eq!(pixel.red(), 0);
        assert_eq!(pixel.blue(), 255);
    }

    #[test]
    fn test_draw_rect_out_of_bounds() {
        let mut painter = Painter::new(10, 10).unwrap();
        painter.paint();
        let red = Color::from_hex("#FF0000");
        // 绘制超出像素图范围的矩形，不 panic
        painter.draw_rect(-5.0, -5.0, 100.0, 100.0, &red);
    }

    #[test]
    fn test_draw_border_negative_values() {
        let mut painter = Painter::new(10, 10).unwrap();
        painter.paint();
        let black = Color::from_hex("#000000");
        // 负的边框宽度不 panic
        painter.draw_border(2.0, 2.0, 6.0, 6.0, -1.0, &black);
        // 负的位置不 panic
        painter.draw_border(-5.0, -5.0, 20.0, 20.0, 2.0, &black);
    }

    #[test]
    fn test_draw_rounded_rect_large_radius() {
        let mut painter = Painter::new(20, 20).unwrap();
        painter.paint();
        let blue = Color::from_hex("#0000FF");
        // 半径大于宽/高，应该被 clamp
        painter.draw_rounded_rect(2.0, 2.0, 10.0, 10.0, 20.0, &blue);
        let pixel = painter.pixmap().pixel(7, 7).unwrap();
        assert_eq!(pixel.blue(), 255);
    }

    #[test]
    fn test_gradient_h_black_to_white() {
        let mut painter = Painter::new(30, 10).unwrap();
        painter.paint();
        let black = Color::from_hex("#000000");
        let white = Color::from_hex("#FFFFFF");
        painter.draw_gradient_h(0.0, 0.0, 30.0, 10.0, &black, &white);
        // 最左应该接近黑
        let left = painter.pixmap().pixel(1, 5).unwrap();
        // 最右应该接近白
        let right = painter.pixmap().pixel(28, 5).unwrap();
        assert!(left.red() < right.red());
        assert!(left.green() < right.green());
        assert!(left.blue() < right.blue());
    }
}
