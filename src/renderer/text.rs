//! Text Renderer - 文本渲染
//!
//! 处理文本布局和渲染

use crate::css::values::Color;
use cosmic_text::{Align, Attrs, Buffer, FontSystem, Metrics, Shaping, Wrap};

/// 文本渲染器
pub struct TextRenderer {
    /// 默认字体大小
    default_font_size: f32,
    /// 默认字体颜色
    default_color: Color,
    /// 行高
    line_height: f32,
}

impl TextRenderer {
    /// 创建新的文本渲染器
    pub fn new() -> Self {
        Self {
            default_font_size: 16.0,
            default_color: Color::BLACK,
            line_height: 1.2,
        }
    }

    /// 设置默认字体大小
    pub fn set_font_size(&mut self, size: f32) {
        self.default_font_size = size;
    }

    /// 设置默认颜色
    pub fn set_color(&mut self, color: Color) {
        self.default_color = color;
    }

    /// 设置行高
    pub fn set_line_height(&mut self, height: f32) {
        self.line_height = height;
    }

    /// 测量文本尺寸（快速估计，所有字符等宽）
    ///
    /// 对于需要高精度的场景（如 CJK 字符、可变宽度字体），
    /// 请使用 [`Self::measure_text_accurate`] 方法替代。
    pub fn measure_text(&self, text: &str) -> (f32, f32) {
        // 简化实现：按字符估计宽度
        let char_width = self.default_font_size * 0.6;
        let width = text.len() as f32 * char_width;
        let height = self.default_font_size * self.line_height;

        (width, height)
    }

    /// 使用 cosmic-text 精确测量文本尺寸。
    ///
    /// 支持可变宽度字体、CJK 字符、emoji 等复杂字形，
    /// 返回准确的 (宽度, 高度)。
    ///
    /// # 参数
    /// - `text`: 要测量的文本
    /// - `font_system`: cosmic-text 字体系统
    /// - `max_width`: 最大可用宽度（用于自动换行）
    /// - `font_size`: 字体大小（覆盖默认值）
    /// - `line_height`: 行高倍率（覆盖默认值）
    pub fn measure_text_accurate(
        &self,
        text: &str,
        font_system: &mut FontSystem,
        max_width: f32,
        font_size: f32,
        line_height: f32,
    ) -> (f32, f32) {
        measure_text_cosmic(text, font_system, max_width, font_size, line_height)
    }

    /// 测量多行文本
    pub fn measure_multiline(&self, text: &str, max_width: f32) -> Vec<(String, (f32, f32))> {
        let char_width = self.default_font_size * 0.6;
        let chars_per_line = (max_width / char_width).floor() as usize;

        if chars_per_line == 0 {
            return vec![];
        }

        let mut lines = Vec::new();
        let mut current_line = String::new();
        let mut current_width = 0.0f32;

        for c in text.chars() {
            let char_size = if c.is_whitespace() {
                char_width * 0.5
            } else {
                char_width
            };

            if current_width + char_size > max_width && !current_line.is_empty() {
                let line_copy = current_line.clone();
                lines.push((line_copy, self.measure_text(&current_line)));
                current_line.clear();
                current_width = 0.0;
            }

            current_line.push(c);
            current_width += char_size;
        }

        if !current_line.is_empty() {
            let line_copy = current_line.clone();
            lines.push((line_copy, self.measure_text(&current_line)));
        }

        lines
    }

    /// 换行信息
    pub fn line_break(&self, text: &str, max_width: f32) -> Vec<usize> {
        let char_width = self.default_font_size * 0.6;
        let chars_per_line = (max_width / char_width).floor() as usize;

        if chars_per_line == 0 || text.is_empty() {
            return vec![];
        }

        let mut breaks = Vec::new();
        let mut last_break = 0;

        for (i, _c) in text.char_indices() {
            if i - last_break >= chars_per_line {
                // 找到最后一个空格作为断点
                if let Some(space_pos) = text[last_break..i].rfind(' ') {
                    breaks.push(last_break + space_pos);
                    last_break = last_break + space_pos + 1;
                } else {
                    // 没有空格，直接断开
                    breaks.push(i);
                    last_break = i;
                }
            }
        }

        breaks
    }
}

impl Default for TextRenderer {
    fn default() -> Self {
        Self::new()
    }
}

/// 使用 cosmic-text 精确测量文本尺寸（独立函数，无需 TextRenderer 实例）。
///
/// # 返回值
/// `(width, height)` — 文本实际占用的宽度和高度。
/// - 宽度为所有行中的最大宽度。
/// - 高度为行数 × 行高。
///
/// 如果文本为空，返回 `(0.0, 0.0)`。
pub fn measure_text_cosmic(
    text: &str,
    font_system: &mut FontSystem,
    max_width: f32,
    font_size: f32,
    line_height: f32,
) -> (f32, f32) {
    let text = text.trim();
    if text.is_empty() || font_size <= 0.0 {
        return (0.0, 0.0);
    }

    let lh = font_size * line_height;
    let mut buffer = Buffer::new(font_system, Metrics::new(font_size, lh));

    buffer.set_size(Some(max_width.max(50.0)), Some(f32::INFINITY));
    buffer.set_wrap(Wrap::Word);
    let attrs = Attrs::new();
    buffer.set_text(text, &attrs, Shaping::Advanced, Some(Align::Left));
    buffer.shape_until_scroll(font_system, true);

    // 从 layout_runs 中获取每行的实际宽度
    let mut max_line_width = 0.0f32;
    let mut line_count = 0usize;

    for run in buffer.layout_runs() {
        max_line_width = max_line_width.max(run.line_w);
        line_count += 1;
    }

    let width = max_line_width.ceil();
    let height = (line_count as f32 * lh).max(if line_count > 0 { lh } else { 0.0 });

    (width, height)
}

/// 文本片段
#[derive(Debug, Clone)]
pub struct TextFragment {
    /// 文本内容
    pub text: String,
    /// x 坐标
    pub x: f32,
    /// y 坐标
    pub y: f32,
    /// 宽度
    pub width: f32,
    /// 高度
    pub height: f32,
    /// 颜色
    pub color: Color,
    /// 字体大小
    pub font_size: f32,
}

impl TextFragment {
    /// 创建新的文本片段
    pub fn new(text: &str, x: f32, y: f32) -> Self {
        Self {
            text: text.to_string(),
            x,
            y,
            width: 0.0,
            height: 0.0,
            color: Color::BLACK,
            font_size: 16.0,
        }
    }

    /// 设置尺寸
    pub fn with_size(mut self, width: f32, height: f32) -> Self {
        self.width = width;
        self.height = height;
        self
    }

    /// 设置颜色
    pub fn with_color(mut self, color: Color) -> Self {
        self.color = color;
        self
    }

    /// 设置字体大小
    pub fn with_font_size(mut self, size: f32) -> Self {
        self.font_size = size;
        self
    }
}

/// 文本布局
#[derive(Debug, Clone)]
pub struct TextLayout {
    /// 片段
    pub fragments: Vec<TextFragment>,
    /// 总宽度
    pub width: f32,
    /// 总高度
    pub height: f32,
}

impl TextLayout {
    /// 创建新的文本布局
    pub fn new() -> Self {
        Self {
            fragments: Vec::new(),
            width: 0.0,
            height: 0.0,
        }
    }

    /// 添加片段
    pub fn add_fragment(&mut self, fragment: TextFragment) {
        self.width = self.width.max(fragment.x + fragment.width);
        self.height = self.height.max(fragment.y + fragment.height);
        self.fragments.push(fragment);
    }

    /// 清空
    pub fn clear(&mut self) {
        self.fragments.clear();
        self.width = 0.0;
        self.height = 0.0;
    }
}

impl Default for TextLayout {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_text_renderer_creation() {
        let renderer = TextRenderer::new();
        let (width, height) = renderer.measure_text("Hello");
        assert!(width > 0.0);
        assert!(height > 0.0);
    }

    #[test]
    fn test_text_measurement() {
        let renderer = TextRenderer::new();
        let text = "Hello, World!";
        let (width, height) = renderer.measure_text(text);
        assert_eq!(height, 16.0 * 1.2); // 默认字体大小 * 行高
    }
}
