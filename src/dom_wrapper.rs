//! DOM 包装器 - 使用 kuchiki
//!
//! 封装 DOM 树的解析和访问

use kuchiki::parse_html;
use kuchiki::traits::TendrilSink;
use kuchiki::NodeRef;
use log::{info, trace};
use std::collections::HashMap;
use std::rc::Rc;
use url::Url;

/// DOM 树包装器
pub struct DomWrapper {
    /// kuchiki 文档节点
    document: NodeRef,
    /// 文档 URL
    url: Option<Url>,
    /// 节点列表（用于按索引访问）
    node_list: Vec<NodeRef>,
    /// 节点到索引的映射（使用 Rc 指针）
    node_to_index: HashMap<usize, usize>,
}

impl DomWrapper {
    /// 从 HTML 字符串解析 DOM
    pub fn from_html(html: &str, url: Option<&str>) -> Self {
        info!("使用 kuchiki 解析 HTML 文档");
        let url = url.and_then(|u| Url::parse(u).ok());

        let document = parse_html().one(html);
        let mut wrapper = Self {
            document,
            url,
            node_list: Vec::new(),
            node_to_index: HashMap::new(),
        };

        wrapper.build_node_index();
        trace!("DOM 树创建完成，节点数: {}", wrapper.node_list.len());

        wrapper
    }

    /// 构建节点索引
    fn build_node_index(&mut self) {
        self.node_list.clear();
        self.node_to_index.clear();

        for node in self.document.descendants() {
            let index = self.node_list.len();
            self.node_list.push(node.clone());
            let rc_ptr = Rc::as_ptr(&node.0) as usize;
            self.node_to_index.insert(rc_ptr, index);
        }
    }

    /// 获取文档 URL
    pub fn url(&self) -> Option<&Url> {
        self.url.as_ref()
    }

    /// 获取根文档节点
    pub fn document(&self) -> usize {
        0
    }

    /// 根据索引获取节点
    pub fn get_node(&self, index: usize) -> Option<NodeRef> {
        self.node_list.get(index).cloned()
    }

    /// 获取节点的子节点索引
    pub fn children(&self, index: usize) -> Vec<usize> {
        if let Some(node) = self.get_node(index) {
            node.children()
                .filter_map(|child| {
                    let rc_ptr = Rc::as_ptr(&child.0) as usize;
                    self.node_to_index.get(&rc_ptr).copied()
                })
                .collect()
        } else {
            Vec::new()
        }
    }

    /// 获取节点的标签名（仅元素节点）
    pub fn tag_name(&self, index: usize) -> Option<String> {
        if let Some(node) = self.get_node(index) {
            if let Some(element) = node.as_element() {
                return Some(element.name.local.to_string());
            }
        }
        None
    }

    /// 检查节点是否为文本节点
    pub fn is_text_node(&self, index: usize) -> bool {
        if let Some(node) = self.get_node(index) {
            return node.as_text().is_some();
        }
        false
    }

    /// 获取节点的文本内容（仅文本节点）
    pub fn text_content(&self, index: usize) -> Option<String> {
        if let Some(node) = self.get_node(index) {
            if let Some(text) = node.as_text() {
                return Some(text.borrow().to_string());
            }
        }
        None
    }

    /// 获取整棵子树的文本内容
    pub fn text_content_recursive(&self, index: usize) -> String {
        if let Some(node) = self.get_node(index) {
            let mut result = String::new();
            for descendant in node.descendants() {
                if let Some(text) = descendant.as_text() {
                    result.push_str(&text.borrow());
                }
            }
            return result;
        }
        String::new()
    }

    /// 获取属性值
    pub fn attribute(&self, index: usize, name: &str) -> Option<String> {
        if let Some(node) = self.get_node(index) {
            if let Some(element) = node.as_element() {
                return element.attributes.borrow().get(name).map(|s| s.to_string());
            }
        }
        None
    }

    /// 设置属性值
    pub fn set_attribute(&self, index: usize, name: &str, value: &str) {
        if let Some(node) = self.get_node(index) {
            if let Some(element) = node.as_element() {
                element
                    .attributes
                    .borrow_mut()
                    .insert(name.to_string(), value.to_string());
            }
        }
    }

    /// 遍历所有元素节点
    pub fn traverse_elements(&self) -> Vec<(usize, String)> {
        let mut elements = Vec::new();
        self.traverse_elements_recursive(0, &mut elements);
        elements
    }

    fn traverse_elements_recursive(&self, current: usize, elements: &mut Vec<(usize, String)>) {
        if let Some(tag) = self.tag_name(current) {
            elements.push((current, tag));
        }

        for child in self.children(current) {
            self.traverse_elements_recursive(child, elements);
        }
    }

    /// 获取页面标题（<title> 标签）
    pub fn title(&self) -> Option<String> {
        if let Ok(title_node) = self.document.select_first("title") {
            let title_text = title_node.text_contents();
            if !title_text.trim().is_empty() {
                return Some(title_text.trim().to_string());
            }
        }
        None
    }

    fn find_node_index_by_selector(&self, selector: &str) -> Option<usize> {
        if let Ok(element) = self.document.select_first(selector) {
            let node = element.as_node();
            let rc_ptr = Rc::as_ptr(&node.0) as usize;
            return self.node_to_index.get(&rc_ptr).copied();
        }
        None
    }

    /// 获取 body 元素
    pub fn body(&self) -> usize {
        self.find_node_index_by_selector("body").unwrap_or(0)
    }

    /// 获取 head 元素
    pub fn head(&self) -> Option<usize> {
        self.find_node_index_by_selector("head")
    }

    /// 根据 CSS 选择器查找元素
    pub fn select(&self, selector: &str) -> Vec<usize> {
        if let Ok(elements) = self.document.select(selector) {
            elements
                .filter_map(|element| {
                    let node = element.as_node();
                    let rc_ptr = Rc::as_ptr(&node.0) as usize;
                    self.node_to_index.get(&rc_ptr).copied()
                })
                .collect()
        } else {
            Vec::new()
        }
    }

    /// 查找第一个匹配的元素
    pub fn select_first(&self, selector: &str) -> Option<usize> {
        self.find_node_index_by_selector(selector)
    }

    /// 获取节点名称（兼容旧 API）
    pub fn node_name(&self, index: usize) -> Option<String> {
        if let Some(node) = self.get_node(index) {
            if node.as_element().is_some() {
                if let Some(element) = node.as_element() {
                    return Some(element.name.local.to_string());
                }
            } else if node.as_text().is_some() {
                return Some("#text".to_string());
            }
        }
        None
    }

    /// 获取父节点
    pub fn parent(&self, index: usize) -> Option<usize> {
        if let Some(node) = self.get_node(index) {
            if let Some(parent) = node.parent() {
                let rc_ptr = Rc::as_ptr(&parent.0) as usize;
                return self.node_to_index.get(&rc_ptr).copied();
            }
        }
        None
    }

    /// 计算子节点数量
    pub fn child_count(&self, index: usize) -> usize {
        self.children(index).len()
    }

    /// 判断节点是否有子节点
    pub fn has_children(&self, index: usize) -> bool {
        if let Some(node) = self.get_node(index) {
            return node.children().next().is_some();
        }
        false
    }

    /// 获取内部文档引用（用于样式计算等）
    pub fn inner_document(&self) -> &NodeRef {
        &self.document
    }

    /// Look up node index by Rc pointer (fast O(1) lookup)
    pub fn index_of_node(&self, node_ref: &NodeRef) -> Option<usize> {
        let rc_ptr = Rc::as_ptr(&node_ref.0) as usize;
        self.node_to_index.get(&rc_ptr).copied()
    }
}

impl Clone for DomWrapper {
    fn clone(&self) -> Self {
        Self::from_html("<!-- cloned -->", self.url.as_ref().map(|u| u.as_str()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_html() {
        let html = "<html><head><title>Test</title></head><body><p>Hello</p></body></html>";
        let dom = DomWrapper::from_html(html, Some("http://example.com"));

        assert_eq!(dom.title(), Some("Test".to_string()));
        assert!(dom.body() > 0);
    }

    #[test]
    fn test_traverse_elements() {
        let html = "<html><body><div><p>Text</p></div></body></html>";
        let dom = DomWrapper::from_html(html, None);

        let elements = dom.traverse_elements();
        assert!(elements.len() >= 4);
    }
}
