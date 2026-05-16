//! Renderer - 主渲染器
//!
//! 使用 kuchiki DOM 和 tiny-skia 渲染

use crate::browser::Document;
use crate::css::values::Color;
use crate::DomWrapper;
use crate::renderer::context::RenderContext;
use crate::renderer::painter::Painter;
use crate::renderer::text::TextRenderer;
use log::{debug, info, trace};
use std::path::Path;
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

pub struct Renderer {
    context: RenderContext,
    painter: Painter,
    text_renderer: TextRenderer,
}

impl Renderer {
    pub fn new(width: u32, height: u32) -> Self {
        info!("初始化渲染器 ({}x{})", width, height);

        let context = RenderContext::new(width, height);
        let painter = Painter::new(width, height)
            .ok_or(RenderError::PainterCreationFailed)
            .unwrap();
        let text_renderer = TextRenderer::new();

        Self {
            context,
            painter,
            text_renderer,
        }
    }

    pub fn set_viewport(&mut self, width: u32, height: u32) {
        debug!("设置视口: {}x{}", width, height);
        self.context.set_viewport(width, height);
        self.painter.set_viewport(width, height);
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
        trace!("渲染文档: {}", document.title.as_deref().unwrap_or("无标题"));

        let (width, height) = self.context.viewport();

        let dom = document.get_dom();

        let mut renderer = SimpleRenderer {
            painter: &mut self.painter,
            current_y: 20.0,
            viewport_width: width as f32,
        };

        renderer.render_dom(dom);

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

        let extension = path.extension()
            .and_then(|e| e.to_str())
            .unwrap_or("png")
            .to_lowercase();

        match extension.as_str() {
            "png" => {
                self.painter.save_png(path.to_str().unwrap())
                    .map_err(|e| RenderError::SaveFailed(e.to_string()))
            }
            "jpg" | "jpeg" => {
                let png_data = self.painter.to_png();
                let img = image::load_from_memory(&png_data)
                    .map_err(|e| RenderError::SaveFailed(e.to_string()))?;
                img.save(path)
                    .map_err(|e| RenderError::SaveFailed(e.to_string()))
            }
            _ => {
                self.painter.save_png(path.to_str().unwrap())
                    .map_err(|e| RenderError::SaveFailed(e.to_string()))
            }
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

struct SimpleRenderer<'a> {
    painter: &'a mut Painter,
    current_y: f32,
    viewport_width: f32,
}

impl<'a> SimpleRenderer<'a> {
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

    fn render_text(&mut self, text: &str) {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return;
        }

        let max_line_length = ((self.viewport_width - 40.0) / 10.0) as usize;
        let chars: Vec<char> = trimmed.chars().collect();
        let mut i = 0;

        while i < chars.len() {
            let end = (i + max_line_length).min(chars.len());
            let line: String = chars[i..end].iter().collect();

            self.painter.paint_rect(
                20.0,
                self.current_y,
                (line.len() as f32 * 10.0).min(self.viewport_width - 40.0),
                18.0,
                &Color::from_hex("#333333"),
            );

            self.current_y += 22.0;
            i = end;
        }
    }

    fn render_element(&mut self, tag: &str, dom: &DomWrapper) {
        match tag {
            "h1" => {
                self.painter.paint_rect(
                    20.0,
                    self.current_y,
                    150.0,
                    32.0,
                    &Color::from_hex("#333333"),
                );
                self.current_y += 42.0;
            }
            "h2" => {
                self.painter.paint_rect(
                    20.0,
                    self.current_y,
                    120.0,
                    28.0,
                    &Color::from_hex("#333333"),
                );
                self.current_y += 38.0;
            }
            "h3" | "h4" | "h5" | "h6" => {
                self.painter.paint_rect(
                    20.0,
                    self.current_y,
                    100.0,
                    24.0,
                    &Color::from_hex("#333333"),
                );
                self.current_y += 34.0;
            }
            "br" => {
                self.current_y += 20.0;
            }
            "hr" => {
                self.painter.paint_rect(
                    20.0,
                    self.current_y,
                    self.viewport_width - 40.0,
                    2.0,
                    &Color::from_hex("#cccccc"),
                );
                self.current_y += 20.0;
            }
            "img" => {
                self.painter.paint_rect(
                    20.0,
                    self.current_y,
                    200.0,
                    150.0,
                    &Color::from_hex("#e0e0e0"),
                );
                self.current_y += 160.0;
            }
            "p" | "div" | "span" | "a" | "ul" | "ol" | "li" => {}
            _ => {}
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
