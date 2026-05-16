//! DOM 节点 - 文档树的基本单元
//!
//! 支持元素节点、文本节点、注释等

use log::debug;
use std::collections::HashMap;
use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub enum NodeType {
    Document,
    Element(ElementData),
    Text(String),
    Comment(String),
    Doctype(String),
    DocumentFragment,
}

impl Default for NodeType {
    fn default() -> Self {
        Self::Document
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ElementData {
    pub tag_name: String,
    pub attributes: HashMap<String, String>,
}

impl ElementData {
    pub fn new(tag_name: &str) -> Self {
        Self {
            tag_name: tag_name.to_lowercase(),
            attributes: HashMap::new(),
        }
    }

    pub fn get_attribute(&self, name: &str) -> Option<&str> {
        self.attributes.get(name).map(|s| s.as_str())
    }

    pub fn set_attribute(&mut self, name: &str, value: &str) {
        debug!("设置属性: {} = {}", name, value);
        self.attributes.insert(name.to_string(), value.to_string());
    }

    pub fn remove_attribute(&mut self, name: &str) {
        self.attributes.remove(name);
    }

    pub fn has_class(&self, class: &str) -> bool {
        self.get_attribute("class")
            .map(|c| c.split_whitespace().any(|x| x == class))
            .unwrap_or(false)
    }

    pub fn classes(&self) -> Vec<&str> {
        self.get_attribute("class")
            .map(|c| c.split_whitespace().collect())
            .unwrap_or_default()
    }

    pub fn is_block(&self) -> bool {
        matches!(
            self.tag_name.as_str(),
            "div" | "p" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6"
                | "ul" | "ol" | "li" | "table" | "form" | "header"
                | "footer" | "nav" | "section" | "article" | "aside"
                | "blockquote" | "hr" | "pre" | "address"
        )
    }

    pub fn is_inline(&self) -> bool {
        matches!(
            self.tag_name.as_str(),
            "span" | "a" | "strong" | "em" | "b" | "i" | "u"
                | "small" | "mark" | "sub" | "sup" | "code" | "kbd"
                | "samp" | "var" | "cite" | "abbr" | "data" | "time"
        )
    }
}

#[derive(Debug, Clone)]
pub struct DomNode {
    pub node_type: NodeType,
    pub parent: Option<usize>,
    pub children: Vec<usize>,
    pub index: usize,
}

impl DomNode {
    pub fn document() -> Self {
        Self {
            node_type: NodeType::Document,
            parent: None,
            children: Vec::new(),
            index: 0,
        }
    }

    pub fn element(tag_name: &str) -> Self {
        Self {
            node_type: NodeType::Element(ElementData::new(tag_name)),
            parent: None,
            children: Vec::new(),
            index: 0,
        }
    }

    pub fn text(content: &str) -> Self {
        Self {
            node_type: NodeType::Text(content.to_string()),
            parent: None,
            children: Vec::new(),
            index: 0,
        }
    }

    pub fn comment(content: &str) -> Self {
        Self {
            node_type: NodeType::Comment(content.to_string()),
            parent: None,
            children: Vec::new(),
            index: 0,
        }
    }

    pub fn tag_name(&self) -> Option<&str> {
        match &self.node_type {
            NodeType::Element(data) => Some(&data.tag_name),
            _ => None,
        }
    }

    pub fn text_content(&self) -> String {
        match &self.node_type {
            NodeType::Text(text) => text.clone(),
            _ => String::new(),
        }
    }

    pub fn is_element(&self) -> bool {
        matches!(self.node_type, NodeType::Element(_))
    }

    pub fn is_text(&self) -> bool {
        matches!(self.node_type, NodeType::Text(_))
    }

    pub fn is_block(&self) -> bool {
        match &self.node_type {
            NodeType::Element(data) => data.is_block(),
            _ => false,
        }
    }
}

impl Default for DomNode {
    fn default() -> Self {
        Self::document()
    }
}

impl fmt::Display for DomNode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.node_type {
            NodeType::Document => write!(f, "#document"),
            NodeType::Element(data) => write!(f, "<{}>", data.tag_name),
            NodeType::Text(text) => write!(f, "\"{}\"", text),
            NodeType::Comment(text) => write!(f, "<!-- {} -->", text),
            NodeType::Doctype(doctype) => write!(f, "<!DOCTYPE {}>", doctype),
            NodeType::DocumentFragment => write!(f, "#document-fragment"),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct DomTree {
    nodes: Vec<DomNode>,
}

impl DomTree {
    pub fn new() -> Self {
        Self { nodes: Vec::new() }
    }

    pub fn set_root(&mut self, node: DomNode) {
        self.nodes.clear();
        self.nodes.push(node);
    }

    pub fn add_node(&mut self, node: DomNode) -> usize {
        let index = self.nodes.len();
        let mut node = node;
        node.index = index;
        self.nodes.push(node);
        index
    }

    pub fn get(&self, index: usize) -> Option<&DomNode> {
        self.nodes.get(index)
    }

    pub fn get_mut(&mut self, index: usize) -> Option<&mut DomNode> {
        self.nodes.get_mut(index)
    }

    pub fn root(&self) -> Option<&DomNode> {
        self.nodes.first()
    }

    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    pub fn append_child(&mut self, parent_index: usize, child_index: usize) {
        if let Some(parent) = self.nodes.get_mut(parent_index) {
            parent.children.push(child_index);
        }
        if let Some(child) = self.nodes.get_mut(child_index) {
            child.parent = Some(parent_index);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_element_creation() {
        let elem = DomNode::element("div");
        assert!(elem.is_element());
        assert_eq!(elem.tag_name(), Some("div"));
    }

    #[test]
    fn test_text_node() {
        let text = DomNode::text("Hello");
        assert!(text.is_text());
        assert_eq!(text.text_content(), "Hello");
    }

    #[test]
    fn test_block_element() {
        let div = ElementData::new("div");
        assert!(div.is_block());

        let span = ElementData::new("span");
        assert!(!span.is_block());
    }
}
