//! Layout Engine - 布局引擎
//!
//! 使用 Taffy 进行 CSS 布局计算

use crate::dom::node::{DomNode, DomTree};
use crate::dom::visitor::{DomTraverser, DomVisitor};
use log::{debug, info};

/// 布局结果
#[derive(Debug, Clone)]
pub struct LayoutResult {
    /// 节点索引
    pub node_index: usize,
    /// x 坐标
    pub x: f32,
    /// y 坐标
    pub y: f32,
    /// 宽度
    pub width: f32,
    /// 高度
    pub height: f32,
    /// 标签名（用于调试）
    pub tag_name: String,
}

/// 布局引擎
pub struct LayoutEngine {
    /// 节点映射
    node_map: Vec<usize>,
    /// 布局结果
    results: Vec<LayoutResult>,
    /// 视口宽度
    viewport_width: f32,
}

impl LayoutEngine {
    /// 创建新的布局引擎
    pub fn new() -> Self {
        info!("初始化布局引擎");
        Self {
            node_map: Vec::new(),
            results: Vec::new(),
            viewport_width: 800.0,
        }
    }

    /// 设置视口宽度
    pub fn set_viewport_width(&mut self, width: f32) {
        self.viewport_width = width;
    }

    /// 计算布局
    pub fn compute(&mut self, tree: &DomTree, viewport: (u32, u32)) -> Result<(), String> {
        debug!("计算布局: {:?}", viewport);

        self.node_map.clear();
        self.results.clear();
        self.viewport_width = viewport.0 as f32;

        // 遍历 DOM 树计算布局
        if let Some(root) = tree.root() {
            let mut visitor = LayoutVisitor {
                engine: self,
                y_offset: 0.0,
                parent_width: viewport.0 as f32,
            };
            DomTraverser::pre_order(tree, &mut visitor);
        }

        Ok(())
    }

    /// 添加布局节点
    pub fn add_node(&mut self, dom_index: usize) {
        self.node_map.push(dom_index);
    }

    /// 添加布局结果
    pub fn add_result(&mut self, node_index: usize, x: f32, y: f32, width: f32, height: f32, tag_name: &str) {
        self.results.push(LayoutResult {
            node_index,
            x,
            y,
            width,
            height,
            tag_name: tag_name.to_string(),
        });
    }

    /// 获取节点的布局
    pub fn get_layout(&self, dom_index: usize) -> Option<&LayoutResult> {
        self.results.iter().find(|r| r.node_index == dom_index)
    }

    /// 获取布局结果
    pub fn results(&self) -> &[LayoutResult] {
        &self.results
    }

    /// 清除布局结果
    pub fn clear(&mut self) {
        self.results.clear();
        self.node_map.clear();
    }
}

impl Default for LayoutEngine {
    fn default() -> Self {
        Self::new()
    }
}

/// 布局访问者
struct LayoutVisitor<'a> {
    engine: &'a mut LayoutEngine,
    y_offset: f32,
    parent_width: f32,
}

impl<'a> DomVisitor for LayoutVisitor<'a> {
    fn visit_pre(&mut self, node: &DomNode) {
        if node.is_element() {
            self.engine.add_node(node.index);

            if let crate::dom::node::NodeType::Element(data) = &node.node_type {
                let tag_name = &data.tag_name;
                let tag_lower = tag_name.to_lowercase();

                // 根据标签类型设置高度
                let (width, height) = match tag_lower.as_str() {
                    "h1" => (self.parent_width - 40.0, 50.0),
                    "h2" => (self.parent_width - 40.0, 40.0),
                    "h3" | "h4" | "h5" | "h6" => (self.parent_width - 40.0, 30.0),
                    "p" => (self.parent_width - 40.0, 25.0),
                    "div" | "section" | "article" | "main" | "header" | "footer" | "nav" | "aside" => {
                        (self.parent_width - 40.0, 0.0) // 高度由内容决定
                    }
                    "ul" | "ol" => (self.parent_width - 40.0, 0.0),
                    "li" => (self.parent_width - 60.0, 25.0),
"img" => {
                        let w = data.get_attribute("width")
                            .and_then(|v| v.parse::<f32>().ok())
                            .unwrap_or(200.0)
                            .min(self.parent_width - 40.0);
                        let h = data.get_attribute("height")
                            .and_then(|v| v.parse::<f32>().ok())
                            .unwrap_or(150.0);
                        (w, h)
                    }
                    "br" => (self.parent_width - 40.0, 20.0),
                    "hr" => (self.parent_width - 40.0, 2.0),
                    "input" | "button" | "select" => (200.0, 30.0),
                    "textarea" => (300.0, 100.0),
                    "table" => (self.parent_width - 40.0, 0.0),
                    "tr" => (self.parent_width - 60.0, 30.0),
                    "td" | "th" => (100.0, 30.0),
                    _ => (self.parent_width - 40.0, 25.0),
                };

                let is_block = data.is_block();
                let is_void = matches!(
                    tag_lower.as_str(),
                    "img" | "br" | "hr" | "input" | "meta" | "link"
                );

                if is_block || is_void {
                    let actual_height = if height == 0.0 { 20.0 } else { height };
                    self.engine.add_result(
                        node.index,
                        20.0,
                        self.y_offset,
                        width,
                        actual_height,
                        tag_name,
                    );
                    self.y_offset += actual_height + 10.0;
                } else {
                    // 行内元素
                    self.engine.add_result(
                        node.index,
                        20.0,
                        self.y_offset - 20.0,
                        width,
                        height,
                        tag_name,
                    );
                }
            }
        }
    }

    fn visit_text(&mut self, _node: &DomNode, text: &str) {
        if !text.trim().is_empty() {
            // 估算文本行数
            let chars_per_line = (self.parent_width - 40.0) / 8.0; // 每个字符约 8px
            let lines = (text.len() as f32 / chars_per_line).ceil();
            let text_height = (lines * 18.0).max(18.0);
            self.y_offset += text_height + 5.0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_layout_engine_creation() {
        let engine = LayoutEngine::new();
        assert!(engine.results.is_empty());
    }
}
