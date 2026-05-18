//! Border 绘制模块
//!
//! 使用 tiny-skia 的 `PathBuilder` + `stroke_path()` 实现 CSS border 绘制

use crate::css::values::Color;
use tiny_skia::{Paint, PathBuilder, Stroke, StrokeDash, Transform};

/// 边框线条样式
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BorderLineStyle {
    /// 实线
    Solid,
    /// 虚线
    Dashed,
    /// 点线
    Dotted,
    /// 无边框
    None,
}

impl BorderLineStyle {
    /// 是否可见
    pub fn is_visible(&self) -> bool {
        !matches!(self, BorderLineStyle::None)
    }
}

/// 单个边框边的样式
#[derive(Debug, Clone)]
pub struct BorderSide {
    /// 边框宽度（px）
    pub width: f32,
    /// 边框颜色
    pub color: Color,
    /// 边框线条样式
    pub style: BorderLineStyle,
}

impl BorderSide {
    /// 创建默认的不可见边框
    pub fn none() -> Self {
        Self {
            width: 0.0,
            color: Color::BLACK,
            style: BorderLineStyle::None,
        }
    }

    /// 创建实线边框
    pub fn solid(width: f32, color: Color) -> Self {
        Self {
            width,
            color,
            style: BorderLineStyle::Solid,
        }
    }
}

impl Default for BorderSide {
    fn default() -> Self {
        Self::none()
    }
}

/// 完整的 CSS 边框样式
#[derive(Debug, Clone)]
pub struct BorderStyle {
    /// 上边框
    pub top: BorderSide,
    /// 右边框
    pub right: BorderSide,
    /// 下边框
    pub bottom: BorderSide,
    /// 左边框
    pub left: BorderSide,
    /// 圆角半径（简化：统一圆角）
    pub radius: f32,
}

impl BorderStyle {
    /// 创建无线边框
    pub fn none() -> Self {
        Self {
            top: BorderSide::none(),
            right: BorderSide::none(),
            bottom: BorderSide::none(),
            left: BorderSide::none(),
            radius: 0.0,
        }
    }

    /// 创建四边一致的实线边框
    pub fn uniform(width: f32, color: Color) -> Self {
        Self {
            top: BorderSide::solid(width, color.clone()),
            right: BorderSide::solid(width, color.clone()),
            bottom: BorderSide::solid(width, color.clone()),
            left: BorderSide::solid(width, color),
            radius: 0.0,
        }
    }

    /// 是否有任何可见的边框
    pub fn has_visible_border(&self) -> bool {
        self.top.style.is_visible()
            || self.right.style.is_visible()
            || self.bottom.style.is_visible()
            || self.left.style.is_visible()
    }
}

impl Default for BorderStyle {
    fn default() -> Self {
        Self::none()
    }
}

/// 将 `BorderLineStyle` 转换为 tiny-skia 的 `LineCap` 和 `DashPattern`
fn dash_pattern_for_style(style: BorderLineStyle, width: f32) -> (tiny_skia::LineCap, Vec<f32>) {
    match style {
        BorderLineStyle::Solid => (tiny_skia::LineCap::Butt, vec![]),
        BorderLineStyle::Dashed => {
            // 虚线：线长 ≈ 3x 宽度，间隙 ≈ 2x 宽度
            (tiny_skia::LineCap::Butt, vec![width * 3.0, width * 2.0])
        }
        BorderLineStyle::Dotted => {
            // 点线：线长 ≈ 宽度，间隙 ≈ 宽度
            (tiny_skia::LineCap::Round, vec![width, width])
        }
        BorderLineStyle::None => (tiny_skia::LineCap::Butt, vec![]),
    }
}

/// 绘制单个边框边的直线路径
///
/// 绘制从 (x1, y1) 到 (x2, y2) 的线段作为边框
fn draw_border_side(
    pixmap: &mut tiny_skia::Pixmap,
    x1: f32,
    y1: f32,
    x2: f32,
    y2: f32,
    side: &BorderSide,
) {
    if !side.style.is_visible() || side.width <= 0.0 {
        return;
    }

    let rgba = side.color.to_rgba();
    let mut paint = Paint::default();
    paint.set_color_rgba8(rgba[0], rgba[1], rgba[2], rgba[3]);

    let (line_cap, dash_pattern) = dash_pattern_for_style(side.style, side.width);

    let mut stroke = Stroke::default();
    stroke.width = side.width;
    stroke.line_cap = line_cap;

    if !dash_pattern.is_empty() {
        if let Some(dash) = StrokeDash::new(dash_pattern.clone(), 0.0) {
            stroke.dash = Some(dash);
        }
    }

    // 构建线段路径
    let mut pb = PathBuilder::new();
    pb.move_to(x1, y1);
    pb.line_to(x2, y2);
    if let Some(path) = pb.finish() {
        pixmap.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
    }
}

/// 构建圆角矩形路径
///
/// 使用 `PathBuilder` 构建一个左上角在 (x, y)、宽 w、高 h、圆角半径 r 的矩形路径
fn build_rounded_rect_path(x: f32, y: f32, w: f32, h: f32, r: f32) -> Option<tiny_skia::Path> {
    if w <= 0.0 || h <= 0.0 {
        return None;
    }

    let r = r.min(w / 2.0).min(h / 2.0);

    if r <= 0.0 {
        // 无圆角：普通矩形
        let mut pb = PathBuilder::new();
        pb.move_to(x, y);
        pb.line_to(x + w, y);
        pb.line_to(x + w, y + h);
        pb.line_to(x, y + h);
        pb.close();
        return pb.finish();
    }

    // 带圆角的矩形路径
    // 使用弧线构建四个圆角
    let mut pb = PathBuilder::new();

    // 从左上角开始（在圆角之后）
    pb.move_to(x + r, y);

    // 上边（右半部分到右上角）
    pb.line_to(x + w - r, y);
    // 右上角弧线（顺时针）
    pb.cubic_to(x + w - r * 0.448, y, x + w, y + r * 0.448, x + w, y + r);

    // 右边（到右下角）
    pb.line_to(x + w, y + h - r);
    // 右下角弧线（顺时针）
    pb.cubic_to(
        x + w,
        y + h - r * 0.448,
        x + w - r * 0.448,
        y + h,
        x + w - r,
        y + h,
    );

    // 下边（到左下角）
    pb.line_to(x + r, y + h);
    // 左下角弧线（顺时针）
    pb.cubic_to(x + r * 0.448, y + h, x, y + h - r * 0.448, x, y + h - r);

    // 左边（到左上角）
    pb.line_to(x, y + r);
    // 左上角弧线（顺时针）
    pb.cubic_to(x, y + r * 0.448, x + r * 0.448, y, x + r, y);

    pb.close();
    pb.finish()
}

/// 构建单边圆角路径（包含相邻的两个圆角弧线）
///
/// 为指定的侧边构建路径，包括该侧的直线段和相邻的两个角弧线。
/// side: "top", "right", "bottom", "left"
fn build_rounded_side_path(
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    r: f32,
    side: &str,
) -> Option<tiny_skia::Path> {
    if w <= 0.0 || h <= 0.0 {
        return None;
    }

    let r = r.min(w / 2.0).min(h / 2.0);
    let mut pb = PathBuilder::new();

    match side {
        "top" => {
            if r > 0.0 {
                // 从左边开始：左上角弧线 -> 上边直线 -> 右上角弧线
                pb.move_to(x, y + r);
                pb.cubic_to(x, y + r * 0.448, x + r * 0.448, y, x + r, y);
                pb.line_to(x + w - r, y);
                pb.cubic_to(x + w - r * 0.448, y, x + w, y + r * 0.448, x + w, y + r);
            } else {
                pb.move_to(x, y);
                pb.line_to(x + w, y);
            }
        }
        "right" => {
            if r > 0.0 {
                // 从上边开始：右上角弧线 -> 右边直线 -> 右下角弧线
                pb.move_to(x + w - r, y);
                pb.cubic_to(x + w - r * 0.448, y, x + w, y + r * 0.448, x + w, y + r);
                pb.line_to(x + w, y + h - r);
                pb.cubic_to(
                    x + w,
                    y + h - r * 0.448,
                    x + w - r * 0.448,
                    y + h,
                    x + w - r,
                    y + h,
                );
            } else {
                pb.move_to(x + w, y);
                pb.line_to(x + w, y + h);
            }
        }
        "bottom" => {
            if r > 0.0 {
                // 从右边开始：右下角弧线 -> 下边直线 -> 左下角弧线
                pb.move_to(x + w, y + h - r);
                pb.cubic_to(
                    x + w,
                    y + h - r * 0.448,
                    x + w - r * 0.448,
                    y + h,
                    x + w - r,
                    y + h,
                );
                pb.line_to(x + r, y + h);
                pb.cubic_to(x + r * 0.448, y + h, x, y + h - r * 0.448, x, y + h - r);
            } else {
                pb.move_to(x + w, y + h);
                pb.line_to(x, y + h);
            }
        }
        "left" => {
            if r > 0.0 {
                // 从下边开始：左下角弧线 -> 左边直线 -> 左上角弧线
                pb.move_to(x + r, y + h);
                pb.cubic_to(x + r * 0.448, y + h, x, y + h - r * 0.448, x, y + h - r);
                pb.line_to(x, y + r);
                pb.cubic_to(x, y + r * 0.448, x + r * 0.448, y, x + r, y);
            } else {
                pb.move_to(x, y + h);
                pb.line_to(x, y);
            }
        }
        _ => return None,
    }

    pb.finish()
}

/// 绘制单个边框边的辅助函数（含笔触设置，供圆角非均匀场景使用）
fn draw_side_with_style(pixmap: &mut tiny_skia::Pixmap, path: &tiny_skia::Path, side: &BorderSide) {
    if !side.style.is_visible() || side.width <= 0.0 {
        return;
    }

    let rgba = side.color.to_rgba();
    let mut paint = Paint::default();
    paint.set_color_rgba8(rgba[0], rgba[1], rgba[2], rgba[3]);

    let (line_cap, dash_pattern) = dash_pattern_for_style(side.style, side.width);

    let mut stroke = Stroke::default();
    stroke.width = side.width;
    stroke.line_cap = line_cap;

    if !dash_pattern.is_empty() {
        if let Some(dash) = StrokeDash::new(dash_pattern.clone(), 0.0) {
            stroke.dash = Some(dash);
        }
    }

    pixmap.stroke_path(path, &paint, &stroke, Transform::identity(), None);
}

/// 检查四个边是否具有统一的样式（相同的颜色、宽度和线条样式）
fn is_border_uniform(border: &BorderStyle) -> bool {
    let c = &border.top;
    border.right.width == c.width
        && border.bottom.width == c.width
        && border.left.width == c.width
        && border.right.color.to_rgba() == c.color.to_rgba()
        && border.bottom.color.to_rgba() == c.color.to_rgba()
        && border.left.color.to_rgba() == c.color.to_rgba()
        && border.right.style == c.style
        && border.bottom.style == c.style
        && border.left.style == c.style
}

/// 绘制完整的 CSS 边框
///
/// 如果 `border.radius > 0`，则使用圆角矩形描边路径绘制所有四条边。
/// 否则分别绘制四条直线边。
///
/// # 参数
///
/// * `pixmap` - 目标像素图
/// * `x` - 矩形左上角 x 坐标
/// * `y` - 矩形左上角 y 坐标
/// * `w` - 矩形宽度
/// * `h` - 矩形高度
/// * `border` - 边框样式
pub fn draw_border(
    pixmap: &mut tiny_skia::Pixmap,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    border: &BorderStyle,
) {
    if !border.has_visible_border() || w <= 0.0 || h <= 0.0 {
        return;
    }

    let max_width = border.top.width.max(
        border
            .right
            .width
            .max(border.bottom.width.max(border.left.width)),
    );

    if max_width <= 0.0 {
        return;
    }

    if border.radius > 0.0 {
        // 圆角边框
        if is_border_uniform(border) {
            // 快速路径：四边统一，使用单条圆角矩形描边
            let primary = &border.top;
            let rgba = primary.color.to_rgba();
            let mut paint = Paint::default();
            paint.set_color_rgba8(rgba[0], rgba[1], rgba[2], rgba[3]);

            let (line_cap, dash_pattern) = dash_pattern_for_style(primary.style, primary.width);

            let mut stroke = Stroke::default();
            stroke.width = primary.width;
            stroke.line_cap = line_cap;

            if !dash_pattern.is_empty() {
                if let Some(dash) = StrokeDash::new(dash_pattern.clone(), 0.0) {
                    stroke.dash = Some(dash);
                }
            }

            if let Some(path) = build_rounded_rect_path(x, y, w, h, border.radius) {
                pixmap.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
            }
        } else {
            // 非均匀边框：分别绘制四条边，每条边包含相邻的圆角弧线
            for side in &["top", "right", "bottom", "left"] {
                let side_style = match *side {
                    "top" => &border.top,
                    "right" => &border.right,
                    "bottom" => &border.bottom,
                    "left" => &border.left,
                    _ => continue,
                };

                if !side_style.style.is_visible() || side_style.width <= 0.0 {
                    continue;
                }

                if let Some(path) = build_rounded_side_path(x, y, w, h, border.radius, side) {
                    draw_side_with_style(pixmap, &path, side_style);
                }
            }
        }
    } else {
        // 非圆角：分别绘制四条边
        // 上边
        if border.top.style.is_visible() && border.top.width > 0.0 {
            draw_border_side(pixmap, x, y, x + w, y, &border.top);
        }
        // 右边
        if border.right.style.is_visible() && border.right.width > 0.0 {
            draw_border_side(pixmap, x + w, y, x + w, y + h, &border.right);
        }
        // 下边
        if border.bottom.style.is_visible() && border.bottom.width > 0.0 {
            draw_border_side(pixmap, x, y + h, x + w, y + h, &border.bottom);
        }
        // 左边
        if border.left.style.is_visible() && border.left.width > 0.0 {
            draw_border_side(pixmap, x, y, x, y + h, &border.left);
        }
    }
}

/// 绘制 box-shadow
///
/// 在单独创建的临时 Pixmap 上绘制阴影矩形，使用 fastblur 做高斯模糊，
/// 然后将结果绘制到目标 pixmap 上。
///
/// # 参数
///
/// * `pixmap` - 目标像素图
/// * `x` - 元素左上角 x 坐标
/// * `y` - 元素左上角 y 坐标
/// * `w` - 元素宽度
/// * `h` - 元素高度
/// * `offset_x` - 阴影水平偏移（正数向右）
/// * `offset_y` - 阴影垂直偏移（正数向下）
/// * `blur_radius` - 高斯模糊半径（px）
/// * `spread` - 扩展半径（px），正数使阴影扩大，负数缩小
/// * `color` - 阴影颜色
pub fn draw_box_shadow(
    pixmap: &mut tiny_skia::Pixmap,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    offset_x: f32,
    offset_y: f32,
    blur_radius: f32,
    spread: f32,
    color: &Color,
) {
    if w <= 0.0 || h <= 0.0 || blur_radius <= 0.0 {
        return;
    }

    let blur_radius = blur_radius.ceil() as u32;
    let margin = blur_radius * 2;

    // 阴影矩形的尺寸（加上扩展和边距）
    let shadow_w = (w + spread * 2.0 + margin as f32 * 2.0) as u32;
    let shadow_h = (h + spread * 2.0 + margin as f32 * 2.0) as u32;

    if shadow_w == 0 || shadow_h == 0 {
        return;
    }

    // 创建临时 pixmap 用于绘制阴影
    let Some(mut shadow_pixmap) = tiny_skia::Pixmap::new(shadow_w, shadow_h) else {
        return;
    };

    // 临时 pixmap 中阴影矩形的位置（考虑偏移和边距）
    let shadow_rect_x = margin as f32 + spread + offset_x;
    let shadow_rect_y = margin as f32 + spread + offset_y;
    let shadow_rect_w = w;
    let shadow_rect_h = h;

    // 绘制填充矩形作为阴影
    let rgba = color.to_rgba();
    let mut paint = tiny_skia::Paint::default();
    paint.set_color_rgba8(rgba[0], rgba[1], rgba[2], rgba[3]);

    if let Some(rect) =
        tiny_skia::Rect::from_xywh(shadow_rect_x, shadow_rect_y, shadow_rect_w, shadow_rect_h)
    {
        shadow_pixmap.fill_rect(rect, &paint, tiny_skia::Transform::identity(), None);
    }

    // 通过多 pass box blur 近似高斯模糊（对 RGBA 全部通道进行模糊）
    let shadow_data = shadow_pixmap.data_mut();
    gaussian_blur_rgba(
        shadow_data,
        shadow_w as usize,
        shadow_h as usize,
        blur_radius as f32,
    );

    // 将模糊后的阴影绘制到目标 pixmap
    // 计算阴影在目标 pixmap 中的位置
    let dest_x = (x + offset_x - margin as f32 - spread) as i32;
    let dest_y = (y + offset_y - margin as f32 - spread) as i32;

    let pixmap_paint = tiny_skia::PixmapPaint {
        opacity: 1.0,
        blend_mode: tiny_skia::BlendMode::SourceOver,
        ..tiny_skia::PixmapPaint::default()
    };

    pixmap.draw_pixmap(
        dest_x,
        dest_y,
        shadow_pixmap.as_ref(),
        &pixmap_paint,
        tiny_skia::Transform::identity(),
        None,
    );
}

/// 对 RGBA 像素数据做多 pass box blur（近似高斯模糊）
///
/// `data` 是 RGBA 格式的字节数组（每个像素 4 个 byte），
/// `width` 和 `height` 是像素尺寸，`radius` 是模糊半径。
/// 使用 3 次 box blur 来近似高斯模糊。
fn gaussian_blur_rgba(data: &mut [u8], width: usize, height: usize, radius: f32) {
    if radius <= 0.0 || width < 3 || height < 3 {
        return;
    }

    // 计算需要的 pass 数和每次的 box blur 半径
    // 3 passes of box blur approximate a gaussian blur
    let pass_count = 3;
    // 将高斯 sigma 转换为 box blur 半径
    // sigma = sqrt((n*w^2 - n)/12) where n = number of passes, w = box size
    // For n=3: w = sqrt(4*sigma^2 + 1)
    let sigma = radius;
    let box_size = (4.0 * sigma * sigma + 1.0).sqrt();
    let box_radius = ((box_size - 1.0) / 2.0).ceil() as usize;

    if box_radius == 0 {
        return;
    }

    let stride = width * 4;

    // 临时缓冲区
    let mut temp = vec![0u8; data.len()];

    for _pass in 0..pass_count {
        // 水平方向模糊
        box_blur_horizontal_rgba(data, &mut temp, width, height, stride, box_radius);
        // 垂直方向模糊
        box_blur_vertical_rgba(temp.as_mut_slice(), data, width, height, stride, box_radius);
    }
}

/// 水平方向 box blur（RGBA）
fn box_blur_horizontal_rgba(
    src: &[u8],
    dst: &mut [u8],
    width: usize,
    height: usize,
    stride: usize,
    radius: usize,
) {
    if radius == 0 || width == 0 || height == 0 {
        dst.copy_from_slice(src);
        return;
    }

    let iarr = 1.0 / (radius + radius + 1) as f32;

    for y in 0..height {
        let row_start = y * stride;

        for c in 0..4 {
            // 初始累积
            let mut val: isize = 0;
            for x in 0..(radius + 1).min(width) {
                val += src[row_start + x * 4 + c] as isize;
            }
            // 边缘填充：左侧
            let first_val = src[row_start + c] as isize;
            val += radius.saturating_sub(width.saturating_sub(1)) as isize * first_val;

            let mut ti = row_start + c;
            let mut li = row_start + c;
            let mut ri = row_start + (radius.min(width.saturating_sub(1))) * 4 + c;
            let row_end = row_start + (width - 1) * 4 + c;

            // 左侧部分（靠近边缘）
            for _ in 0..(radius + 1).min(width) {
                // 使用右边像素
                let right_val = if ri <= row_end {
                    src[ri] as isize
                } else {
                    src[row_end] as isize
                };
                val += right_val - first_val;
                dst[ti] = (val as f32 * iarr).round() as u8;
                ti += 4;
                if ri + 4 <= row_end {
                    ri += 4;
                }
            }

            // 中间部分
            if width > radius + 1 {
                for _ in (radius + 1)..width.saturating_sub(radius) {
                    if ri < src.len() && li < src.len() {
                        val += src[ri] as isize - src[li] as isize;
                    }
                    dst[ti] = (val as f32 * iarr).round() as u8;
                    ti += 4;
                    li += 4;
                    ri += 4;
                }
            }

            // 右侧部分
            if width > radius {
                let last_val = src[row_end] as isize;
                let right_start = width.saturating_sub(radius);
                let count = radius.min(right_start);
                for _ in 0..count {
                    if li < src.len() {
                        val += last_val - src[li] as isize;
                    }
                    dst[ti] = (val as f32 * iarr).round() as u8;
                    ti += 4;
                    li += 4;
                }
            }
        }
    }
}

/// 垂直方向 box blur（RGBA）
fn box_blur_vertical_rgba(
    src: &[u8],
    dst: &mut [u8],
    width: usize,
    height: usize,
    stride: usize,
    radius: usize,
) {
    if radius == 0 || width == 0 || height == 0 {
        dst.copy_from_slice(src);
        return;
    }

    let iarr = 1.0 / (radius + radius + 1) as f32;

    for x in 0..width {
        let col_offset = x * 4;

        for c in 0..4 {
            // 初始累积
            let mut val: isize = 0;
            for y in 0..(radius + 1).min(height) {
                val += src[y * stride + col_offset + c] as isize;
            }
            let first_val = src[col_offset + c] as isize;
            val += radius.saturating_sub(height.saturating_sub(1)) as isize * first_val;

            let mut ti = col_offset + c;
            let mut li = col_offset + c;
            let mut ri = (radius.min(height.saturating_sub(1))) * stride + col_offset + c;
            let col_end = (height - 1) * stride + col_offset + c;

            // 顶部部分
            for _ in 0..(radius + 1).min(height) {
                let bottom_val = if ri <= col_end {
                    src[ri] as isize
                } else {
                    src[col_end] as isize
                };
                val += bottom_val - first_val;
                dst[ti] = (val as f32 * iarr).round() as u8;
                ti += stride;
                if ri + stride <= col_end {
                    ri += stride;
                }
            }

            // 中间部分
            if height > radius + 1 {
                for _ in (radius + 1)..height.saturating_sub(radius) {
                    if ri < src.len() && li < src.len() {
                        val += src[ri] as isize - src[li] as isize;
                    }
                    dst[ti] = (val as f32 * iarr).round() as u8;
                    ti += stride;
                    li += stride;
                    ri += stride;
                }
            }

            // 底部部分
            if height > radius {
                let last_val = src[col_end] as isize;
                let bottom_start = height.saturating_sub(radius);
                let count = radius.min(bottom_start);
                for _ in 0..count {
                    if li < src.len() {
                        val += last_val - src[li] as isize;
                    }
                    dst[ti] = (val as f32 * iarr).round() as u8;
                    ti += stride;
                    li += stride;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_border_side_none() {
        let side = BorderSide::none();
        assert!(!side.style.is_visible());
        assert_eq!(side.width, 0.0);
    }

    #[test]
    fn test_border_style_uniform() {
        let style = BorderStyle::uniform(2.0, Color::RED);
        assert!(style.has_visible_border());
        assert_eq!(style.top.width, 2.0);
        assert_eq!(style.top.color, Color::RED);
    }

    #[test]
    fn test_border_style_has_visible() {
        let style = BorderStyle::none();
        assert!(!style.has_visible_border());

        let mut style = BorderStyle::none();
        style.top = BorderSide::solid(1.0, Color::BLACK);
        assert!(style.has_visible_border());
    }

    #[test]
    fn test_draw_border_no_op_for_invisible() {
        let mut pixmap = tiny_skia::Pixmap::new(100, 100).unwrap();
        let border = BorderStyle::none();
        // 应该不会 panic
        draw_border(&mut pixmap, 10.0, 10.0, 50.0, 50.0, &border);
    }

    #[test]
    fn test_draw_border_solid() {
        let mut pixmap = tiny_skia::Pixmap::new(100, 100).unwrap();
        let border = BorderStyle::uniform(2.0, Color::BLACK);
        draw_border(&mut pixmap, 10.0, 10.0, 50.0, 50.0, &border);
        // 检查边框绘制成功（没有 panic）
        assert!(pixmap.encode_png().is_ok());
    }

    #[test]
    fn test_draw_border_with_radius() {
        let mut pixmap = tiny_skia::Pixmap::new(100, 100).unwrap();
        let mut border = BorderStyle::uniform(2.0, Color::BLUE);
        border.radius = 5.0;
        draw_border(&mut pixmap, 10.0, 10.0, 50.0, 50.0, &border);
        assert!(pixmap.encode_png().is_ok());
    }
}
