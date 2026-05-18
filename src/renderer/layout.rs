//! Layout Engine - 布局引擎
//!
//! 使用 Taffy 进行 CSS 布局计算

use crate::css::stylesheet::ComputedStyle;
use crate::dom::node::{DomNode, DomTree, NodeType};
use crate::dom::visitor::{DomTraverser, DomVisitor};
use log::{debug, info, trace};

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
}

/// 布局引擎
pub struct LayoutEngine {
    /// 节点映射
    node_map: Vec<usize>,
    /// 布局结果
    results: Vec<LayoutResult>,
}

impl LayoutEngine {
    /// 创建新的布局引擎
    pub fn new() -> Self {
        info!("初始化布局引擎");
        Self {
            node_map: Vec::new(),
            results: Vec::new(),
        }
    }

    /// 计算布局
    pub fn compute(&mut self, tree: &DomTree, _stylesheet: &crate::css::stylesheet::Stylesheet, viewport: (u32, u32)) -> Result<(), String> {
        debug!("计算布局: {:?}", viewport);
        
        self.node_map.clear();
        self.results.clear();
        
        // 简化实现：直接遍历 DOM 树生成布局
        if let Some(root) = tree.root() {
            let mut visitor = LayoutVisitor {
                engine: self,
                y_offset: 0.0,
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
    pub fn add_result(&mut self, node_index: usize, x: f32, y: f32, width: f32, height: f32) {
        self.results.push(LayoutResult {
            node_index,
            x,
            y,
            width,
            height,
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
}

impl<'a> DomVisitor for LayoutVisitor<'a> {
    fn visit_pre(&mut self, node: &DomNode) {
        if node.is_element() {
            self.engine.add_node(node.index);
            
            // 简单布局：块级元素垂直排列
            if let crate::dom::node::NodeType::Element(data) = &node.node_type {
                if data.is_block() {
                    // 假设每个块元素高度为 100px
                    self.engine.add_result(
                        node.index,
                        0.0,
                        self.y_offset,
                        800.0, // 简化：固定宽度
                        100.0,
                    );
                    self.y_offset += 100.0;
                } else {
                    self.engine.add_result(
                        node.index,
                        0.0,
                        self.y_offset - 20.0, // 行内元素放在上一行
                        100.0,
                        20.0,
                    );
                }
            }
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
