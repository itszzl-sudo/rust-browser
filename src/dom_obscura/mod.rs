//! DOM 适配层 —— 基于 obscura-dom
//!
//! 提供与现有代码兼容的 DOM 访问接口。

use obscura_dom::tree::{DomTree, Node, NodeData, NodeId};
use obscura_dom::tree_sink::parse_html;

pub use obscura_dom::tree::{Attribute, NodeData as ObscuraNodeData};

/// DOM 包装器 —— 替代 kuchiki 的 DomWrapper
pub struct ObscuraDom {
    tree: DomTree,
}

impl ObscuraDom {
    /// 从 HTML 字符串解析 DOM
    pub fn from_html(html: &str) -> Self {
        let tree = parse_html(html);
        Self { tree }
    }

    /// 获取文档节点 ID
    pub fn document(&self) -> NodeId {
        self.tree.document()
    }

    /// 获取节点（返回 clone，因 obscura-dom 不暴露引用）
    pub fn get_node(&self, id: NodeId) -> Option<Node> {
        self.tree.get_node(id)
    }

    /// 获取子节点 ID 列表
    pub fn children(&self, id: NodeId) -> Vec<NodeId> {
        let mut result = Vec::new();
        if let Some(node) = self.tree.get_node(id) {
            let mut child = node.first_child;
            while let Some(cid) = child {
                result.push(cid);
                child = self.tree.get_node(cid).and_then(|n| n.next_sibling);
            }
        }
        result
    }

    /// 获取标签名（仅元素节点）
    pub fn tag_name(&self, id: NodeId) -> Option<String> {
        self.tree.get_node(id).and_then(|n| {
            if let NodeData::Element { name, .. } = &n.data {
                Some(name.local.to_string())
            } else {
                None
            }
        })
    }

    /// 获取属性值
    pub fn get_attribute(&self, id: NodeId, name: &str) -> Option<String> {
        self.tree.get_node(id).and_then(|n| {
            if let NodeData::Element { attrs, .. } = &n.data {
                attrs
                    .iter()
                    .find(|a| a.name.local.as_ref() == name)
                    .map(|a| a.value.clone())
            } else {
                None
            }
        })
    }

    /// 获取文本内容（仅文本节点）
    pub fn text_content(&self, id: NodeId) -> Option<String> {
        self.tree.get_node(id).and_then(|n| match &n.data {
            NodeData::Text { contents } => Some(contents.clone()),
            _ => None,
        })
    }

    /// 获取后代所有文本
    pub fn text_content_recursive(&self, id: NodeId) -> String {
        let mut result = String::new();
        self.collect_text_recursive(id, &mut result);
        result
    }

    fn collect_text_recursive(&self, id: NodeId, result: &mut String) {
        if let Some(node) = self.tree.get_node(id) {
            if let NodeData::Text { contents } = &node.data {
                result.push_str(contents);
            }
            for child in self.children(id) {
                self.collect_text_recursive(child, result);
            }
        }
    }

    /// 查找第一个匹配标签选择器的元素
    pub fn select_first(&self, selector: &str) -> Option<NodeId> {
        let sel = selector.trim();
        self.select_all_recursive(self.document(), sel)
            .into_iter()
            .next()
    }

    fn select_all_recursive(&self, id: NodeId, selector: &str) -> Vec<NodeId> {
        let mut result = Vec::new();
        if let Some(node) = self.tree.get_node(id) {
            if let NodeData::Element { name, .. } = &node.data {
                let tag = name.local.as_ref();
                if tag == selector || selector == "*" {
                    result.push(id);
                }
            }
        }
        for child in self.children(id) {
            result.extend(self.select_all_recursive(child, selector));
        }
        result
    }

    /// 获取 body 元素
    pub fn body(&self) -> Option<NodeId> {
        self.select_first("body")
    }

    /// 获取页面标题
    pub fn title(&self) -> Option<String> {
        let title_id = self.select_first("title")?;
        Some(self.text_content_recursive(title_id).trim().to_string())
    }

    /// 获取 document 的 children
    pub fn document_children(&self) -> Vec<NodeId> {
        self.children(self.document())
    }
}
