//! SVG 渲染模块
//!
//! 使用 resvg 将 SVG 数据渲染到 tiny-skia Pixmap
//!
//! # 用途
//!
//! - 渲染内联 `<svg>` 元素
//! - 渲染 CSS `background-image: url(...)` 中的 SVG 文件
//! - 渲染百度首页等网站使用的 SVG 图标

use log::warn;
use std::collections::HashMap;
// 使用 resvg re-export 的 tiny-skia，确保版本一致
use resvg::tiny_skia::Pixmap;

/// 全局 SVG 渲染器（缓存已解析的 SVG 文档）
pub struct SvgRenderer {
    /// 按 URL 缓存的渲染结果
    cache: HashMap<String, Pixmap>,
    /// 是否启用
    enabled: bool,
}

impl SvgRenderer {
    /// 创建新的 SVG 渲染器
    pub fn new() -> Self {
        Self {
            cache: HashMap::new(),
            enabled: true,
        }
    }

    /// 设置启用状态
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    /// 从字节数据渲染 SVG
    ///
    /// # 参数
    ///
    /// * `data` - SVG 文件字节数据
    /// * `width` - 目标宽度（px），为 0 则使用原始尺寸
    /// * `height` - 目标高度（px），为 0 则使用原始尺寸
    ///
    /// # 返回
    ///
    /// 渲染后的 Pixmap，失败返回 None
    pub fn render_from_data(&self, data: &[u8], width: u32, height: u32) -> Option<Pixmap> {
        if !self.enabled || data.is_empty() {
            return None;
        }

        // 使用 usvg 解析 SVG
        let opt = usvg::Options::default();
        let rtree = match usvg::Tree::from_data(data, &opt) {
            Ok(tree) => tree,
            Err(e) => {
                warn!("SVG 解析失败: {}", e);
                return None;
            }
        };

        // 确定渲染尺寸
        let size = rtree.size();
        let render_width = if width > 0 {
            width
        } else {
            size.width() as u32
        };
        let render_height = if height > 0 {
            height
        } else {
            size.height() as u32
        };

        let mut pixmap = Pixmap::new(render_width, render_height)?;

        // 使用 resvg 渲染（使用 resvg 内部版本避免类型冲突）
        resvg::render(
            &rtree,
            resvg::tiny_skia::Transform::default(),
            &mut pixmap.as_mut(),
        );

        Some(pixmap)
    }

    /// 从字符串渲染 SVG
    pub fn render_from_str(&self, svg_text: &str, width: u32, height: u32) -> Option<Pixmap> {
        self.render_from_data(svg_text.as_bytes(), width, height)
    }

    /// 从文件路径渲染 SVG
    pub fn render_from_file(&self, path: &str, width: u32, height: u32) -> Option<Pixmap> {
        let data = std::fs::read(path).ok()?;
        self.render_from_data(&data, width, height)
    }

    /// 渲染并缓存 SVG（克隆返回避免借用冲突）
    pub fn render_cached(
        &mut self,
        url: &str,
        data: &[u8],
        width: u32,
        height: u32,
    ) -> Option<Pixmap> {
        if let Some(cached) = self.cache.get(url) {
            return Some(cached.clone());
        }
        let pixmap = self.render_from_data(data, width, height)?;
        self.cache.insert(url.to_string(), pixmap.clone());
        Some(pixmap)
    }

    /// 清空缓存
    pub fn clear_cache(&mut self) {
        self.cache.clear();
    }

    /// 检查是否支持 SVG
    pub fn is_supported(&self) -> bool {
        self.enabled
    }
}

impl Default for SvgRenderer {
    fn default() -> Self {
        Self::new()
    }
}

/// 将 SVG 数据渲染到现有的 Pixmap 上（在指定位置贴图）
pub fn render_svg_to_pixmap(
    target: &mut Pixmap,
    svg_data: &[u8],
    x: f32,
    y: f32,
    width: u32,
    height: u32,
) -> bool {
    let renderer = SvgRenderer::new();
    if let Some(src) = renderer.render_from_data(svg_data, width, height) {
        target.draw_pixmap(
            x as i32,
            y as i32,
            src.as_ref(),
            &resvg::tiny_skia::PixmapPaint::default(),
            resvg::tiny_skia::Transform::identity(),
            None,
        );
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_renderer_creation() {
        let renderer = SvgRenderer::new();
        assert!(renderer.is_supported());
    }

    #[test]
    fn test_render_simple_svg() {
        let renderer = SvgRenderer::new();
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100">
            <circle cx="50" cy="50" r="40" fill="red"/>
        </svg>"#;
        let pixmap = renderer.render_from_str(svg, 0, 0);
        assert!(pixmap.is_some(), "应该能渲染 SVG");
    }

    #[test]
    fn test_render_invalid_svg() {
        let renderer = SvgRenderer::new();
        let result = renderer.render_from_data(b"not svg data", 100, 100);
        assert!(result.is_none(), "无效 SVG 应该返回 None");
    }

    #[test]
    fn test_cache() {
        let mut renderer = SvgRenderer::new();
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="50" height="50">
            <rect width="50" height="50" fill="blue"/>
        </svg>"#;
        let result1 = renderer.render_cached("test.svg", svg.as_bytes(), 0, 0);
        assert!(result1.is_some());
        let result2 = renderer.render_cached("test.svg", svg.as_bytes(), 0, 0);
        assert!(result2.is_some());
    }
}
