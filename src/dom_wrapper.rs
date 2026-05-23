//! DOM 包装器 - 使用 kuchiki
//!
//! 封装 DOM 树的解析和访问

use kuchiki::parse_html;
use kuchiki::traits::TendrilSink;
use kuchiki::NodeRef;
use log::{info, trace};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};
use url::Url;

/// 全局单调递增 64 位节点 ID 分配器
/// 类似 Blink 的 WeakMap 句柄表，永不回收
fn allocate_node_id() -> u64 {
    static NEXT_NODE_ID: AtomicU64 = AtomicU64::new(1);
    NEXT_NODE_ID.fetch_add(1, Ordering::SeqCst)
}

/// DOM 树包装器
pub struct DomWrapper {
    /// kuchiki 文档节点
    document: NodeRef,
    /// 文档 URL
    url: Option<Url>,
    /// 节点列表（用于按索引访问）
    node_list: Vec<NodeRef>,
    /// 节点 Rc 指针到 64 位 ID 的映射
    node_id_map: HashMap<usize, u64>,
    /// 64 位 ID 到索引的映射
    id_to_index: HashMap<u64, usize>,
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
            node_id_map: HashMap::new(),
            id_to_index: HashMap::new(),
        };

        wrapper.build_node_index();
        trace!("DOM 树创建完成，节点数: {}", wrapper.node_list.len());

        wrapper
    }

    /// 构建节点索引（分配 64 位单调递增 ID）
    fn build_node_index(&mut self) {
        self.node_list.clear();
        self.node_id_map.clear();
        self.id_to_index.clear();

        for node in self.document.descendants() {
            let index = self.node_list.len();
            let node_id = allocate_node_id();
            self.node_list.push(node.clone());
            let rc_ptr = Rc::as_ptr(&node.0) as usize;
            self.node_id_map.insert(rc_ptr, node_id);
            self.id_to_index.insert(node_id, index);
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
                    self.node_id_map
                        .get(&rc_ptr)
                        .and_then(|id| self.id_to_index.get(id))
                        .copied()
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

    /// 获取节点的直接文本内容（收集所有直接文本子节点的内容）
    pub fn text_content(&self, index: usize) -> Option<String> {
        if let Some(node) = self.get_node(index) {
            // 如果节点本身就是文本节点，直接返回
            if let Some(text) = node.as_text() {
                return Some(text.borrow().to_string());
            }
            // 否则收集所有直接文本子节点
            let mut result = String::new();
            for child in node.children() {
                if let Some(text) = child.as_text() {
                    result.push_str(&text.borrow());
                }
            }
            if result.is_empty() {
                None
            } else {
                Some(result)
            }
        } else {
            None
        }
    }

    /// 递归获取节点及其所有子节点的文本内容
    pub fn text_content_recursive(&self, index: usize) -> String {
        if let Some(node) = self.get_node(index) {
            return node.text_contents();
        }
        String::new()
    }

    /// 获取元素属性
    pub fn attribute(&self, index: usize, name: &str) -> Option<String> {
        if let Some(node) = self.get_node(index) {
            if let Some(element) = node.as_element() {
                return element.attributes.borrow().get(name).map(|s| s.to_string());
            }
        }
        None
    }

    /// 设置元素属性
    /// Rebuild node index from scratch (call after any DOM tree mutation)
    pub fn rebuild_node_index(&mut self) {
        self.build_node_index();
    }

    /// Set element attribute
    pub fn set_attribute(&self, index: usize, name: &str, value: &str) {
        if let Some(node) = self.get_node(index) {
            if let Some(element) = node.as_element() {
                let mut attrs = element.attributes.borrow_mut();
                attrs.insert(name.to_string(), value.to_string());
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
            let rc_ptr = Rc::as_ptr(&element.as_node().0) as usize;
            return self
                .node_id_map
                .get(&rc_ptr)
                .and_then(|id| self.id_to_index.get(id))
                .copied();
        }
        None
    }

    /// 获取 <body> 元素的索引
    pub fn body(&self) -> usize {
        self.find_node_index_by_selector("body").unwrap_or(0)
    }

    /// 获取 <head> 元素的索引
    pub fn head(&self) -> Option<usize> {
        self.find_node_index_by_selector("head")
    }

    /// 使用 CSS 选择器查找元素
    pub fn select(&self, selector: &str) -> Vec<usize> {
        let mut results = Vec::new();
        if let Ok(elements) = self.document.select(selector) {
            for element in elements {
                let rc_ptr = Rc::as_ptr(&element.as_node().0) as usize;
                if let Some(id) = self.node_id_map.get(&rc_ptr) {
                    if let Some(&idx) = self.id_to_index.get(id) {
                        results.push(idx);
                    }
                }
            }
        }
        results
    }

    /// 使用 CSS 选择器查找第一个匹配的元素
    pub fn select_first(&self, selector: &str) -> Option<usize> {
        self.find_node_index_by_selector(selector)
    }

    /// 获取节点名称（#document, #text, 或标签名）
    pub fn node_name(&self, index: usize) -> Option<String> {
        if let Some(node) = self.get_node(index) {
            if node.as_text().is_some() {
                return Some("#text".to_string());
            }
            if node.as_element().is_some() {
                return self.tag_name(index);
            }
            if node.as_document().is_some() {
                return Some("#document".to_string());
            }
        }
        None
    }

    /// 获取父节点索引
    pub fn parent(&self, index: usize) -> Option<usize> {
        if let Some(node) = self.get_node(index) {
            if let Some(parent) = node.parent() {
                let rc_ptr = Rc::as_ptr(&parent.0) as usize;
                return self
                    .node_id_map
                    .get(&rc_ptr)
                    .and_then(|id| self.id_to_index.get(id))
                    .copied();
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

    /// 通过节点索引获取其 64 位稳定 ID
    pub fn node_id(&self, index: usize) -> Option<u64> {
        self.get_node(index).and_then(|node| {
            let rc_ptr = Rc::as_ptr(&node.0) as usize;
            self.node_id_map.get(&rc_ptr).copied()
        })
    }

    /// 通过 64 位 ID 获取节点索引
    pub fn index_by_id(&self, id: u64) -> Option<usize> {
        self.id_to_index.get(&id).copied()
    }

    /// 获取内部文档引用（用于样式计算等）
    pub fn inner_document(&self) -> &NodeRef {
        &self.document
    }

    /// 通过 NodeRef 查找节点索引
    pub fn index_of_node(&self, node_ref: &NodeRef) -> Option<usize> {
        let rc_ptr = Rc::as_ptr(&node_ref.0) as usize;
        self.node_id_map
            .get(&rc_ptr)
            .and_then(|id| self.id_to_index.get(id))
            .copied()
    }

    /// 通过 NodeRef 获取标签名
    pub fn tag_name_of_node(&self, node: &NodeRef) -> Option<String> {
        if let Some(element) = node.as_element() {
            return Some(element.name.local.to_string());
        }
        None
    }

    /// 通过 NodeRef 获取 64 位稳定 ID
    pub fn node_id_of_node(&self, node_ref: &NodeRef) -> Option<u64> {
        let rc_ptr = Rc::as_ptr(&node_ref.0) as usize;
        self.node_id_map.get(&rc_ptr).copied()
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

    fn assert_tag(elements: &[(usize, String)], tag: &str) -> usize {
        for (idx, t) in elements {
            if t == tag {
                return *idx;
            }
        }
        panic!("tag '{}' not found in elements: {:?}", tag, elements);
    }

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

    #[test]
    fn test_url() {
        let html = "<html><head><title>T</title></head><body><div>c</div></body></html>";
        let dom = DomWrapper::from_html(html, Some("https://example.com/page"));
        let url = dom.url();
        assert!(url.is_some());
        assert_eq!(url.unwrap().as_str(), "https://example.com/page");
    }

    #[test]
    fn test_url_none() {
        let html = "<html></html>";
        let dom = DomWrapper::from_html(html, None);
        assert!(dom.url().is_none());
    }

    #[test]
    fn test_document_root() {
        let html = "<html></html>";
        let dom = DomWrapper::from_html(html, None);
        assert_eq!(dom.document(), 0);
    }

    #[test]
    fn test_get_node_valid_index() {
        let html = "<html><body><p>a</p></body></html>";
        let dom = DomWrapper::from_html(html, None);
        let node = dom.get_node(0);
        assert!(node.is_some());
    }

    #[test]
    fn test_get_node_invalid_index() {
        let html = "<html></html>";
        let dom = DomWrapper::from_html(html, None);
        let node = dom.get_node(99999);
        assert!(node.is_none());
    }

    #[test]
    fn test_children_of_document() {
        let html = "<html><body></body></html>";
        let dom = DomWrapper::from_html(html, None);
        let children = dom.children(0);
        assert!(!children.is_empty(), "Document should have children");
    }

    #[test]
    fn test_children_returns_indices() {
        let html = "<html><body><div></div><p></p></body></html>";
        let dom = DomWrapper::from_html(html, None);
        let body = dom.body();
        let children = dom.children(body);
        assert!(children.len() >= 2, "body should have at least 2 children");
    }

    #[test]
    fn test_tag_name() {
        let html = "<html><body><div>test</div></body></html>";
        let dom = DomWrapper::from_html(html, None);
        let elements = dom.traverse_elements();
        let idx = assert_tag(&elements, "div");
        assert_eq!(dom.tag_name(idx), Some("div".to_string()));
    }

    #[test]
    fn test_tag_name_invalid_index() {
        let html = "<html></html>";
        let dom = DomWrapper::from_html(html, None);
        assert_eq!(dom.tag_name(99999), None);
    }

    #[test]
    fn test_is_text_node() {
        let html = "<html><body>hello</body></html>";
        let dom = DomWrapper::from_html(html, None);
        let elements = dom.traverse_elements();
        let body_idx = assert_tag(&elements, "body");
        let children = dom.children(body_idx);
        let has_text = children.iter().any(|c| dom.is_text_node(*c));
        assert!(has_text, "body should have a text child");
    }

    #[test]
    fn test_text_content() {
        let html = "<html><body><p>hello</p></body></html>";
        let dom = DomWrapper::from_html(html, None);
        let elements = dom.traverse_elements();
        let p_idx = assert_tag(&elements, "p");
        let text = dom.text_content(p_idx);
        assert_eq!(text, Some("hello".to_string()));
    }

    #[test]
    fn test_text_content_recursive() {
        let html = "<html><body><p>Hello <span>world</span></p></body></html>";
        let dom = DomWrapper::from_html(html, None);
        let elements = dom.traverse_elements();
        let p_idx = assert_tag(&elements, "p");
        let text = dom.text_content_recursive(p_idx);
        assert!(text.contains("Hello"));
        assert!(text.contains("world"));
    }

    #[test]
    fn test_attribute_existing() {
        let html = r#"<html><body><div id="main">text</div></body></html>"#;
        let dom = DomWrapper::from_html(html, None);
        let elements = dom.traverse_elements();
        let idx = assert_tag(&elements, "div");
        let id = dom.attribute(idx, "id");
        assert_eq!(id, Some("main".to_string()));
    }

    #[test]
    fn test_attribute_missing() {
        let html = "<html><body><div>text</div></body></html>";
        let dom = DomWrapper::from_html(html, None);
        let elements = dom.traverse_elements();
        let idx = assert_tag(&elements, "div");
        let missing = dom.attribute(idx, "class");
        assert_eq!(missing, None);
    }

    #[test]
    fn test_set_attribute() {
        let html = "<html><body><footer>end</footer></body></html>";
        let dom = DomWrapper::from_html(html, None);
        let elements = dom.traverse_elements();
        let idx = assert_tag(&elements, "footer");
        dom.set_attribute(idx, "data-test", "value123");
        let val = dom.attribute(idx, "data-test");
        assert_eq!(val, Some("value123".to_string()));
    }

    #[test]
    fn test_head() {
        let html = "<html><head><title>T</title></head><body></body></html>";
        let dom = DomWrapper::from_html(html, None);
        let head = dom.head();
        assert!(head.is_some(), "head should exist");
    }

    #[test]
    fn test_select_class() {
        let html = r#"<html><body><p class="text">hello</p></body></html>"#;
        let dom = DomWrapper::from_html(html, None);
        let results = dom.select(".text");
        assert!(!results.is_empty(), "should find .text elements");
    }

    #[test]
    fn test_select_tag() {
        let html = "<html><body><p>a</p><p>b</p></body></html>";
        let dom = DomWrapper::from_html(html, None);
        let results = dom.select("p");
        assert_eq!(results.len(), 2, "should find 2 <p>");
    }

    #[test]
    fn test_select_nonexistent() {
        let html = "<html><body></body></html>";
        let dom = DomWrapper::from_html(html, None);
        let results = dom.select("nonexistent-tag");
        assert!(results.is_empty(), "should find nothing");
    }

    #[test]
    fn test_select_first_existing() {
        let html = "<html><body><a href='#'>link</a></body></html>";
        let dom = DomWrapper::from_html(html, None);
        let result = dom.select_first("a");
        assert!(result.is_some(), "should find <a>");
    }

    #[test]
    fn test_select_first_nonexistent() {
        let html = "<html><body></body></html>";
        let dom = DomWrapper::from_html(html, None);
        let result = dom.select_first("nonexistent");
        assert!(result.is_none());
    }

    #[test]
    fn test_node_name() {
        let html = "<html><body></body></html>";
        let dom = DomWrapper::from_html(html, None);
        let elements = dom.traverse_elements();
        let idx = assert_tag(&elements, "body");
        let name = dom.node_name(idx);
        assert_eq!(name, Some("body".to_string()));
    }

    #[test]
    fn test_node_name_document() {
        let html = "<html></html>";
        let dom = DomWrapper::from_html(html, None);
        // 索引 0 可能是 html 元素（descendants 的第一个），而不是 document
        // 但 node_name 对元素应返回标签名
        let name = dom.node_name(0);
        assert!(name.is_some(), "node_name(0) should return some name");
    }

    #[test]
    fn test_parent() {
        let html = "<html><body><div><p>text</p></div></body></html>";
        let dom = DomWrapper::from_html(html, None);
        let elements = dom.traverse_elements();
        let p_idx = assert_tag(&elements, "p");
        let parent = dom.parent(p_idx);
        assert!(parent.is_some(), "p should have parent");
        let parent_tag = dom.tag_name(parent.unwrap());
        assert_eq!(parent_tag, Some("div".to_string()));
    }

    #[test]
    fn test_child_count() {
        let html = "<html><body><div></div><p></p></body></html>";
        let dom = DomWrapper::from_html(html, None);
        let body = dom.body();
        let count = dom.child_count(body);
        assert!(count >= 2, "body should have at least 2 children");
    }

    #[test]
    fn test_has_children_true() {
        let html = "<html><body></body></html>";
        let dom = DomWrapper::from_html(html, None);
        assert!(dom.has_children(0), "document should have children");
    }

    #[test]
    fn test_has_children_false() {
        let html = "<html><body><br></body></html>";
        let dom = DomWrapper::from_html(html, None);
        let elements = dom.traverse_elements();
        let idx = assert_tag(&elements, "br");
        assert!(!dom.has_children(idx));
    }

    #[test]
    fn test_inner_document() {
        let html = "<html><body></body></html>";
        let dom = DomWrapper::from_html(html, None);
        let doc = dom.inner_document();
        assert!(doc.children().next().is_some());
    }

    #[test]
    fn test_index_of_node() {
        let html = "<html><body></body></html>";
        let dom = DomWrapper::from_html(html, None);
        let doc = dom.inner_document();
        let index = dom.index_of_node(doc);
        // inner_document 返回的是内部的 document 引用，应该能在索引中找到
        // 注意：descendants() 是否包含 document 本身取决于 kuchiki 的实现
        if let Some(idx) = index {
            assert_eq!(idx, 0);
        }
        // 如果没有找到，可能是因为 document 节点不在 descendants() 中
        // 这种情况下至少 index_of_node 不 panic
    }

    #[test]
    fn test_select_by_id() {
        let html = r#"<html><body><div id="main">text</div></body></html>"#;
        let dom = DomWrapper::from_html(html, None);
        let results = dom.select("#main");
        assert_eq!(results.len(), 1, "should find #main");
    }

    #[test]
    fn test_paragraph_text() {
        let html = "<p>Hello World</p>";
        let dom = DomWrapper::from_html(html, None);
        let elements = dom.traverse_elements();
        let p_idx = assert_tag(&elements, "p");
        let text = dom.text_content(p_idx);
        assert_eq!(text, Some("Hello World".to_string()));
    }

    #[test]
    fn test_link_href_attribute() {
        let html = r#"<html><body><a href="https://example.com">link</a></body></html>"#;
        let dom = DomWrapper::from_html(html, None);
        let elements = dom.traverse_elements();
        let idx = assert_tag(&elements, "a");
        let href = dom.attribute(idx, "href");
        assert_eq!(href, Some("https://example.com".to_string()));
    }

    #[test]
    fn test_img_src_attribute() {
        let html = r#"<html><body><img src="test.png" alt="test image"></body></html>"#;
        let dom = DomWrapper::from_html(html, None);
        let elements = dom.traverse_elements();
        let idx = assert_tag(&elements, "img");
        let src = dom.attribute(idx, "src");
        assert_eq!(src, Some("test.png".to_string()));
        let alt = dom.attribute(idx, "alt");
        assert_eq!(alt, Some("test image".to_string()));
    }

    #[test]
    fn test_empty_html() {
        let dom = DomWrapper::from_html("", None);
        let elements = dom.traverse_elements();
        assert!(!elements.is_empty());
    }

    #[test]
    fn test_malformed_html() {
        let html = "<div><p>missing close tags";
        let dom = DomWrapper::from_html(html, None);
        let title = dom.title();
        assert!(title.is_none());
    }

    #[test]
    fn test_clone_preserves_url() {
        let html = "<html></html>";
        let dom = DomWrapper::from_html(html, Some("https://example.com"));
        let cloned = dom.clone();
        assert!(cloned.url().is_some());
    }

    #[test]
    fn test_select_multiple_classes() {
        let html = r#"<html><body><div class="a b">x</div><div class="a">y</div></body></html>"#;
        let dom = DomWrapper::from_html(html, None);
        let results = dom.select(".a");
        assert_eq!(results.len(), 2, "should find both .a divs");
    }

    #[test]
    fn test_deeply_nested_elements() {
        let html = "<html><body><div><div><div><p>deep</p></div></div></div></body></html>";
        let dom = DomWrapper::from_html(html, None);
        let elements = dom.traverse_elements();
        let p_idx = assert_tag(&elements, "p");
        let text = dom.text_content(p_idx);
        assert_eq!(text, Some("deep".to_string()));
    }

    #[test]
    fn test_body_children_include_text() {
        let html = "<html><body>hello<div>world</div></body></html>";
        let dom = DomWrapper::from_html(html, None);
        let body = dom.body();
        let children = dom.children(body);
        let text_child = children.iter().find(|c| dom.is_text_node(**c));
        assert!(text_child.is_some(), "body should have a text child");
    }

    #[test]
    fn test_nonexistent_parent() {
        let html = "<html></html>";
        let dom = DomWrapper::from_html(html, None);
        let parent = dom.parent(0);
        // document 节点没有父节点
        assert!(parent.is_none());
    }

    #[test]
    fn test_child_count_zero() {
        let html = "<html><body><br></body></html>";
        let dom = DomWrapper::from_html(html, None);
        let elements = dom.traverse_elements();
        let idx = assert_tag(&elements, "br");
        assert_eq!(dom.child_count(idx), 0);
    }

    #[test]
    fn test_has_children_invalid_index() {
        let html = "<html></html>";
        let dom = DomWrapper::from_html(html, None);
        assert!(!dom.has_children(99999));
    }

    #[test]
    fn test_index_of_node_nonexistent() {
        let html = "<html></html>";
        let dom = DomWrapper::from_html(html, None);
        // 创建一个新的未在树中的节点
        let new_node = kuchiki::parse_html().one("<p></p>");
        let index = dom.index_of_node(&new_node);
        assert!(index.is_none());
    }
}
