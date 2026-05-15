//! DOM 包装器 - 使用 obscura-dom
//!
//! 封装 DOM 树的解析和访问
use log::{info, trace};
use obscura_dom::{DomTree, Node, NodeData, NodeId, parse_html};
use url::Url;

/// DOM 树包装器
pub struct DomWrapper {
    /// Obscura DOM 树
    pub dom: DomTree,
    /// 文档 URL
    url: Option<Url>,
}

impl DomWrapper {
    /// 从 HTML 字符串解析 DOM
    pub fn from_html(html: &str, url: Option<&str>) -> Self {
        info!("解析 HTML 文档");
        let url = url.and_then(|u| Url::parse(u).ok());
        let dom = parse_html(html);
        
        trace!("DOM 树创建完成");
        
        Self { dom, url }
    }

    /// 获取文档 URL
    pub fn url(&self) -> Option<&Url> {
        self.url.as_ref()
    }

    /// 获取文档节点
    pub fn document(&self) -> NodeId {
        self.dom.document()
    }

    /// 获取节点（直接返回 obscura 的 Node 类型）
    pub fn get_node(&self, id: NodeId) -> Option<Node> {
        self.dom.get_node(id)
    }

    /// 获取节点的子节点
    pub fn children(&self, id: NodeId) -> Vec<NodeId> {
        self.dom.children(id)
    }

    /// 获取节点的标签名（仅元素节点）
    pub fn tag_name(&self, id: NodeId) -> Option<String> {
        let node = self.get_node(id)?;
        match node.data {
            NodeData::Element { ref name, .. } => {
                Some(name.local.to_string())
            }
            _ => None
        }
    }

    /// 获取节点的文本内容（仅文本节点）
    pub fn text_content(&self, id: NodeId) -> Option<String> {
        let node = self.get_node(id)?;
        match node.data {
            NodeData::Text { ref contents, .. } => {
                Some(contents.clone())
            }
            _ => None
        }
    }

    /// 获取整棵子树的文本内容
    pub fn text_content_recursive(&self, id: NodeId) -> String {
        self.dom.text_content(id)
    }

    /// 获取属性值
    pub fn attribute(&self, id: NodeId, name: &str) -> Option<String> {
        let node = self.get_node(id)?;
        node.get_attribute(name).map(|s| s.to_string())
    }

    /// 遍历所有元素节点
    pub fn traverse_elements(&self) -> Vec<(NodeId, String)> {
        let mut elements = Vec::new();
        self.traverse_elements_recursive(self.document(), &mut elements);
        elements
    }

    fn traverse_elements_recursive(&self, current: NodeId, elements: &mut Vec<(NodeId, String)>) {
        if let Some(tag) = self.tag_name(current) {
            elements.push((current, tag));
        }
        
        for child in self.children(current) {
            self.traverse_elements_recursive(child, elements);
        }
    }

    /// 获取页面标题（<title> 标签）
    pub fn title(&self) -> Option<String> {
        for (id, tag) in self.traverse_elements() {
            if tag.to_lowercase() == "title" {
                let title_text = self.text_content_recursive(id);
                if !title_text.is_empty() {
                    return Some(title_text.trim().to_string());
                }
            }
        }
        None
    }

    /// 获取 body 元素
    pub fn body(&self) -> NodeId {
        self.dom.find_body_or_root()
    }
}

impl Clone for DomWrapper {
    fn clone(&self) -> Self {
        Self::from_html("<!-- cloned -->", self.url.as_ref().map(|u| u.as_str()))
    }
}
