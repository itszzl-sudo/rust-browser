//! DOM Visitor - 遍历 DOM 树
//!
//! 提供访问者模式遍历 DOM 树

use super::node::{DomNode, DomTree, NodeType};
use log::debug;

/// DOM 遍历模式
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum VisitMode {
    /// 前序遍历（先访问节点，再访问子节点）
    PreOrder,
    /// 后序遍历（先访问子节点，再访问节点）
    PostOrder,
    /// 层序遍历（广度优先）
    LevelOrder,
}

/// DOM 访问者 trait
pub trait DomVisitor {
    /// 访问节点前
    fn visit_pre(&mut self, _node: &DomNode) {}
    
    /// 访问节点后
    fn visit_post(&mut self, _node: &DomNode) {}
    
    /// 访问文本节点
    fn visit_text(&mut self, _node: &DomNode, _text: &str) {}
    
    /// 是否继续遍历子节点
    fn should_visit_children(&self, _node: &DomNode) -> bool {
        true
    }
}

/// DOM 遍历器
pub struct DomTraverser;

impl DomTraverser {
    /// 前序遍历
    pub fn pre_order(tree: &DomTree, visitor: &mut impl DomVisitor) {
        if let Some(root) = tree.root() {
            Self::traverse_node(tree, root.index, visitor, VisitMode::PreOrder);
        }
    }

    /// 后序遍历
    pub fn post_order(tree: &DomTree, visitor: &mut impl DomVisitor) {
        if let Some(root) = tree.root() {
            Self::traverse_node(tree, root.index, visitor, VisitMode::PostOrder);
        }
    }

    /// 层序遍历
    pub fn level_order(tree: &DomTree, visitor: &mut impl DomVisitor) {
        if let Some(root) = tree.root() {
            Self::traverse_level(tree, &[root.index], visitor);
        }
    }

    fn traverse_node(tree: &DomTree, index: usize, visitor: &mut impl DomVisitor, mode: VisitMode) {
        if let Some(node) = tree.get(index) {
            match mode {
                VisitMode::PreOrder => {
                    visitor.visit_pre(node);
                    
                    // 处理文本节点
                    if let NodeType::Text(text) = &node.node_type {
                        visitor.visit_text(node, text);
                    }
                    
                    // 遍历子节点
                    if visitor.should_visit_children(node) {
                        for &child_index in &node.children {
                            Self::traverse_node(tree, child_index, visitor, mode);
                        }
                    }
                    
                    visitor.visit_post(node);
                }
                VisitMode::PostOrder => {
                    // 先遍历子节点
                    if visitor.should_visit_children(node) {
                        for &child_index in &node.children {
                            Self::traverse_node(tree, child_index, visitor, mode);
                        }
                    }
                    
                    visitor.visit_pre(node);
                    visitor.visit_post(node);
                }
                _ => {}
            }
        }
    }

    fn traverse_level(tree: &DomTree, indices: &[usize], visitor: &mut impl DomVisitor) {
        if indices.is_empty() {
            return;
        }

        let mut next_level = Vec::new();

        for &index in indices {
            if let Some(node) = tree.get(index) {
                visitor.visit_pre(node);
                
                // 处理文本节点
                if let NodeType::Text(text) = &node.node_type {
                    visitor.visit_text(node, text);
                }
                
                if visitor.should_visit_children(node) {
                    next_level.extend(&node.children);
                }
                
                visitor.visit_post(node);
            }
        }

        Self::traverse_level(tree, &next_level, visitor);
    }
}

/// 打印 DOM 树的访问者
pub struct PrintVisitor {
    indent: usize,
}

impl PrintVisitor {
    pub fn new() -> Self {
        Self { indent: 0 }
    }
}

impl DomVisitor for PrintVisitor {
    fn visit_pre(&mut self, node: &DomNode) {
        let spaces = "  ".repeat(self.indent);
        debug!("{}{}", spaces, node);
        self.indent += 1;
    }

    fn visit_post(&mut self, _node: &DomNode) {
        self.indent = self.indent.saturating_sub(1);
    }
}

/// 收集文本内容的访问者
pub struct TextCollector {
    texts: Vec<String>,
}

impl TextCollector {
    pub fn new() -> Self {
        Self { texts: Vec::new() }
    }

    pub fn into_text(self) -> String {
        self.texts.join("")
    }

    pub fn texts(&self) -> &[String] {
        &self.texts
    }
}

impl DomVisitor for TextCollector {
    fn visit_text(&mut self, _node: &DomNode, text: &str) {
        self.texts.push(text.to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_text_collector() {
        let tree = DomTree::new();
        let mut collector = TextCollector::new();
        DomTraverser::pre_order(&tree, &mut collector);
    }
}
