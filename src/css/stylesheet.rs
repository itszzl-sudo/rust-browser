//! 样式表 - CSS 规则和计算样式
//!
//! 管理 CSS 规则和应用样式

use crate::dom::node::{DomNode, ElementData};
use log::{debug, trace};
use std::collections::HashMap;

#[cfg(feature = "taffy")]
use taffy::Taffy;

/// CSS 选择器
#[derive(Debug, Clone, PartialEq)]
pub struct Selector {
    /// 选择器类型
    pub kind: SelectorKind,
    /// 特异性
    pub(crate) specificity: (usize, usize, usize),
}

/// 选择器类型
#[derive(Debug, Clone, PartialEq)]
pub enum SelectorKind {
    /// 通配符选择器
    Universal,
    /// 标签选择器
    Tag(String),
    /// ID 选择器
    Id(String),
    /// 类选择器
    Class(String),
    /// 属性选择器
    Attribute { name: String, value: Option<String> },
    /// 组合选择器
    Descendant(Box<Selector>, Box<Selector>),
    /// 子选择器
    Child(Box<Selector>, Box<Selector>),
    /// 兄弟选择器
    Sibling(Box<Selector>, Box<Selector>),
}

impl Selector {
    /// 创建标签选择器
    pub fn tag(name: &str) -> Self {
        Self {
            kind: SelectorKind::Tag(name.to_lowercase()),
            specificity: (0, 0, 1),
        }
    }

    /// 创建类选择器
    pub fn class(name: &str) -> Self {
        Self {
            kind: SelectorKind::Class(name.to_string()),
            specificity: (0, 1, 0),
        }
    }

    /// 创建 ID 选择器
    pub fn id(name: &str) -> Self {
        Self {
            kind: SelectorKind::Id(name.to_string()),
            specificity: (1, 0, 0),
        }
    }

    /// 创建通配符选择器
    pub fn universal() -> Self {
        Self {
            kind: SelectorKind::Universal,
            specificity: (0, 0, 0),
        }
    }

    /// 匹配节点
    pub fn matches(&self, node: &DomNode) -> bool {
        match (&self.kind, &node.node_type) {
            (SelectorKind::Universal, _) => true,
            (SelectorKind::Tag(tag), crate::dom::node::NodeType::Element(data)) => {
                data.tag_name.to_lowercase() == *tag
            }
            (SelectorKind::Id(id), crate::dom::node::NodeType::Element(data)) => {
                data.get_attribute("id").map(|a| a == id).unwrap_or(false)
            }
            (SelectorKind::Class(class), crate::dom::node::NodeType::Element(data)) => {
                data.has_class(class)
            }
            _ => false,
        }
    }
}

impl PartialOrd for Selector {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.specificity.cmp(&other.specificity))
    }
}

/// CSS 规则
#[derive(Debug, Clone)]
pub struct Rule {
    /// 选择器
    pub selector: Selector,
    /// 属性声明
    pub declarations: Vec<(String, Property)>,
}

/// 属性值
#[derive(Debug, Clone)]
pub enum Property {
    /// 颜色
    Color(super::values::Color),
    /// 长度值
    Length(super::values::Length),
    /// 关键字
    Keyword(String),
    /// 多个值
    Multiple(Vec<Property>),
}

impl Property {
    /// 转换为 taffy 样式
    #[cfg(feature = "taffy")]
    pub fn to_taffy_style(&self) -> taffy::style::Style {
        use taffy::style::*;
        use taffy::geometry::*;
        use taffy::prelude::*;

        let mut style = Style::default();

        match self {
            Property::Color(color) => {
                let rgba = color.to_rgba();
                style.background_color = Some(Color {
                    r: rgba[0] as f32 / 255.0,
                    g: rgba[1] as f32 / 255.0,
                    b: rgba[2] as f32 / 255.0,
                    a: rgba[3] as f32 / 255.0,
                });
            }
            Property::Length(len) => {
                use super::values::LengthUnit;
                match len.unit {
                    LengthUnit::Px => {
                        style.size = Size {
                            width: Dimension::Points(len.value as f32),
                            height: Dimension::Points(len.value as f32),
                        };
                    }
                    LengthUnit::Percent => {
                        style.size = Size {
                            width: Dimension::Percent(len.value as f32 / 100.0),
                            height: Dimension::Percent(len.value as f32 / 100.0),
                        };
                    }
                    _ => {}
                }
            }
            Property::Keyword(keyword) => {
                match keyword.as_str() {
                    "flex" => {
                        style.display = Display::Flex;
                    }
                    "block" => {
                        style.display = Display::Block;
                    }
                    "none" => {
                        style.display = Display::None;
                    }
                    "row" => {
                        style.flex_direction = FlexDirection::Row;
                    }
                    "column" => {
                        style.flex_direction = FlexDirection::Column;
                    }
                    "wrap" => {
                        style.flex_wrap = FlexWrap::Wrap;
                    }
                    _ => {}
                }
            }
            Property::Multiple(_) => {}
        }

        style
    }
}

/// 已匹配的规则
#[derive(Debug, Clone)]
pub struct MatchedRule {
    /// 规则
    pub rule: Rule,
    /// 匹配的选择器
    pub selector: Selector,
}

/// 计算后的样式
#[derive(Debug, Clone, Default)]
pub struct ComputedStyle {
    /// 属性映射
    pub properties: HashMap<String, Property>,
    /// 布局节点 ID
    pub layout_node: Option<usize>,
}

impl ComputedStyle {
    /// 创建新的计算样式
    pub fn new() -> Self {
        Self {
            properties: HashMap::new(),
            layout_node: None,
        }
    }

    /// 获取属性
    pub fn get(&self, name: &str) -> Option<&Property> {
        self.properties.get(name)
    }

    /// 设置属性
    pub fn set(&mut self, name: &str, value: Property) {
        self.properties.insert(name.to_string(), value);
    }
}

/// 样式表
#[derive(Debug, Clone, Default)]
pub struct Stylesheet {
    /// CSS 规则列表
    rules: Vec<Rule>,
}

impl Stylesheet {
    /// 解析 CSS
    pub fn parse(css: &str) -> Result<Self, String> {
        let mut parser = super::parser::CssParser::new(css);
        parser.parse()
    }

    /// 获取所有规则
    pub fn rules(&self) -> &[Rule] {
        &self.rules
    }

    /// 添加规则
    pub fn add_rule(&mut self, rule: Rule) {
        self.rules.push(rule);
    }

    /// 匹配规则到节点
    pub fn match_rules(&self, node: &DomNode) -> Vec<MatchedRule> {
        let mut matched = Vec::new();

        for rule in &self.rules {
            if rule.selector.matches(node) {
                matched.push(MatchedRule {
                    rule: rule.clone(),
                    selector: rule.selector.clone(),
                });
            }
        }

        // 按特异性排序
        matched.sort_by(|a, b| a.selector.partial_cmp(&b.selector).unwrap());
        matched
    }

    /// 计算节点样式
    pub fn compute_style(&self, node: &DomNode) -> ComputedStyle {
        let mut style = ComputedStyle::new();
        let matched = self.match_rules(node);

        debug!("节点 {:?} 匹配 {} 条规则", node, matched.len());

        for matched_rule in matched {
            for (name, value) in &matched_rule.rule.declarations {
                trace!("  设置属性: {} = {:?}", name, value);
                style.set(name, value.clone());
            }
        }

        style
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_selector_matching() {
        let selector = Selector::class("test");
        let node = DomNode::element("div");
        assert!(!selector.matches(&node));
    }

    #[test]
    fn test_stylesheet_parse() {
        let css = "div { color: red; }";
        let stylesheet = Stylesheet::parse(css);
        assert!(stylesheet.is_ok());
    }
}
