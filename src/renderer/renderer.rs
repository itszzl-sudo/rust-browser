//! Renderer - 主渲染器
//!
//! 使用 obscura-dom 和 tiny-skia 渲染

use crate::browser::Document;
use crate::css::stylesheet::Stylesheet;
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

/// 主渲染器
pub struct Renderer {
    /// 渲染上下文
    context: RenderContext,
    /// 绘制器
    painter: Painter,
    /// 文本渲染器
    text_renderer: TextRenderer,
}

impl Renderer {
    /// 创建新的渲染器
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

    /// 设置视口
    pub fn set_viewport(&mut self, width: u32, height: u32) {
        debug!("设置视口: {}x{}", width, height);
        self.context.set_viewport(width, height);
    }

    /// 设置样式表
    pub fn set_stylesheet(&mut self, stylesheet: Stylesheet) {
        debug!("设置样式表");
        self.context.set_stylesheet(stylesheet);
    }

    /// 渲染文档
    pub fn render(&mut self, document: &Option<Document>) -> Result<Vec<u8>, RenderError> {
        info!("开始渲染");

        // 清空画布
        self.painter.set_background(crate::css::values::Color::WHITE);

        if let Some(doc) = document {
            // 渲染文档内容
            self.render_document(doc)?;
        } else {
            // 渲染默认空白页面
            self.render_blank_page()?;
        }

        // 输出 PNG
        Ok(self.painter.to_png())
    }

    /// 渲染文档内容
    fn render_document(&mut self, document: &Document) -> Result<(), RenderError> {
        trace!("渲染文档: {}", document.title.as_deref().unwrap_or("无标题"));

        let (width, height) = self.context.viewport();

        // 获取 DOM 树
        let dom = document.get_dom();

        // 简单渲染：遍历所有元素和文本
        let mut renderer = SimpleRenderer {
            painter: &mut self.painter,
            current_y: 20.0,
            viewport_width: width as f32,
        };

        renderer.render_dom(dom);

        debug!("文档渲染完成");
        Ok(())
    }

    /// 渲染空白页面
    fn render_blank_page(&mut self) -> Result<(), RenderError> {
        debug!("渲染空白页面");
        Ok(())
    }

    /// 截取视口
    pub fn capture_viewport(&self) -> Vec<u8> {
        self.painter.to_png()
    }

    /// 保存为文件
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

    /// 获取上下文
    pub fn context(&self) -> &RenderContext {
        &self.context
    }

    /// 获取绘制器
    pub fn painter(&self) -> &Painter {
        &self.painter
    }

    /// 获取文本渲染器
    pub fn text_renderer(&self) -> &TextRenderer {
        &self.text_renderer
    }
}

/// 简单的 DOM 渲染器
struct SimpleRenderer<'a> {
    painter: &'a mut Painter,
    current_y: f32,
    viewport_width: f32,
}

impl<'a> SimpleRenderer<'a> {
    fn render_dom(&mut self, dom: &DomWrapper) {
        // 从 body 开始
        let body = dom.body();

        self.render_node_recursive(body, dom);
    }

    fn render_node_recursive(&mut self, node: obscura_dom::NodeId, dom: &DomWrapper) {
        if let Some(node_obj) = dom.get_node(node) {
            match &node_obj.data {
                obscura_dom::NodeData::Text { contents, .. } => {
                    // 渲染文本
                    self.render_text(contents);
                }
                obscura_dom::NodeData::Element { name, .. } => {
                    // 渲染元素
                    self.render_element(&name.local.to_string(), node, dom);
                }
                _ => {}
            }
        }

        // 遍历子节点
        for child in dom.children(node) {
            self.render_node_recursive(child, dom);
        }
    }

    fn render_text(&mut self, text: &str) {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return;
        }

        // 简单渲染文本（一行一行渲染）
        let max_line_length = ((self.viewport_width - 40.0) / 10.0) as usize;
        let chars: Vec<char> = trimmed.chars().collect();
        let mut i = 0;

        while i < chars.len() {
            let end = (i + max_line_length).min(chars.len());
            let line: String = chars[i..end].iter().collect();

            // 绘制文本（用矩形模拟）
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

    fn render_element(&mut self, tag: &str, _node: obscura_dom::NodeId, _dom: &DomWrapper) {
        let tag_lower = tag.to_lowercase();

        match tag_lower.as_str() {
            "h1" => {
                // 标题
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
                // 换行
                self.current_y += 20.0;
            }
            "hr" => {
                // 分隔线
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
                // 图片占位符
                self.painter.paint_rect(
                    20.0,
                    self.current_y,
                    200.0,
                    150.0,
                    &Color::from_hex("#e0e0e0"),
                );
                self.current_y += 160.0;
            }
            "p" | "div" | "span" | "a" | "ul" | "ol" | "li" => {
                // 这些主要通过它们的文本子节点渲染
            }
            _ => {}
        }
    }
}

/// 便捷函数
impl Renderer {
    /// 创建默认渲染器
    pub fn default_renderer() -> Self {
        Self::new(1280, 720)
    }

    /// 创建自定义尺寸渲染器
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
        let renderer = Renderer::new(100, 100);
        let result = renderer.render(&None);
        assert!(result.is_ok());
    }
}
