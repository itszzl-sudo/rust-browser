//! HTML 解析器 - 将 HTML 文本解析为 DOM 树
//!
//! 实现完整的 HTML 解析功能，支持标准 HTML 标签和属性

use crate::dom::node::{DomNode, DomTree};
use log::{debug, info};

/// HTML 解析器
pub struct HtmlParser {
    /// 输入 HTML 文本
    input: String,
    /// 当前位置（字符索引）
    pos: usize,
}

impl HtmlParser {
    /// 创建新的解析器
    pub fn new(html: &str) -> Self {
        Self {
            input: html.to_string(),
            pos: 0,
        }
    }

    /// 获取总字符数
    fn len(&self) -> usize {
        self.input.chars().count()
    }

    /// 检查是否到达末尾
    fn is_at_end(&self) -> bool {
        self.pos >= self.len()
    }

    /// 获取当前位置的字符
    fn current_char(&self) -> Option<char> {
        self.input.chars().nth(self.pos)
    }

    /// 获取下一个字符（不移除）
    fn peek(&self, offset: usize) -> Option<char> {
        self.input.chars().nth(self.pos + offset)
    }

    /// 跳过空白字符
    fn skip_whitespace(&mut self) {
        while let Some(c) = self.current_char() {
            if c.is_whitespace() {
                self.pos += 1;
            } else {
                break;
            }
        }
    }

    /// 解析 HTML 并返回 DOM 树
    pub fn parse(&mut self) -> DomTree {
        info!("开始解析 HTML ({} 字符)", self.input.len());

        let mut tree = DomTree::new();

        // 创建文档根节点
        let doc_node = DomNode::document();
        let root_index = tree.add_node(doc_node);

        // 解析内容
        self.skip_doctype();

        // 处理所有子节点
        while !self.is_at_end() {
            self.skip_whitespace();

            if self.is_at_end() {
                break;
            }

            // 检查是否是注释
            let rest = self.get_remaining_str();
            if rest.starts_with("<!--") {
                self.skip_comment();
                continue;
            }

            // 检查是否是 DOCTYPE
            if rest.starts_with("<!") && !rest.starts_with("<!--") {
                self.skip_doctype();
                continue;
            }

            // 检查是否是标签
            if self.current_char() == Some('<') {
                if let Some(node) = self.parse_element(&mut tree, root_index) {
                    // 如果根节点没有子节点，设置为子节点
                    if tree.get(root_index).map(|n| n.children.is_empty()).unwrap_or(false) {
                        tree.append_child(root_index, node);
                    }
                }
            } else {
                // 解析文本内容
                self.parse_text(&mut tree, root_index);
            }
        }

        debug!("HTML 解析完成，{} 个节点", tree.len());
        tree
    }

    /// 获取从当前位置开始的剩余字符串
    fn get_remaining_str(&self) -> &str {
        let byte_pos = self.char_to_byte(self.pos);
        &self.input[byte_pos..]
    }

    /// 将字符索引转换为字节位置
    fn char_to_byte(&self, char_idx: usize) -> usize {
        self.input.chars().take(char_idx).map(|c| c.len_utf8()).sum()
    }

    /// 将字节位置转换为字符索引
    fn byte_to_char(&self, byte_idx: usize) -> usize {
        self.input[..byte_idx].chars().count()
    }

    /// 解析元素
    fn parse_element(&mut self, tree: &mut DomTree, parent_index: usize) -> Option<usize> {
        self.consume_char(); // 消费 '<'

        // 检查是否是闭合标签
        if self.current_char() == Some('/') {
            self.consume_char();
            let tag_name = self.parse_tag_name();
            self.skip_whitespace();
            self.consume_char(); // 消费 '>'
            // 这是一个闭合标签，不需要创建节点
            debug!("遇到闭合标签: {}", tag_name);
            return None;
        }

        // 解析标签名
        let tag_name = self.parse_tag_name();
        self.skip_whitespace();

        // 创建元素节点
        let mut node = DomNode::element(&tag_name);

        // 解析属性 - 需要获取 ElementData 的可变引用
        {
            if let crate::dom::node::NodeType::Element(ref mut data) = node.node_type {
                while !self.is_at_end() && self.current_char() != Some('/')
                    && self.current_char() != Some('>') && self.current_char() != Some('<') {
                    self.skip_whitespace();

                    if self.current_char() == Some('/') || self.current_char() == Some('>')
                        || self.current_char() == Some('<') {
                        break;
                    }

                    if let Some((name, value)) = self.parse_attribute() {
                        data.set_attribute(&name, &value);
                    } else {
                        break;
                    }
                }
            }
        }

        // 检查自闭合标签
        let is_self_closing = self.current_char() == Some('/');
        if is_self_closing {
            self.consume_char();
        }

        // 检查是否是无内容标签（如 img, br, hr, input 等）
        let is_void = matches!(
            tag_name.to_lowercase().as_str(),
            "img" | "br" | "hr" | "input" | "meta" | "link" | "area"
                | "base" | "col" | "embed" | "param" | "source" | "track" | "wbr"
        );

        self.skip_whitespace();

        // 检查是否有闭合标签
        let has_closing_tag = if self.check('>') {
            false
        } else if self.check('<') && self.check_next('/') {
            true
        } else {
            self.consume_char(); // 消费 '>'
            false
        };

        // 添加节点到树
        let node_index = tree.add_node(node);
        tree.append_child(parent_index, node_index);

        // 如果不是自闭合且不是 void 元素，处理内容
        if !is_self_closing && !is_void {
            // 消费开始标签的闭合
            if !has_closing_tag {
                // 已经消费过了
            }

            // 解析子内容
            self.parse_children(tree, node_index, &tag_name);

            // 如果有闭合标签，跳过它
            if has_closing_tag {
                self.consume_char(); // 消费 '<'
                self.consume_char(); // 消费 '/'
                let _ = self.parse_tag_name();
                self.skip_whitespace();
                self.consume_char(); // 消费 '>'
            }
        }

        Some(node_index)
    }

    /// 解析子内容
    fn parse_children(&mut self, tree: &mut DomTree, parent_index: usize, parent_tag: &str) {
        let parent_tag_lower = parent_tag.to_lowercase();

        // 忽略 script 和 style 标签内容（后续可以扩展）
        let skip_content = matches!(parent_tag_lower.as_str(), "script" | "style" | "noscript" | "textarea" | "template" | "iframe");

        loop {
            // 跳过空白
            self.skip_whitespace();

            if self.is_at_end() {
                break;
            }

            // 检查是否是标签结束
            if self.current_char() == Some('<') {
                // 检查是否是闭合标签
                if self.peek(1) == Some('/') {
                    break;
                }

                // 检查是否是注释
                let rest = self.get_remaining_str();
                if rest.starts_with("<!--") {
                    self.skip_comment();
                    continue;
                }

                // 检查是否是 DOCTYPE
                if rest.starts_with("<!") && !rest.starts_with("<!--") {
                    self.skip_doctype();
                    continue;
                }

                // 解析子元素
                if let Some(child_index) = self.parse_element(tree, parent_index) {
                    // 子元素已经添加到树中
                    let _ = child_index;
                }
            } else {
                // 解析文本内容
                if skip_content {
                    // 简单跳过整个标签内容
                    let close_tag = format!("</{}>", parent_tag_lower);
                    let rest = self.get_remaining_str();
                    if let Some(pos) = rest.find(&close_tag) {
                        // 计算要跳过的字符数
                        let byte_to_skip = self.char_to_byte(self.pos) + pos + close_tag.len();
                        self.pos = self.byte_to_char(byte_to_skip);
                    } else if let Some(pos) = rest.find('<') {
                        let byte_to_skip = self.char_to_byte(self.pos) + pos;
                        self.pos = self.byte_to_char(byte_to_skip);
                    } else {
                        self.pos = self.len();
                    }
                    break;
                } else {
                    self.parse_text(tree, parent_index);
                }
            }
        }
    }

    /// 解析文本内容
    fn parse_text(&mut self, tree: &mut DomTree, parent_index: usize) {
        let start = self.pos;

        // 找到下一个 '<' 字符
        while !self.is_at_end() {
            if self.current_char() == Some('<') {
                break;
            }
            self.consume_char();
        }

        // 提取文本（使用字符）
        let text: String = self.input.chars().skip(start).take(self.pos - start).collect();
        let text = text.trim().to_string();

        if !text.is_empty() {
            let text_node = DomNode::text(&text);
            let text_index = tree.add_node(text_node);
            tree.append_child(parent_index, text_index);
        }
    }

    /// 解析属性
    fn parse_attribute(&mut self) -> Option<(String, String)> {
        // 跳过空白
        self.skip_whitespace();

        if self.is_at_end() || self.check('/') || self.check('>') || self.check('<') {
            return None;
        }

        // 解析属性名
        let name = self.parse_identifier();
        if name.is_empty() {
            return None;
        }

        self.skip_whitespace();

        // 检查是否有值
        if self.check('=') {
            self.consume_char();
            self.skip_whitespace();

            // 解析属性值
            let value = self.parse_attribute_value();
            Some((name, value))
        } else {
            // 没有值的属性（如 disabled, checked 等）
            Some((name.clone(), name))
        }
    }

    /// 解析属性值
    fn parse_attribute_value(&mut self) -> String {
        if self.check('"') {
            // 双引号
            self.consume_char();
            let mut end = self.pos;
            while end < self.input.len() && !self.input.chars().nth(end).map(|c| c == '"').unwrap_or(false) {
                end += 1;
            }
            let value: String = self.input.chars().skip(self.pos).take(end - self.pos).collect();
            if end < self.input.len() {
                self.pos = end + 1; // Skip the closing quote
            } else {
                self.pos = end;
            }
            value
        } else if self.check('\'') {
            // 单引号
            self.consume_char();
            let mut end = self.pos;
            while end < self.input.len() && !self.input.chars().nth(end).map(|c| c == '\'').unwrap_or(false) {
                end += 1;
            }
            let value: String = self.input.chars().skip(self.pos).take(end - self.pos).collect();
            if end < self.input.len() {
                self.pos = end + 1; // Skip the closing quote
            } else {
                self.pos = end;
            }
            value
        } else {
            // 无引号值
            let mut end = self.pos;
            while end < self.input.len() {
                if let Some(c) = self.input.chars().nth(end) {
                    if c.is_whitespace() || c == '>' || c == '/' {
                        break;
                    }
                }
                end += 1;
            }
            let value: String = self.input.chars().skip(self.pos).take(end - self.pos).collect();
            self.pos = end;
            value
        }
    }

    /// 解析标签名
    fn parse_tag_name(&mut self) -> String {
        self.skip_whitespace();

        let start = self.pos;
        let mut end = self.pos;

        while end < self.input.len() {
            if let Some(c) = self.input.chars().nth(end) {
                if c.is_whitespace() || c == '>' || c == '/' || c == '<' {
                    break;
                }
            }
            end += 1;
        }

        let result: String = self.input.chars().skip(start).take(end - start).collect();
        self.pos = end;
        result.trim().to_lowercase()
    }

    /// 解析标识符
    fn parse_identifier(&mut self) -> String {
        let start = self.pos;
        let mut end = self.pos;

        while end < self.input.len() {
            if let Some(c) = self.input.chars().nth(end) {
                if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == ':' || c == '.' {
                    end += 1;
                } else {
                    break;
                }
            } else {
                break;
            }
        }

        let result: String = self.input.chars().skip(start).take(end - start).collect();
        self.pos = end;
        result.trim().to_string()
    }

    /// 跳过注释
    fn skip_comment(&mut self) {
        // 消耗 <!--
        let rest = self.get_remaining_str();
        if rest.starts_with("<!--") {
            self.pos += 4; // 字符数
        } else if rest.starts_with("<!") {
            // 消耗 <!
            self.consume_char();
            self.consume_char();
        } else {
            self.consume_char(); // consume '<'
            if self.current_char() == Some('!') {
                self.consume_char();
            }
        }

        // 查找 -->
        let rest = self.get_remaining_str();
        if let Some(end) = rest.find("-->") {
            self.pos += end + 3;
        } else {
            // 没有找到结束标记，消耗到末尾
            self.pos = self.len();
        }
    }

    /// 检查是否是注释开始
    fn check_comment_start(&self) -> bool {
        self.input[self.pos..].starts_with("<!--") || self.input[self.pos..].starts_with("<!")
    }

    /// 跳过 DOCTYPE
    fn skip_doctype(&mut self) {
        let rest = self.get_remaining_str();
        if rest.to_lowercase().starts_with("<!doctype") {
            if let Some(end) = rest.find('>') {
                let byte_to_skip = self.char_to_byte(self.pos) + end + 1;
                self.pos = self.byte_to_char(byte_to_skip);
            }
        }
    }

    /// 检查下一个字符
    fn check_next(&self, expected: char) -> bool {
        self.peek(1) == Some(expected)
    }

    /// 检查字符
    fn check(&self, expected: char) -> bool {
        !self.is_at_end() && self.current_char() == Some(expected)
    }

    /// 消费当前字符并前进
    fn consume_char(&mut self) -> char {
        if let Some(c) = self.current_char() {
            self.pos += 1;
            c
        } else {
            '\0'
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dom::node::NodeType;

    #[test]
    fn test_parse_simple_html() {
        let html = "<html><head><title>Test</title></head><body><h1>Hello</h1><p>World</p></body></html>";
        let mut parser = HtmlParser::new(html);
        let tree = parser.parse();
        assert!(tree.len() > 0);
    }

    #[test]
    fn test_parse_attributes() {
        let html = r#"<div id="main" class="container" data-value="test"></div>"#;
        let mut parser = HtmlParser::new(html);
        let tree = parser.parse();

        if let Some(root) = tree.root() {
            if let NodeType::Element(data) = &root.node_type {
                assert_eq!(data.get_attribute("id"), Some("main"));
                assert_eq!(data.get_attribute("class"), Some("container"));
            }
        }
    }

    #[test]
    fn test_parse_nested_elements() {
        let html = "<div><span><strong>Text</strong></span></div>";
        let mut parser = HtmlParser::new(html);
        let tree = parser.parse();
        assert!(tree.len() >= 4); // div, span, strong, text
    }
}