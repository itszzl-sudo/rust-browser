//! Taffy 布局引擎 - 使用 taffy 进行完整的 CSS 布局计算
//!
//! 整合 kuchiki DOM、CSS 解析和 taffy

use crate::DomWrapper;
use crate::css::values::Color;
use kuchiki::NodeRef;
use log::{debug, info};
use std::collections::HashMap;
use taffy::prelude::*;

/// 单个节点的布局信息
#[derive(Debug, Clone)]
pub struct LayoutNode {
    /// taffy 节点 id
    pub node: NodeId,
    /// 对应的 dom 节点索引
    pub dom_node: usize,
    /// 标签名 (用于调试)
    pub tag_name: String,
    /// 计算后的绝对 x 坐标
    pub x: f32,
    /// 计算后的绝对 y 坐标
    pub y: f32,
    /// 宽度
    pub width: f32,
    /// 高度
    pub height: f32,
    /// 背景颜色
    pub background: Option<Color>,
    /// 文字颜色
    pub color: Option<Color>,
}

/// 完整的 taffy 布局引擎
pub struct TaffyLayoutEngine {
    /// taffy 树管理器
    taffy: TaffyTree,
    /// dom_node -> LayoutNode 映射
    layout_nodes: HashMap<usize, LayoutNode>,
    /// 根节点
    root: Option<NodeId>,
    /// 视口尺寸
    viewport: Size<AvailableSpace>,
}

impl TaffyLayoutEngine {
    /// 创建新的布局引擎
    pub fn new(viewport_width: f32, viewport_height: f32) -> Self {
        info!("初始化 Taffy 布局引擎 ({:.0}x{:.0})", viewport_width, viewport_height);

        let viewport = Size {
            width: AvailableSpace::Definite(viewport_width),
            height: AvailableSpace::Definite(viewport_height),
        };

        Self {
            taffy: TaffyTree::new(),
            layout_nodes: HashMap::new(),
            root: None,
            viewport,
        }
    }

    /// 设置视口尺寸
    pub fn set_viewport(&mut self, width: f32, height: f32) {
        debug!("更新视口: {:.0}x{:.0}", width, height);
        self.viewport = Size {
            width: AvailableSpace::Definite(width),
            height: AvailableSpace::Definite(height),
        };
    }

    /// 计算完整的布局，从 dom 开始
    pub fn compute(&mut self, dom: &DomWrapper) -> Result<(), String> {
        debug!("开始计算完整布局");

        // 清除旧布局
        self.layout_nodes.clear();
        self.root = None;

        // 获取文档根节点
        let root = dom.document();

        // 创建根节点的 taffy 节点
        let root_style = Style {
            display: Display::Block,
            size: Size {
                width: Dimension::Percent(1.0),
                height: Dimension::Auto,
            },
            flex_direction: FlexDirection::Column,
            padding: Rect::from_length(10.0, 10.0, 10.0, 10.0),
            ..Default::default()
        };

        let root_node = self.taffy
            .new_leaf(root_style)
            .map_err(|e| format!("创建根节点失败: {}", e))?;

        self.root = Some(root_node);

        // 递归构建树
        self.build_tree_recursive(root, root_node, dom)?;

        // 计算布局
        self.compute_layout()?;

        debug!("布局计算完成！共 {} 个节点", self.layout_nodes.len());
        Ok(())
    }

    /// 递归构建树
    fn build_tree_recursive(
        &mut self,
        dom_node: usize,
        taffy_parent: NodeId,
        dom: &DomWrapper,
    ) -> Result<(), String> {
        if let Some(node_ref) = dom.get_node(dom_node) {
            if let Some(element) = node_ref.as_element() {
                let tag_name = element.name.local.to_string();
                let attrs = element.attributes.borrow();
                
                let style = self.determine_style(&tag_name, &attrs);

                let taffy_node = self.taffy
                    .new_leaf(style)
                    .map_err(|e| format!("创建元素节点 {} 失败: {}", tag_name, e))?;

                self.taffy
                    .add_child(taffy_parent, taffy_node)
                    .map_err(|e| format!("添加子节点 {} 失败: {}", tag_name, e))?;

                let layout_node = LayoutNode {
                    node: taffy_node,
                    dom_node,
                    tag_name: tag_name.clone(),
                    x: 0.0,
                    y: 0.0,
                    width: 0.0,
                    height: 0.0,
                    background: self.determine_background(&tag_name),
                    color: None,
                };

                self.layout_nodes.insert(dom_node, layout_node);

                // 递归处理子节点
                for child in dom.children(dom_node) {
                    self.build_tree_recursive(child, taffy_node, dom)?;
                }
            } else if node_ref.as_text().is_some() {
                // 文本节点 - 简单处理
            } else {
                // 其他类型节点
                for child in dom.children(dom_node) {
                    self.build_tree_recursive(child, taffy_parent, dom)?;
                }
            }
        }

        Ok(())
    }

    /// 根据标签名确定默认样式
    fn determine_style(&self, tag_name: &str, attrs: &kuchiki::AttributeMap) -> Style {
        let tag_lower = tag_name.to_lowercase();
        let mut style = Style::default();

        // 默认显示方式
        match tag_lower.as_str() {
            "div" | "p" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "section" | "article"
            | "header" | "footer" | "nav" | "aside" | "main" | "ul" | "ol" | "li"
            | "form" | "table" | "tr" => {
                style.display = Display::Block;
                style.flex_direction = FlexDirection::Column;
                style.margin = Rect::from_length(0.0, 0.0, 10.0, 0.0);
            }
            "span" | "a" | "em" | "strong" | "b" | "i" | "code" => {
                style.display = Display::Inline;
            }
            "img" => {
                style.display = Display::Block;
                let width = attrs.get("width")
                    .and_then(|w| w.parse::<f32>().ok())
                    .unwrap_or(200.0);
                let height = attrs.get("height")
                    .and_then(|h| h.parse::<f32>().ok())
                    .unwrap_or(150.0);
                style.size = Size {
                    width: Dimension::Length(width),
                    height: Dimension::Length(height),
                };
            }
            "br" => {
                style.display = Display::Block;
                style.size = Size {
                    width: Dimension::Percent(1.0),
                    height: Dimension::Length(20.0),
                };
            }
            "hr" => {
                style.display = Display::Block;
                style.size = Size {
                    width: Dimension::Percent(1.0),
                    height: Dimension::Length(2.0),
                };
            }
            "body" | "html" => {
                style.display = Display::Block;
                style.size = Size {
                    width: Dimension::Percent(1.0),
                    height: Dimension::Auto,
                };
                style.padding = Rect::from_length(10.0, 10.0, 10.0, 10.0);
                style.flex_direction = FlexDirection::Column;
            }
            _ => {
                style.display = Display::Block;
            }
        }

        // 根据标签设置默认尺寸
        match tag_lower.as_str() {
            "h1" => {
                style.size = Size {
                    width: Dimension::Percent(1.0),
                    height: Dimension::Length(50.0),
                };
            }
            "h2" => {
                style.size = Size {
                    width: Dimension::Percent(1.0),
                    height: Dimension::Length(40.0),
                };
            }
            "h3" | "h4" | "h5" | "h6" => {
                style.size = Size {
                    width: Dimension::Percent(1.0),
                    height: Dimension::Length(30.0),
                };
            }
            "p" => {
                style.size = Size {
                    width: Dimension::Percent(1.0),
                    height: Dimension::Length(40.0),
                };
            }
            "li" => {
                style.size = Size {
                    width: Dimension::Percent(1.0),
                    height: Dimension::Length(25.0),
                };
                style.padding = Rect::from_length(0.0, 0.0, 0.0, 10.0);
            }
            _ => {}
        }

        style
    }

    /// 确定默认背景颜色
    fn determine_background(&self, tag_name: &str) -> Option<Color> {
        match tag_name.to_lowercase().as_str() {
            "hr" => Some(Color::from_hex("#cccccc")),
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => Some(Color::from_hex("#333333")),
            _ => None,
        }
    }

    /// 计算布局
    fn compute_layout(&mut self) -> Result<(), String> {
        if let Some(root) = self.root {
            self.taffy
                .compute_layout(root, self.viewport)
                .map_err(|e| format!("布局计算失败: {}", e))?;

            for (dom_node, mut layout_node) in self.layout_nodes.clone() {
                let layout = self.taffy.layout(layout_node.node)
                    .map_err(|e| format!("获取节点布局失败: {}", e))?;
                layout_node.x = layout.location.x;
                layout_node.y = layout.location.y;
                layout_node.width = layout.size.width;
                layout_node.height = layout.size.height;

                self.layout_nodes.insert(dom_node, layout_node);
            }
        }

        Ok(())
    }

    /// 获取 dom 节点的布局
    pub fn get_layout(&self, dom_node: usize) -> Option<&LayoutNode> {
        self.layout_nodes.get(&dom_node)
    }

    /// 获取所有布局节点
    pub fn get_all_layout_nodes(&self) -> Vec<&LayoutNode> {
        self.layout_nodes.values().collect()
    }

    /// 清空布局
    pub fn clear(&mut self) {
        debug!("清空布局");
        self.layout_nodes.clear();
        self.root = None;
    }
}

impl Default for TaffyLayoutEngine {
    fn default() -> Self {
        Self::new(1280.0, 720.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_layout_creation() {
        let engine = TaffyLayoutEngine::new(800.0, 600.0);
        assert!(engine.layout_nodes.is_empty());
    }
}
