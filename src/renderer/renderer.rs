//! Renderer - 主渲染器
//!
//! 使用 kuchiki DOM, cosmic-text 文本渲染 和 tiny-skia 渲染

use crate::browser::Document;
use crate::css::values::Color;
use crate::renderer::context::RenderContext;
use crate::renderer::painter::Painter;
use crate::renderer::taffy_layout::{LayoutNode, TaffyLayoutEngine};
use crate::renderer::text::TextRenderer;
use crate::DomWrapper;
use cosmic_text::{Attrs, Buffer, FontSystem, Metrics, Shaping, SwashCache, Wrap};
use log::{debug, info, trace};
use std::path::Path;
use std::sync::OnceLock;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum RenderError {
    #[error("创建绘制器失败")]
    PainterCreationFailed,
    #[error("布局计算失败: {0}")]
    LayoutFailed(String),
    #[error("渲染失败: {0}")]
    RenderFailed(String),
    #[error("保存图像失败: {0}")]
    SaveFailed(String),
}

/// 全局共享的字体系统（懒初始化）
fn global_font_system() -> &'static std::sync::Mutex<FontSystem> {
    static FONT_SYSTEM: OnceLock<std::sync::Mutex<FontSystem>> = OnceLock::new();
    FONT_SYSTEM.get_or_init(|| {
        info!("初始化 cosmic-text 字体系统");
        let font_system = FontSystem::new();

        // 尝试加载中文字体 (msyh.ttc)
        let font_paths = [
            "C:/Windows/Fonts/msyh.ttc",   // Microsoft YaHei
            "C:/Windows/Fonts/msyhbd.ttc", // Microsoft YaHei Bold
            "C:/Windows/Fonts/simsun.ttc", // SimSun
            "C:/Windows/Fonts/simhei.ttf", // SimHei
        ];

        for path in &font_paths {
            if let Ok(_data) = std::fs::read(path) {
                info!("加载字体: {}", path);
            }
        }

        info!("字体系统初始化完成");
        std::sync::Mutex::new(font_system)
    })
}

/// 全局共享的 SwashCache
fn global_swash_cache() -> &'static std::sync::Mutex<SwashCache> {
    static CACHE: OnceLock<std::sync::Mutex<SwashCache>> = OnceLock::new();
    CACHE.get_or_init(|| std::sync::Mutex::new(SwashCache::new()))
}

pub struct Renderer {
    context: RenderContext,
    painter: Painter,
    text_renderer: TextRenderer,
    document: Option<Document>,
    title: Option<String>,
}

impl Renderer {
    pub fn new(width: u32, height: u32) -> Self {
        info!("初始化渲染器 ({}x{})", width, height);

        let context = RenderContext::new(width, height);
        let painter = Painter::new(width, height)
            .ok_or(RenderError::PainterCreationFailed)
            .unwrap();
        let text_renderer = TextRenderer::new();

        // 预热字体系统
        let _ = global_font_system();
        let _ = global_swash_cache();

        Self {
            context,
            painter,
            text_renderer,
            document: None,
            title: None,
        }
    }

    pub fn set_viewport(&mut self, width: u32, height: u32) {
        debug!("设置视口: {}x{}", width, height);
        self.context.set_viewport(width, height);
        self.painter.set_viewport(width, height);
    }

    /// 设置当前文档（用于多进程模式下的渲染）
    pub fn set_document(&mut self, doc: Document) {
        let t = doc.title.clone();
        self.title = t;
        self.document = Some(doc);
    }

    /// 获取页面标题
    pub fn title(&self) -> Option<&str> {
        self.title.as_deref()
    }

    /// 渲染存储的文档到 PNG
    pub fn render_to_png(&mut self) -> Result<Vec<u8>, RenderError> {
        let doc = self.document.clone();
        self.render(&doc)
    }

    /// 调整大小（resize 是 set_viewport 的别名）
    pub fn resize(&mut self, width: u32, height: u32) {
        self.set_viewport(width, height);
    }

    pub fn render(&mut self, document: &Option<Document>) -> Result<Vec<u8>, RenderError> {
        info!("开始渲染");

        self.painter.set_background(Color::WHITE);
        self.painter.paint();

        if let Some(doc) = document {
            self.render_document(doc)?;
        } else {
            self.render_blank_page()?;
        }

        Ok(self.painter.to_png())
    }

    fn render_document(&mut self, document: &Document) -> Result<(), RenderError> {
        trace!(
            "渲染文档: {}",
            document.title.as_deref().unwrap_or("无标题")
        );

        let (width, height) = self.context.viewport();
        let dom = document.get_dom();

        // 1. 使用 Taffy 计算精确布局
        let mut taffy = TaffyLayoutEngine::new(width as f32, height as f32);
        if let Err(e) = taffy.compute(dom) {
            warn!("Taffy 布局计算失败: {}, 使用手动布局回退", e);
        }

        // 2. 使用 Taffy 布局结果渲染
        let mut renderer = TaffyRenderer {
            painter: &mut self.painter,
            taffy: &taffy,
            dom,
            viewport_width: width as f32,
        };
        renderer.render_dom();

        debug!("文档渲染完成");
        Ok(())
    }

    fn render_blank_page(&mut self) -> Result<(), RenderError> {
        debug!("渲染空白页面");
        Ok(())
    }

    pub fn capture_viewport(&self) -> Vec<u8> {
        self.painter.to_png()
    }

    pub fn save(&self, path: &Path) -> Result<(), RenderError> {
        debug!("保存渲染结果到: {:?}", path);

        let extension = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("png")
            .to_lowercase();

        match extension.as_str() {
            "png" => self
                .painter
                .save_png(path.to_str().unwrap())
                .map_err(|e| RenderError::SaveFailed(e.to_string())),
            "jpg" | "jpeg" => {
                let png_data = self.painter.to_png();
                let img = image::load_from_memory(&png_data)
                    .map_err(|e| RenderError::SaveFailed(e.to_string()))?;
                img.save(path)
                    .map_err(|e| RenderError::SaveFailed(e.to_string()))
            }
            _ => self
                .painter
                .save_png(path.to_str().unwrap())
                .map_err(|e| RenderError::SaveFailed(e.to_string())),
        }
    }

    pub fn context(&self) -> &RenderContext {
        &self.context
    }

    pub fn painter(&self) -> &Painter {
        &self.painter
    }

    pub fn text_renderer(&self) -> &TextRenderer {
        &self.text_renderer
    }
}

/// 使用 cosmic-text 实际渲染文本的渲染器
struct CosmicRenderer<'a> {
    painter: &'a mut Painter,
    current_y: f32,
    viewport_width: f32,
}

impl<'a> CosmicRenderer<'a> {
    fn render_dom(&mut self, dom: &DomWrapper) {
        let body = dom.body();
        self.render_node_recursive(body, dom);
    }

    fn render_node_recursive(&mut self, node: usize, dom: &DomWrapper) {
        if let Some(node_ref) = dom.get_node(node) {
            if let Some(text) = node_ref.as_text() {
                let contents = text.borrow();
                self.render_text(&contents);
            } else if let Some(element) = node_ref.as_element() {
                let tag_name = &element.name.local;
                self.render_element(tag_name, dom);
            }
        }

        for child in dom.children(node) {
            self.render_node_recursive(child, dom);
        }
    }

    /// 使用 cosmic-text 实际渲染文本
    fn render_text(&mut self, text: &str) {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return;
        }

        let max_width = self.viewport_width - 40.0;

        // 使用 cosmic-text 进行文本布局和渲染
        let mut font_system = global_font_system().lock().unwrap();
        let mut swash_cache = global_swash_cache().lock().unwrap();

        // 创建文本缓冲区
        let mut buffer = Buffer::new(
            &mut font_system,
            Metrics::new(16.0, 22.0), // 字体大小 16px, 行高 22px
        );

        // 设置缓冲区宽度（用于自动换行）
        buffer.set_size(&mut font_system, max_width, f32::INFINITY);
        buffer.set_wrap(&mut font_system, Wrap::Word);

        buffer.set_text(&mut font_system, trimmed, Attrs::new(), Shaping::Advanced);

        buffer.shape_until_scroll(&mut font_system, true);

        // 渲染 layout runs
        let max_lines = 100;
        let mut line_count = 0;
        let scale = 1.0;

        for run in buffer.layout_runs() {
            if line_count >= max_lines {
                break;
            }

            // 渲染该行中的每个字形
            for glyph in run.glyphs {
                let physical_glyph = glyph.physical((20.0, self.current_y), scale);
                let cache_key = physical_glyph.cache_key;

                if let Some(swash_image) = swash_cache.get_image(&mut font_system, cache_key) {
                    let glyph_x = physical_glyph.x as f32 + swash_image.placement.left as f32;
                    let glyph_y = physical_glyph.y as f32 - swash_image.placement.top as f32;

                    // 将渲染的字形绘制到 pixmap 上
                    self.render_glyph_image(
                        glyph_x,
                        glyph_y,
                        swash_image.placement.width as u32,
                        swash_image.placement.height as u32,
                        &swash_image.data,
                        &Color::from_hex("#333333"),
                    );
                }
            }

            line_count += 1;
            self.current_y += 22.0; // 行间距
        }
    }

    /// 将字形 alpha 蒙版渲染到 pixmap
    /// 将字形 alpha 蒙版渲染到 pixmap（使用 direct pixel access）
    fn render_glyph_image(
        &mut self,
        x: f32,
        y: f32,
        width: u32,
        height: u32,
        alpha_data: &[u8],
        color: &Color,
    ) {
        if width == 0 || height == 0 || alpha_data.is_empty() {
            return;
        }

        let text_rgba = color.to_rgba();
        let pixmap_width = self.painter.pixmap_mut().width();
        let pixmap_height = self.painter.pixmap_mut().height();

        // 获取直接像素缓冲区 (RGBA bytes)
        let pixel_data = self.painter.pixmap_mut().data_mut();
        let stride = pixmap_width as usize * 4;

        for row in 0..height {
            for col in 0..width {
                let alpha_idx = (row * width + col) as usize;
                if alpha_idx >= alpha_data.len() {
                    continue;
                }

                let alpha = alpha_data[alpha_idx];
                if alpha == 0 {
                    continue;
                }

                let px = (x + col as f32) as i32;
                let py = (y + row as f32) as i32;

                if px < 0 || py < 0 {
                    continue;
                }

                let px_u = px as u32;
                let py_u = py as u32;

                if px_u >= pixmap_width || py_u >= pixmap_height {
                    continue;
                }

                // 直接操作像素缓冲区 (RGBA 每通道 1 字节)
                let pixel_idx = (py_u as usize) * stride + (px_u as usize) * 4;
                if pixel_idx + 3 < pixel_data.len() {
                    let bg_r = pixel_data[pixel_idx];
                    let bg_g = pixel_data[pixel_idx + 1];
                    let bg_b = pixel_data[pixel_idx + 2];

                    // alpha 混合: result = text_color * alpha + bg * (1 - alpha)
                    let a_norm = alpha as f32 / 255.0;
                    let inv_a = 1.0 - a_norm;

                    pixel_data[pixel_idx] =
                        (text_rgba[0] as f32 * a_norm + bg_r as f32 * inv_a) as u8;
                    pixel_data[pixel_idx + 1] =
                        (text_rgba[1] as f32 * a_norm + bg_g as f32 * inv_a) as u8;
                    pixel_data[pixel_idx + 2] =
                        (text_rgba[2] as f32 * a_norm + bg_b as f32 * inv_a) as u8;
                    pixel_data[pixel_idx + 3] = 255; // 不透明
                }
            }
        }
    }

    fn render_element(&mut self, tag: &str, _dom: &DomWrapper) {
        match tag {
            "h1" => {
                // 绘制标题背景装饰条
                self.painter.paint_rect(
                    20.0,
                    self.current_y + 28.0,
                    4.0, // 左侧竖线
                    18.0,
                    &Color::from_hex("#4A90D9"),
                );
                self.current_y += 42.0;
            }
            "h2" => {
                self.painter.paint_rect(
                    20.0,
                    self.current_y + 22.0,
                    4.0,
                    16.0,
                    &Color::from_hex("#5BA0E9"),
                );
                self.current_y += 38.0;
            }
            "h3" | "h4" | "h5" | "h6" => {
                self.current_y += 34.0;
            }
            "br" => {
                self.current_y += 20.0;
            }
            "hr" => {
                self.painter.paint_rect(
                    20.0,
                    self.current_y + 10.0,
                    self.viewport_width - 40.0,
                    1.0,
                    &Color::from_hex("#dddddd"),
                );
                self.current_y += 20.0;
            }
            "img" => {
                // 占位框
                self.painter.paint_rect(
                    20.0,
                    self.current_y,
                    self.viewport_width - 40.0,
                    150.0,
                    &Color::from_hex("#f0f0f0"),
                );
                // 边框
                self.painter.paint_rect(
                    20.0,
                    self.current_y,
                    self.viewport_width - 40.0,
                    1.0,
                    &Color::from_hex("#dddddd"),
                );
                self.painter.paint_rect(
                    20.0,
                    self.current_y + 149.0,
                    self.viewport_width - 40.0,
                    1.0,
                    &Color::from_hex("#dddddd"),
                );
                // 居中图标（相机符号简化）
                let center_x = 20.0 + (self.viewport_width - 40.0) / 2.0 - 20.0;
                let center_y = self.current_y + 65.0;
                self.painter.paint_rect(
                    center_x,
                    center_y,
                    40.0,
                    20.0,
                    &Color::from_hex("#cccccc"),
                );
                self.current_y += 160.0;
            }
            "a" => {
                // 链接 - 蓝色下划线（但需要等文本渲染）
                // 在文本渲染中处理
            }
            "p" | "div" | "span" | "ul" | "ol" | "li" | "section" | "article" | "header"
            | "footer" | "nav" | "aside" | "main" | "form" | "body" | "html" => {
                // 容器元素 - 不需要额外绘制
            }
            _ => {
                // 其他元素 - 不需要额外绘制
            }
        }
    }
}

impl Renderer {
    pub fn default_renderer() -> Self {
        Self::new(1280, 720)
    }

    pub fn with_size(width: u32, height: u32) -> Self {
        Self::new(width, height)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_renderer_creation() {
        let renderer = Renderer::new(800, 600);
        assert_eq!(renderer.context().viewport(), (800, 600));
    }

    #[test]
    fn test_render_blank_page() {
        let mut renderer = Renderer::new(100, 100);
        let result = renderer.render(&None);
        assert!(result.is_ok());
    }
}
