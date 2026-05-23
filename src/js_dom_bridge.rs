//! JS DOM Bridge — JS 引擎与真实 DOM 树之间的双向桥接层
//!
//! 将 JS 的 DOM API 调用（getElementById, querySelector, innerHTML,
//! appendChild, style 修改等）映射到 kuchiki 真实 DOM 树的操作。
//!
//! # 流程
//!
//! 1. JS 调用 DOM API → 经过 Boa/V8 原生函数绑定
//! 2. JsDomBridge 接收调用，操作 kuchiki NodeRef
//! 3. 标记受影响的节点为 "脏"
//! 4. 下一帧渲染循环读取脏节点，增量重排重绘

use crate::dom_wrapper::DomWrapper;
use html5ever::ns;
use html5ever::QualName;
use kuchiki::traits::TendrilSink;
use kuchiki::{Attribute, ExpandedName, NodeRef};
use std::collections::{HashMap, HashSet};

/// DOM 变异类型 —— 记录 DOM 树发生了哪种变更
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DomMutationType {
    /// 节点属性变更（如 style, class, id）
    Attribute { node_id: usize, attr_name: String },
    /// 子节点添加
    ChildAdded { parent_id: usize, child_id: usize },
    /// 子节点移除
    ChildRemoved { parent_id: usize, child_id: usize },
    /// 文本内容变更
    TextChanged { node_id: usize },
    /// DOM 树整体替换（如 innerHTML）
    SubtreeReplaced { parent_id: usize },
}

/// 节点引用 —— 在 JsDomBridge 中使用的节点标识
/// 可以是已索引的节点 ID（usize），也可以是尚未索引的 NodeRef
#[derive(Debug, Clone)]
pub enum JsNodeRef {
    /// 已存在于 DomWrapper 索引中的节点 ID
    Indexed(usize),
    /// 尚未索引的原始 kuchiki 节点
    Raw(NodeRef),
}

impl JsNodeRef {
    /// 获取节点 ID（如果已索引）
    pub fn id(&self) -> Option<usize> {
        match self {
            JsNodeRef::Indexed(id) => Some(*id),
            JsNodeRef::Raw(_) => None,
        }
    }

    /// 获取 NodeRef
    pub fn node_ref(&self) -> &NodeRef {
        match self {
            JsNodeRef::Indexed(id) => panic!("JsNodeRef::Indexed({}) cannot be converted to &NodeRef directly — use resolve_node() on bridge", id),
            JsNodeRef::Raw(n) => n,
        }
    }
}

/// DOM 桥接器 —— 管理 JS 与真实 DOM 的交互
pub struct JsDomBridge {
    /// 当前 DOM
    dom: DomWrapper,
    /// 自上次渲染以来的 DOM 变异队列
    mutations: Vec<DomMutationType>,
    /// 所有被修改的节点 ID 集合（脏节点）
    dirty_nodes: HashSet<usize>,
    /// DOM 版本号（每次树结构变化递增）
    dom_version: usize,
    /// JS 引擎是否已通知 DOM 变更
    pub has_pending_changes: bool,
    /// JS 端创建的临时节点缓存，key 是 JS 分配的临时 ID
    /// 用于追踪通过 createElement / createTextNode 创建但尚未挂载的节点
    pending_nodes: HashMap<usize, NodeRef>,
    /// 下一个临时节点 ID
    next_pending_id: usize,
}

impl JsDomBridge {
    pub fn new() -> Self {
        let dom = DomWrapper::from_html(
            "<!DOCTYPE html><html><body></body></html>",
            Some("about:blank"),
        );
        Self {
            dom,
            mutations: Vec::new(),
            dirty_nodes: HashSet::new(),
            dom_version: 0,
            has_pending_changes: false,
            pending_nodes: HashMap::new(),
            next_pending_id: 1,
        }
    }

    /// 获取当前 DOM 引用
    pub fn dom(&self) -> &DomWrapper {
        &self.dom
    }

    /// 获取可变 DOM 引用
    pub fn dom_mut(&mut self) -> &mut DomWrapper {
        &mut self.dom
    }

    /// 获取当前 DOM 版本号
    pub fn dom_version(&self) -> usize {
        self.dom_version
    }

    /// 检查是否有待处理的 DOM 变更
    pub fn has_pending(&self) -> bool {
        self.has_pending_changes
    }

    /// 清空变异记录（渲染完成后调用）
    pub fn clear_mutations(&mut self) {
        self.mutations.clear();
        self.dirty_nodes.clear();
        self.has_pending_changes = false;
    }

    /// 获取并清空脏节点列表
    pub fn drain_dirty_nodes(&mut self) -> Vec<usize> {
        let nodes: Vec<usize> = self.dirty_nodes.drain().collect();
        nodes
    }

    /// 获取变异记录
    pub fn mutations(&self) -> &[DomMutationType] {
        &self.mutations
    }

    /// 记录 DOM 变异
    fn record_mutation(&mut self, mutation: DomMutationType) {
        self.mutations.push(mutation);
        self.dom_version += 1;
        self.has_pending_changes = true;
    }

    /// 将 NodeRef 解析为索引 ID，必要时添加到 DOM 的索引中
    #[allow(dead_code)]
    fn resolve_node(&mut self, node: &NodeRef) -> Option<usize> {
        // 先尝试通过已有的索引查找
        if let Some(id) = self.dom.index_of_node(node) {
            return Some(id);
        }
        // 如果不在索引中，重建索引
        self.dom.rebuild_node_index();
        self.dom.index_of_node(node)
    }

    // ============================================================
    // JS 端节点创建（createElement / createTextNode）
    // ============================================================

    /// 创建一个新的元素节点 — 对应 JS document.createElement(tag)
    /// 返回一个临时 ID，JS 端可以用这个 ID 引用该节点
    pub fn create_element(&mut self, tag: &str) -> usize {
        let name = QualName::new(None, ns!(html), tag.to_string().into());
        let attrs: Vec<(ExpandedName, Attribute)> = Vec::new();
        let node = NodeRef::new_element(name, attrs);
        let id = self.next_pending_id;
        self.next_pending_id += 1;
        self.pending_nodes.insert(id, node);
        id
    }

    /// 创建一个新的文本节点 — 对应 JS document.createTextNode(text)
    pub fn create_text_node(&mut self, text: &str) -> usize {
        let node = NodeRef::new_text(text.to_string());
        let id = self.next_pending_id;
        self.next_pending_id += 1;
        self.pending_nodes.insert(id, node);
        id
    }

    /// 通过临时 ID 获取 pending 节点
    #[allow(dead_code)]
    fn take_pending_node(&mut self, pending_id: usize) -> Option<NodeRef> {
        self.pending_nodes.remove(&pending_id)
    }

    /// 检查 pending_id 是否指向一个尚未挂载的元素节点
    #[allow(dead_code)]
    fn is_pending_node(&self, id: usize) -> bool {
        self.pending_nodes.contains_key(&id)
    }

    // ============================================================
    // DOM 查询方法（供 JS 原生函数绑定调用）
    // ============================================================

    /// 通过 ID 查询元素 — 对应 JS document.getElementById(id)
    pub fn get_element_by_id(&self, id: &str) -> Option<usize> {
        self.dom.select_first(&format!("#{}", id))
    }

    /// 通过 CSS 选择器查询 — 对应 JS document.querySelector(selector)
    pub fn query_selector(&self, selector: &str) -> Option<usize> {
        self.dom.select_first(selector)
    }

    /// 通过 CSS 选择器查询全部 — 对应 JS document.querySelectorAll(selector)
    pub fn query_selector_all(&self, selector: &str) -> Vec<usize> {
        self.dom.select(selector)
    }

    /// 获取标签名
    pub fn tag_name(&self, node_id: usize) -> Option<String> {
        self.dom.tag_name(node_id)
    }

    /// 获取属性
    pub fn get_attribute(&self, node_id: usize, name: &str) -> Option<String> {
        self.dom.attribute(node_id, name)
    }

    /// 设置属性（记录变异）
    pub fn set_attribute(&mut self, node_id: usize, name: &str, value: &str) {
        self.dom.set_attribute(node_id, name, value);
        self.dirty_nodes.insert(node_id);
        self.record_mutation(DomMutationType::Attribute {
            node_id,
            attr_name: name.to_string(),
        });
    }

    /// 移除属性
    pub fn remove_attribute(&mut self, node_id: usize, name: &str) {
        self.dom.set_attribute(node_id, name, "");
        self.dirty_nodes.insert(node_id);
        self.record_mutation(DomMutationType::Attribute {
            node_id,
            attr_name: name.to_string(),
        });
    }

    /// 获取文本内容 — 对应 JS element.textContent
    pub fn text_content(&self, node_id: usize) -> Option<String> {
        self.dom.text_content(node_id)
    }

    /// 设置文本内容 — 对应 JS element.textContent = "..."
    pub fn set_text_content(&mut self, node_id: usize, text: &str) {
        if let Some(node_ref) = self.dom.get_node(node_id) {
            // 移除现有子节点
            let children: Vec<_> = node_ref.children().collect();
            for child in children {
                child.detach();
            }
            // 添加文本节点
            if !text.is_empty() {
                let text_node = NodeRef::new_text(text.to_string());
                node_ref.append(text_node);
            }
            // 重建索引（因为移除了子节点）
            self.dom.rebuild_node_index();
            self.dirty_nodes.insert(node_id);
            self.record_mutation(DomMutationType::TextChanged { node_id });
        }
    }

    /// 设置 innerHTML — 对应 JS element.innerHTML = "..."
    ///
    /// 解析 HTML 片段，替换目标元素的所有子节点
    pub fn set_inner_html(&mut self, node_id: usize, html: &str) {
        if let Some(node_ref) = self.dom.get_node(node_id) {
            // 移除现有子节点
            let children: Vec<_> = node_ref.children().collect();
            for child in children {
                child.detach();
            }

            // 解析 HTML 片段
            if !html.trim().is_empty() {
                let fragment = kuchiki::parse_html().one(html);
                // 将 fragment 的子节点移到目标节点
                let fragment_children: Vec<_> = fragment.children().collect();
                for child in fragment_children {
                    node_ref.append(child);
                }
            }

            // 重建索引（因为新增了子节点）
            self.dom.rebuild_node_index();
            self.dirty_nodes.insert(node_id);
            self.record_mutation(DomMutationType::SubtreeReplaced { parent_id: node_id });
        }
    }

    /// 获取 innerHTML — 对应 JS element.innerHTML
    ///
    /// 递归序列化所有子节点为 HTML 字符串，包含元素、文本和注释节点。
    pub fn get_inner_html(&self, node_id: usize) -> Option<String> {
        if let Some(node_ref) = self.dom.get_node(node_id) {
            let mut html = String::new();
            for child in node_ref.children() {
                html.push_str(&serialize_node(&child));
            }
            Some(html)
        } else {
            None
        }
    }

    /// 获取 outerHTML — 对应 JS element.outerHTML
    ///
    /// 返回元素自身的标签和属性，加上内部内容的完整 HTML。
    /// 仅对元素节点有效；文本节点会返回其文本内容。
    pub fn get_outer_html(&self, node_id: usize) -> Option<String> {
        if let Some(node_ref) = self.dom.get_node(node_id) {
            Some(serialize_node(&node_ref))
        } else {
            None
        }
    }

    /// 追加子节点 — 对应 JS element.appendChild(child)
    ///
    /// child_id 可以是 DomWrapper 中的已索引节点 ID，也可以是 createElement 返回的临时 ID
    pub fn append_child(&mut self, parent_id: usize, child_id: usize) -> bool {
        // 尝试从 pending 节点获取
        if let Some(child_node) = self.pending_nodes.remove(&child_id) {
            // child 是尚未挂载的临时节点
            if let Some(parent_ref) = self.dom.get_node(parent_id) {
                let _ = parent_ref.append(child_node);
                // 重建索引以包含新节点
                self.dom.rebuild_node_index();
                self.dirty_nodes.insert(parent_id);
                self.record_mutation(DomMutationType::ChildAdded {
                    parent_id,
                    child_id,
                });
                return true;
            }
        } else if let Some(child_node) = self.dom.get_node(child_id) {
            // child 是已索引的节点
            if let Some(parent_ref) = self.dom.get_node(parent_id) {
                parent_ref.append(child_node);
                self.dom.rebuild_node_index();
                self.dirty_nodes.insert(parent_id);
                self.record_mutation(DomMutationType::ChildAdded {
                    parent_id,
                    child_id,
                });
                return true;
            }
        }
        false
    }

    /// 移除子节点 — 对应 JS element.removeChild(child)
    pub fn remove_child(&mut self, parent_id: usize, child_id: usize) -> bool {
        if self.dom.get_node(parent_id).is_some() {
            if let Some(child_node) = self.dom.get_node(child_id) {
                child_node.detach();
                self.dom.rebuild_node_index();
                self.dirty_nodes.insert(parent_id);
                self.record_mutation(DomMutationType::ChildRemoved {
                    parent_id,
                    child_id,
                });
                return true;
            }
        }
        false
    }

    /// 替换子节点 — 对应 JS element.replaceChild(newChild, oldChild)
    pub fn replace_child(
        &mut self,
        parent_id: usize,
        new_child_id: usize,
        old_child_id: usize,
    ) -> bool {
        if let Some(parent_ref) = self.dom.get_node(parent_id) {
            if let Some(old_child) = self.dom.get_node(old_child_id) {
                // 尝试从 pending 节点获取新子节点
                let new_child = if let Some(node) = self.pending_nodes.remove(&new_child_id) {
                    node
                } else if let Some(node) = self.dom.get_node(new_child_id) {
                    node
                } else {
                    return false;
                };

                // 在 old_child 前面插入 new_child（取代 old_child）
                match old_child.previous_sibling() {
                    Some(sibling_ref) => {
                        sibling_ref.insert_after(new_child.clone());
                    }
                    None => {
                        parent_ref.prepend(new_child.clone());
                    }
                }
                old_child.detach();
                self.dom.rebuild_node_index();
                self.dirty_nodes.insert(parent_id);
                self.record_mutation(DomMutationType::ChildRemoved {
                    parent_id,
                    child_id: old_child_id,
                });
                self.record_mutation(DomMutationType::ChildAdded {
                    parent_id,
                    child_id: new_child_id,
                });
                return true;
            }
        }
        false
    }

    /// 获取父节点
    pub fn parent_node(&self, node_id: usize) -> Option<usize> {
        self.dom.parent(node_id)
    }

    /// 获取子节点列表
    pub fn child_nodes(&self, node_id: usize) -> Vec<usize> {
        self.dom.children(node_id)
    }

    /// 获取第一个子元素 — 对应 JS element.firstElementChild
    pub fn first_child(&self, node_id: usize) -> Option<usize> {
        let children = self.dom.children(node_id);
        // 跳过文本节点，返回第一个元素节点
        for &child in &children {
            if self.dom.tag_name(child).is_some() {
                return Some(child);
            }
        }
        children.first().copied()
    }

    /// 获取最后一个子元素 — 对应 JS element.lastElementChild
    pub fn last_child(&self, node_id: usize) -> Option<usize> {
        let children = self.dom.children(node_id);
        // 从后往前找元素节点
        for child in children.iter().rev() {
            if self.dom.tag_name(*child).is_some() {
                return Some(*child);
            }
        }
        children.last().copied()
    }

    /// 获取下一个兄弟元素 — 对应 JS element.nextElementSibling
    pub fn next_sibling(&self, node_id: usize) -> Option<usize> {
        let parent = self.dom.parent(node_id)?;
        let siblings = self.dom.children(parent);
        let pos = siblings.iter().position(|&id| id == node_id)?;
        // 查找下一个元素节点
        for sibling in &siblings[pos + 1..] {
            if self.dom.tag_name(*sibling).is_some() {
                return Some(*sibling);
            }
        }
        None
    }

    /// 获取上一个兄弟元素 — 对应 JS element.previousElementSibling
    pub fn previous_sibling(&self, node_id: usize) -> Option<usize> {
        let parent = self.dom.parent(node_id)?;
        let siblings = self.dom.children(parent);
        let pos = siblings.iter().position(|&id| id == node_id)?;
        // 从后往前找元素节点
        for sibling in siblings[..pos].iter().rev() {
            if self.dom.tag_name(*sibling).is_some() {
                return Some(*sibling);
            }
        }
        None
    }

    /// 获取节点类名 — 对应 JS element.className
    pub fn class_name(&self, node_id: usize) -> String {
        self.dom.attribute(node_id, "class").unwrap_or_default()
    }

    /// 设置节点类名
    pub fn set_class_name(&mut self, node_id: usize, class_name: &str) {
        self.set_attribute(node_id, "class", class_name);
    }

    /// 获取节点 ID
    pub fn id(&self, node_id: usize) -> String {
        self.dom.attribute(node_id, "id").unwrap_or_default()
    }

    /// 获取内联样式字符串
    pub fn get_style_attr(&self, node_id: usize) -> String {
        self.dom.attribute(node_id, "style").unwrap_or_default()
    }

    /// 设置内联样式字符串
    pub fn set_style_attr(&mut self, node_id: usize, css_text: &str) {
        self.set_attribute(node_id, "style", css_text);
    }

    /// 获取节点列表长度 — 对应 JS NodeList.length
    pub fn node_list_length(&self, node_ids: &[usize]) -> usize {
        node_ids.len()
    }

    /// 通过索引获取 NodeList 中的节点 — 对应 JS NodeList.item(index)
    pub fn node_list_item(node_ids: &[usize], index: usize) -> Option<usize> {
        node_ids.get(index).copied()
    }

    /// 用 HTML 字符串替换整个 DOM — 对应 JS document.write / document.open + write
    pub fn set_document_html(&mut self, html: &str) {
        self.dom = DomWrapper::from_html(html, Some("about:blank"));
        self.mutations.clear();
        self.dirty_nodes.clear();
        self.dom_version += 1;
        self.has_pending_changes = true;
    }

    /// 检查元素是否匹配指定 CSS 选择器 — 对应 JS element.matches(selector)
    pub fn matches(&self, node_id: usize, selector: &str) -> bool {
        // 获取标签名和属性，做简单匹配
        if let Some(tag) = self.dom.tag_name(node_id) {
            let selector_clean = selector.trim();
            // ID 选择器
            if let Some(id_str) = selector_clean.strip_prefix('#') {
                return self
                    .dom
                    .attribute(node_id, "id")
                    .map_or(false, |id| id == id_str);
            }
            // 类选择器
            if let Some(class_str) = selector_clean.strip_prefix('.') {
                return self
                    .dom
                    .attribute(node_id, "class")
                    .map_or(false, |cls| cls.split_whitespace().any(|c| c == class_str));
            }
            // 标签选择器
            return tag.eq_ignore_ascii_case(selector_clean);
        }
        false
    }

    /// 获取元素的所有属性名列表 — 对应 JS element.getAttributeNames()
    pub fn get_attribute_names(&self, node_id: usize) -> Vec<String> {
        if let Some(node) = self.dom.get_node(node_id) {
            if let Some(element) = node.as_element() {
                let attrs = element.attributes.borrow();
                return attrs
                    .map
                    .iter()
                    .filter_map(|(name, attr)| {
                        if !attr.value.is_empty() {
                            Some(name.local.to_string())
                        } else {
                            None
                        }
                    })
                    .collect();
            }
        }
        Vec::new()
    }

    /// 判断节点是否有子节点 — 对应 JS element.hasChildNodes()
    pub fn has_child_nodes(&self, node_id: usize) -> bool {
        self.dom.has_children(node_id)
    }

    /// 获取子节点数量 — 对应 JS element.childElementCount
    pub fn child_element_count(&self, node_id: usize) -> usize {
        self.dom
            .children(node_id)
            .iter()
            .filter(|&&id| self.dom.tag_name(id).is_some())
            .count()
    }

    /// 将节点插入到指定子节点之前 — 对应 JS element.insertBefore(newNode, referenceNode)
    pub fn insert_before(
        &mut self,
        parent_id: usize,
        new_child_id: usize,
        reference_id: usize,
    ) -> bool {
        if let Some(parent_ref) = self.dom.get_node(parent_id) {
            if let Some(ref_node) = self.dom.get_node(reference_id) {
                let new_child = if let Some(node) = self.pending_nodes.remove(&new_child_id) {
                    node
                } else if let Some(node) = self.dom.get_node(new_child_id) {
                    node
                } else {
                    return false;
                };

                match ref_node.previous_sibling() {
                    Some(prev_ref) => {
                        prev_ref.insert_after(new_child);
                    }
                    None => {
                        parent_ref.prepend(new_child);
                    }
                }
                self.dom.rebuild_node_index();
                self.dirty_nodes.insert(parent_id);
                self.record_mutation(DomMutationType::ChildAdded {
                    parent_id,
                    child_id: new_child_id,
                });
                return true;
            }
        }
        false
    }
}

/// 递归序列化节点为 HTML 字符串
///
/// 支持元素节点（含属性）、文本节点和注释节点的完整序列化。
/// 元素及其所有后代都会被递归序列化。
fn serialize_node(node: &kuchiki::NodeRef) -> String {
    if let Some(text) = node.as_text() {
        return text.borrow().to_string();
    }
    if let Some(comment) = node.as_comment() {
        return format!("<!--{}-->", comment.borrow());
    }
    if let Some(el) = node.as_element() {
        let tag = el.name.local.as_ref();
        let mut html = format!("<{}", tag);

        // 序列化属性
        let attrs = el.attributes.borrow();
        for (name, value) in attrs.map.iter() {
            // kuchikikiki 0.12.0 的 Attribute 类型有 value 字段
            let val_str: &str = &value.value;
            html.push_str(&format!(" {}=\"{}\"", name.local, val_str));
        }
        html.push('>');

        // 递归子节点
        for child in node.children() {
            html.push_str(&serialize_node(&child));
        }

        html.push_str(&format!("</{}>", tag));
        return html;
    }
    String::new()
}

impl Default for JsDomBridge {
    fn default() -> Self {
        Self::new()
    }
}
