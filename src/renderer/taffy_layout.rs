//! Taffy 布局引擎 - 使用 taffy 进行完整的 CSS 布局计算
//!
//! 整合 kuchiki DOM、CSS 解析和 taffy

use crate::css::values::Color;
use crate::css_engine::{get_declaration, parse_inline_style, parse_length, Declaration, StyleMap};
use crate::renderer::renderer::global_font_system;
use crate::DomWrapper;
use cosmic_text::{Align, Attrs, Buffer, Metrics, Shaping, Wrap};
use kuchiki::NodeRef;
use log::info;
use std::collections::HashMap;
use std::rc::Rc;
use taffy::prelude::*;

/// CSS float 类型
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FloatType {
    None,
    Left,
    Right,
}

impl Default for FloatType {
    fn default() -> Self {
        FloatType::None
    }
}

/// CSS position 类型
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PositionType {
    Static,
    Relative,
    Absolute,
    Fixed,
}

impl Default for PositionType {
    fn default() -> Self {
        PositionType::Static
    }
}

/// 单个节点的布局信息（完整版）
#[derive(Debug, Clone)]
pub struct TaffyLayoutNode {
    /// taffy 节点 id
    pub node: NodeId,
    /// 对应的 dom 节点索引（DomWrapper 索引）
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
    /// 节点深度
    pub depth: usize,
    /// 从 CSS font-size 解析的字体大小
    pub font_size: f32,
    /// 字体颜色
    pub font_color: Option<Color>,
    /// 字体族
    pub font_family: Option<String>,
    /// CSS position
    pub position_type: PositionType,
    /// CSS float
    pub float_type: FloatType,
    /// CSS background-image URL
    pub background_image: Option<String>,
    /// CSS line-height（倍率，默认 1.375）
    pub line_height: f32,
}

/// 完整的 taffy 布局引擎（完整版）
pub struct TaffyLayoutEngine {
    /// taffy 树管理器
    taffy: TaffyTree,
    /// layout 节点列表
    layout_nodes: Vec<TaffyLayoutNode>,
    /// DOM 节点索引 → layout_nodes 索引映射
    dom_to_layout: HashMap<usize, usize>,
    /// 根节点
    root: Option<NodeId>,
    /// 视口尺寸
    viewport: Size<AvailableSpace>,
    /// CSS 样式映射
    style_map: StyleMap,
    /// 总节点数
    total_node_count: usize,
}

impl TaffyLayoutEngine {
    /// 创建新的布局引擎
    pub fn new(viewport_width: f32, viewport_height: f32) -> Self {
        info!(
            "初始化 Taffy 布局引擎 ({:.0}x{:.0})",
            viewport_width, viewport_height
        );

        let viewport = Size {
            width: AvailableSpace::Definite(viewport_width),
            height: AvailableSpace::Definite(viewport_height),
        };

        Self {
            taffy: TaffyTree::new(),
            layout_nodes: Vec::new(),
            dom_to_layout: HashMap::new(),
            root: None,
            viewport,
            style_map: StyleMap::new(),
            total_node_count: 0,
        }
    }

    /// 设置视口尺寸
    pub fn set_viewport(&mut self, width: f32, height: f32) {
        info!("更新视口: {:.0}x{:.0}", width, height);
        self.viewport = Size {
            width: AvailableSpace::Definite(width),
            height: AvailableSpace::Definite(height),
        };
    }

    /// 计算完整的布局，从 dom 开始
    ///
    /// 使用 kuchiki 的 NodeRef 直接遍历 DOM，
    /// 为每个可见元素创建 taffy 节点，建立映射，计算布局，提取坐标。
    pub fn compute(&mut self, dom: &DomWrapper) -> Result<(), String> {
        info!("开始计算完整布局");

        // 1. 清除所有旧状态
        self.clear();
        self.taffy = TaffyTree::new();

        // 2. 获取文档根 NodeRef
        let doc_root = dom.inner_document().clone();

        // 3. 递归遍历 NodeRef 构建 taffy 树
        //    先创建根节点（html 元素或文档本身）
        let root_style = Style {
            display: Display::Block,
            size: Size {
                width: percent(1.0),
                height: auto(),
            },
            flex_direction: FlexDirection::Column,
            ..Default::default()
        };

        let root_node = self
            .taffy
            .new_leaf(root_style)
            .map_err(|e| format!("创建根节点失败: {}", e))?;
        self.root = Some(root_node);

        let root_depth = 0;
        self.build_from_noderef(&doc_root, root_node, root_depth, dom)?;

        // 4. 计算布局
        self.compute_layout()?;

        info!(
            "布局计算完成！共 {} 个节点 (taffy: {})",
            self.layout_nodes.len(),
            self.total_node_count
        );
        Ok(())
    }

    /// 递归遍历 NodeRef 构建 taffy 树
    fn build_from_noderef(
        &mut self,
        node_ref: &NodeRef,
        parent_node: NodeId,
        depth: usize,
        dom: &DomWrapper,
    ) -> Result<(), String> {
        if let Some(element) = node_ref.as_element() {
            let tag_name = element.name.local.to_string();
            let attrs = element.attributes.borrow();

            // 获取 dom 索引
            let rc_ptr = Rc::as_ptr(&node_ref.0) as usize;
            let dom_idx = dom
                .inner_document()
                .descendants()
                .position(|n| Rc::as_ptr(&n.0) as usize == rc_ptr)
                .unwrap_or(self.total_node_count);

            // 解析内联样式
            let inline_decls: Vec<Declaration> = attrs
                .get("style")
                .map(|s| parse_inline_style(s))
                .unwrap_or_default();

            // 从 style_map 获取对应的声明
            let mut merged_decls: Vec<Declaration> =
                self.style_map.get(&tag_name).cloned().unwrap_or_default();
            merged_decls.extend(inline_decls);

            // 确定样式
            let style = self.determine_style(&tag_name, &merged_decls);

            let taffy_node = self
                .taffy
                .new_leaf(style)
                .map_err(|e| format!("创建元素节点 {} 失败: {}", tag_name, e))?;

            self.taffy
                .add_child(parent_node, taffy_node)
                .map_err(|e| format!("添加子节点 {} 失败: {}", tag_name, e))?;

            // 收集直接文本子节点内容
            let text_content = self.collect_element_text(node_ref);

            // 测量文本高度（如果有文本内容）
            let font_size = self.determine_font_size(&merged_decls);
            let line_height = self.determine_line_height(&merged_decls);
            let text_height = if !text_content.is_empty() {
                self.measure_text_height(&text_content, font_size, line_height)
            } else {
                0.0
            };

            // 创建布局节点
            let layout_node = TaffyLayoutNode {
                node: taffy_node,
                dom_node: dom_idx,
                tag_name: tag_name.clone(),
                x: 0.0,
                y: 0.0,
                width: 0.0,
                height: if text_height > 0.0 { text_height } else { 0.0 },
                background: self.determine_background(&merged_decls),
                color: self.determine_color(&merged_decls),
                depth,
                font_size: self.determine_font_size(&merged_decls),
                font_color: self.determine_font_color(&merged_decls),
                font_family: self.determine_font_family(&merged_decls),
                position_type: self.determine_position_type(&merged_decls),
                float_type: self.determine_float_type(&merged_decls),
                background_image: self.determine_background_image(&merged_decls),
                line_height: self.determine_line_height(&merged_decls),
            };

            let layout_idx = self.layout_nodes.len();
            self.layout_nodes.push(layout_node);
            self.dom_to_layout.insert(dom_idx, layout_idx);
            self.total_node_count += 1;

            // 递归处理子节点
            for child in node_ref.children() {
                self.build_from_noderef(&child, taffy_node, depth + 1, dom)?;
            }
        } else if node_ref.as_text().is_some() {
            // 文本节点 - 跳过，由父元素处理
        } else {
            // 文档节点等 - 直接递归子节点
            for child in node_ref.children() {
                self.build_from_noderef(&child, parent_node, depth, dom)?;
            }
        }

        Ok(())
    }

    /// 收集元素的直接文本子节点内容
    fn collect_element_text(&self, node_ref: &NodeRef) -> String {
        let mut result = String::new();
        for child in node_ref.children() {
            if let Some(text) = child.as_text() {
                let content = text.borrow();
                let trimmed = content.trim();
                if !trimmed.is_empty() {
                    if !result.is_empty() {
                        result.push(' ');
                    }
                    result.push_str(trimmed);
                }
            }
        }
        result
    }

    /// 用 cosmic-text 测量文本高度
    fn measure_text_height(&self, text: &str, font_size: f32, line_height: f32) -> f32 {
        if text.trim().is_empty() {
            return 0.0;
        }

        let max_width = match self.viewport.width {
            AvailableSpace::Definite(w) => w - 40.0, // 留边距
            _ => 800.0,
        };

        // line_height 可能是倍率（如 1.54）或 px 值（如 22.0）
        // 如果 > 20 且 < 200 视为 px 值；否则视为倍率
        let lh = if line_height > 0.0 {
            if line_height > 20.0 && line_height < 200.0 {
                // px 值
                line_height
            } else {
                // 倍率
                font_size * line_height
            }
        } else {
            font_size * 1.375
        };

        let mut font_system = global_font_system().lock().unwrap();
        let mut buffer = Buffer::new(&mut font_system, Metrics::new(font_size, lh));

        buffer.set_size(Some(max_width.max(100.0)), Some(f32::INFINITY));
        buffer.set_wrap(Wrap::Word);
        let attrs = Attrs::new();
        buffer.set_text(text, &attrs, Shaping::Advanced, Some(Align::Left));
        buffer.shape_until_scroll(&mut font_system, true);

        let total_height = buffer.layout_runs().count() as f32 * lh;
        total_height + 10.0 // 额外 padding
    }

    /// 确定样式：合并标签默认 + style_map + 内联样式
    fn determine_style(&self, tag_name: &str, decls: &[Declaration]) -> Style {
        let tag_lower = tag_name.to_lowercase();
        let mut style = Style::default();

        // 默认显示方式
        match tag_lower.as_str() {
            "div" | "p" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "section" | "article"
            | "header" | "footer" | "nav" | "aside" | "main" | "ul" | "ol" | "li" | "form"
            | "table" | "tr" => {
                style.display = Display::Block;
                style.flex_direction = FlexDirection::Column;
            }
            "span" | "a" | "em" | "strong" | "b" | "i" | "code" => {
                style.display = Display::Block;
            }
            "img" => {
                style.display = Display::Block;
                // 默认尺寸
                style.size = Size {
                    width: length(200.0),
                    height: length(150.0),
                };
            }
            "br" => {
                style.display = Display::Block;
                style.size = Size {
                    width: percent(1.0),
                    height: length(20.0),
                };
            }
            "hr" => {
                style.display = Display::Block;
                style.size = Size {
                    width: percent(1.0),
                    height: length(2.0),
                };
            }
            "body" | "html" => {
                style.display = Display::Block;
                style.size = Size {
                    width: percent(1.0),
                    height: auto(),
                };
                style.padding = Rect {
                    left: length(10.0),
                    right: length(10.0),
                    top: length(10.0),
                    bottom: length(10.0),
                };
                style.flex_direction = FlexDirection::Column;
            }
            _ => {
                style.display = Display::Block;
            }
        }

        // 标签默认尺寸
        match tag_lower.as_str() {
            "h1" => {
                style.size = Size {
                    width: percent(1.0),
                    height: length(50.0),
                };
            }
            "h2" => {
                style.size = Size {
                    width: percent(1.0),
                    height: length(40.0),
                };
            }
            "h3" | "h4" | "h5" | "h6" => {
                style.size = Size {
                    width: percent(1.0),
                    height: length(30.0),
                };
            }
            "p" => {
                style.size = Size {
                    width: percent(1.0),
                    height: length(40.0),
                };
            }
            "li" => {
                style.size = Size {
                    width: percent(1.0),
                    height: length(25.0),
                };
                style.padding = Rect {
                    left: length(0.0),
                    right: length(0.0),
                    top: length(0.0),
                    bottom: length(10.0),
                };
            }
            _ => {}
        }

        // 应用 CSS 声明（如果有）
        if let Some(display_val) = get_declaration(decls, "display") {
            match display_val.as_str() {
                "block" => style.display = Display::Block,
                "flex" | "inline-flex" => {
                    style.display = Display::Flex;
                    style.flex_direction = FlexDirection::Column;
                }
                "grid" => style.display = Display::Grid,
                "none" => style.display = Display::None,
                "inline" => style.display = Display::Block,
                "inline-block" => style.display = Display::Block,
                _ => {}
            }
        }

        // 解析 width
        if let Some(w) = get_declaration(decls, "width") {
            if let Some(px) = parse_length(&w) {
                if w.contains('%') {
                    style.size.width = percent(px / 100.0);
                } else {
                    style.size.width = length(px);
                }
            } else if w == "auto" {
                style.size.width = auto();
            }
        }

        // 解析 height
        if let Some(h) = get_declaration(decls, "height") {
            if let Some(px) = parse_length(&h) {
                if h.contains('%') {
                    style.size.height = percent(px / 100.0);
                } else {
                    style.size.height = length(px);
                }
            } else if h == "auto" {
                style.size.height = auto();
            }
        }

        // 解析 margin
        if let Some(m) = get_declaration(decls, "margin") {
            let parts: Vec<&str> = m.split_whitespace().collect();
            if parts.len() == 1 {
                if let Some(px) = parse_length(parts[0]) {
                    let mval = if parts[0].contains('%') {
                        percent(px / 100.0)
                    } else {
                        length(px)
                    };
                    style.margin = Rect {
                        left: mval,
                        right: mval,
                        top: mval,
                        bottom: mval,
                    };
                }
            }
        }

        // 解析 padding
        if let Some(p) = get_declaration(decls, "padding") {
            let parts: Vec<&str> = p.split_whitespace().collect();
            if parts.len() == 1 {
                if let Some(px) = parse_length(parts[0]) {
                    let pval = if parts[0].contains('%') {
                        percent(px / 100.0)
                    } else {
                        length(px)
                    };
                    style.padding = Rect {
                        left: pval,
                        right: pval,
                        top: pval,
                        bottom: pval,
                    };
                }
            }
        }

        // 解析 flex-direction
        if let Some(fd) = get_declaration(decls, "flex-direction") {
            match fd.as_str() {
                "row" => style.flex_direction = FlexDirection::Row,
                "column" => style.flex_direction = FlexDirection::Column,
                "row-reverse" => style.flex_direction = FlexDirection::RowReverse,
                "column-reverse" => style.flex_direction = FlexDirection::ColumnReverse,
                _ => {}
            }
        }

        // 解析 justify-content
        if let Some(jc) = get_declaration(decls, "justify-content") {
            match jc.as_str() {
                "flex-start" => style.justify_content = Some(JustifyContent::FlexStart),
                "flex-end" => style.justify_content = Some(JustifyContent::FlexEnd),
                "center" => style.justify_content = Some(JustifyContent::Center),
                "space-between" => style.justify_content = Some(JustifyContent::SpaceBetween),
                "space-around" => style.justify_content = Some(JustifyContent::SpaceAround),
                "space-evenly" => style.justify_content = Some(JustifyContent::SpaceEvenly),
                _ => {}
            }
        }

        // 解析 align-items
        if let Some(ai) = get_declaration(decls, "align-items") {
            match ai.as_str() {
                "flex-start" => style.align_items = Some(AlignItems::FlexStart),
                "flex-end" => style.align_items = Some(AlignItems::FlexEnd),
                "center" => style.align_items = Some(AlignItems::Center),
                "stretch" => style.align_items = Some(AlignItems::Stretch),
                "baseline" => style.align_items = Some(AlignItems::Baseline),
                _ => {}
            }
        }

        style
    }

    /// 确定背景颜色
    fn determine_background(&self, decls: &[Declaration]) -> Option<Color> {
        if let Some(bg) = get_declaration(decls, "background-color") {
            // 尝试 parse 颜色值
            if bg.starts_with('#') {
                return Some(Color::from_hex(&bg));
            }
            if let Some(named) = Color::from_name(&bg) {
                return Some(named);
            }
            // 尝试 rgb/rgba 解析
            if bg.starts_with("rgb") {
                // 简化处理
                let cleaned = bg.replace("rgba(", "").replace("rgb(", "").replace(")", "");
                let parts: Vec<&str> = cleaned.split(',').collect();
                if parts.len() >= 3 {
                    if let (Ok(r), Ok(g), Ok(b)) = (
                        parts[0].trim().parse::<u8>(),
                        parts[1].trim().parse::<u8>(),
                        parts[2].trim().parse::<u8>(),
                    ) {
                        let a = if parts.len() >= 4 {
                            (parts[3].trim().parse::<f32>().unwrap_or(1.0) * 255.0) as u8
                        } else {
                            255
                        };
                        return Some(Color::rgba(r, g, b, a));
                    }
                }
            }
        }

        // background 简写（仅提取可能的颜色）
        if let Some(bg) = get_declaration(decls, "background") {
            let parts: Vec<&str> = bg.split_whitespace().collect();
            for part in parts {
                if part.starts_with('#') {
                    return Some(Color::from_hex(part));
                }
                if let Some(named) = Color::from_name(part) {
                    return Some(named);
                }
            }
        }

        None
    }

    /// 确定 color 属性
    fn determine_color(&self, decls: &[Declaration]) -> Option<Color> {
        if let Some(c) = get_declaration(decls, "color") {
            if c.starts_with('#') {
                return Some(Color::from_hex(&c));
            }
            if let Some(named) = Color::from_name(&c) {
                return Some(named);
            }
        }
        None
    }

    /// 确定 font-size
    fn determine_font_size(&self, decls: &[Declaration]) -> f32 {
        if let Some(fs) = get_declaration(decls, "font-size") {
            if let Some(px) = parse_length(&fs) {
                return px;
            }
            // 处理关键字
            match fs.trim() {
                "xx-small" => return 9.0,
                "x-small" => return 10.0,
                "small" => return 13.0,
                "medium" => return 16.0,
                "large" => return 18.0,
                "x-large" => return 24.0,
                "xx-large" => return 32.0,
                "smaller" => return 14.0,
                "larger" => return 18.0,
                _ => {}
            }
        }
        16.0 // 默认字体大小
    }

    /// 确定字体颜色
    fn determine_font_color(&self, decls: &[Declaration]) -> Option<Color> {
        // 优先使用 color 属性作为字体颜色
        self.determine_color(decls)
    }

    /// 确定 font-family
    fn determine_font_family(&self, decls: &[Declaration]) -> Option<String> {
        if let Some(ff) = get_declaration(decls, "font-family") {
            // 去除引号
            let cleaned = ff
                .replace('"', "")
                .replace('\'', "")
                .split(',')
                .next()
                .map(|s| s.trim().to_string());
            return cleaned;
        }
        None
    }

    /// 确定 line-height，返回倍率（如 1.375）。默认返回 0.0（调用方用 font_size * 1.375）
    fn determine_line_height(&self, decls: &[Declaration]) -> f32 {
        if let Some(lh) = get_declaration(decls, "line-height") {
            let lh = lh.trim();
            // 无单位数值（如 1.54）=> 倍率
            if let Ok(ratio) = lh.parse::<f32>() {
                return ratio;
            }
            // 带 px 的数值（如 22px）=> 需要转换成相对于 font-size 的倍率
            // 但由于我们在测量时还不知道 font-size，这里先返回数值
            // 调用方处理：如果返回值 > 100 视为 px，否则视为倍率
            if let Some(px) = parse_length(lh) {
                return px; // px 值，调用方会判断
            }
        }
        0.0 // 默认 0，调用方 fallback 到 font_size * 1.375
    }

    /// 确定 background-image URL
    fn determine_background_image(&self, decls: &[Declaration]) -> Option<String> {
        if let Some(bg_img) = get_declaration(decls, "background-image") {
            let trimmed = bg_img.trim();
            if trimmed.starts_with("url(") {
                let inner = trimmed
                    .strip_prefix("url(")
                    .and_then(|s| s.strip_suffix(')'))
                    .unwrap_or(trimmed);
                let cleaned = inner.replace('"', "").replace('\'', "").trim().to_string();
                if !cleaned.is_empty() {
                    return Some(cleaned);
                }
            }
        }
        None
    }

    /// 确定 position 类型
    fn determine_position_type(&self, decls: &[Declaration]) -> PositionType {
        if let Some(pos) = get_declaration(decls, "position") {
            match pos.trim() {
                "relative" => return PositionType::Relative,
                "absolute" => return PositionType::Absolute,
                "fixed" => return PositionType::Fixed,
                _ => {}
            }
        }
        PositionType::Static
    }

    /// 确定 float 类型
    fn determine_float_type(&self, decls: &[Declaration]) -> FloatType {
        if let Some(fl) = get_declaration(decls, "float") {
            match fl.trim() {
                "left" => return FloatType::Left,
                "right" => return FloatType::Right,
                _ => {}
            }
        }
        FloatType::None
    }

    /// 计算布局
    fn compute_layout(&mut self) -> Result<(), String> {
        if let Some(root) = self.root {
            self.taffy
                .compute_layout(root, self.viewport)
                .map_err(|e| format!("布局计算失败: {}", e))?;

            // 提取坐标到 layout_nodes
            for layout_node in self.layout_nodes.iter_mut() {
                let layout = self
                    .taffy
                    .layout(layout_node.node)
                    .map_err(|e| format!("获取节点布局失败: {}", e))?;
                layout_node.x = layout.location.x;
                layout_node.y = layout.location.y;
                layout_node.width = layout.size.width;
                layout_node.height = layout.size.height;
            }
        }

        Ok(())
    }

    /// 点击测试：查找 (x, y) 位置对应的布局节点
    ///
    /// 从后往前遍历（后渲染的在上面，覆盖前面的），
    /// 返回第一个匹配的 TaffyLayoutNode 引用。
    pub fn hit_test(&self, x: f32, y: f32) -> Option<&TaffyLayoutNode> {
        self.layout_nodes
            .iter()
            .rev()
            .find(|n| x >= n.x && x <= n.x + n.width && y >= n.y && y <= n.y + n.height)
    }

    /// 通过 dom 索引获取布局节点
    pub fn get_layout(&self, dom_node: usize) -> Option<&TaffyLayoutNode> {
        self.dom_to_layout
            .get(&dom_node)
            .and_then(|&idx| self.layout_nodes.get(idx))
    }

    /// 通过 dom 索引获取布局矩形 (x, y, w, h)
    pub fn get_layout_rect(&self, dom_node: usize) -> Option<(f32, f32, f32, f32)> {
        self.get_layout(dom_node)
            .map(|n| (n.x, n.y, n.width, n.height))
    }

    /// 根据标签名查找布局节点
    pub fn find_by_tag(&self, tag_name: &str) -> Vec<&TaffyLayoutNode> {
        self.layout_nodes
            .iter()
            .filter(|n| n.tag_name == tag_name)
            .collect()
    }

    /// 获取所有布局节点
    pub fn get_all_layout_nodes(&self) -> Vec<&TaffyLayoutNode> {
        self.layout_nodes.iter().collect()
    }

    /// 设置 CSS 样式映射
    pub fn set_style_map(&mut self, map: StyleMap) {
        self.style_map = map;
    }

    /// 获取 dom → layout 映射
    pub fn dom_to_layout_map(&self) -> &HashMap<usize, usize> {
        &self.dom_to_layout
    }

    /// 布局节点数量
    pub fn len(&self) -> usize {
        self.layout_nodes.len()
    }

    /// 是否为空
    pub fn is_empty(&self) -> bool {
        self.layout_nodes.is_empty()
    }

    /// 清空布局
    pub fn clear(&mut self) {
        info!("清空布局");
        self.layout_nodes.clear();
        self.dom_to_layout.clear();
        self.root = None;
        self.style_map.clear();
        self.total_node_count = 0;
    }

    /// 返回处理的节点总数
    pub fn total_nodes_processed(&self) -> usize {
        self.total_node_count
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
    fn test_create_engine() {
        let engine = TaffyLayoutEngine::new(800.0, 600.0);
        assert!(engine.is_empty());
        assert_eq!(engine.len(), 0);
        assert_eq!(engine.total_nodes_processed(), 0);
    }

    #[test]
    fn test_compute_simple_html() {
        use crate::DomWrapper;
        let html = "<html><body><p>Hello World</p></body></html>";
        let dom = DomWrapper::from_html(html, None);
        let mut engine = TaffyLayoutEngine::new(800.0, 600.0);
        assert!(engine.compute(&dom).is_ok());
        assert!(!engine.is_empty());
        assert!(engine.len() > 0);
    }

    #[test]
    fn test_get_layout_by_dom_index() {
        use crate::DomWrapper;
        let html = "<html><body><div id='test'>Content</div></body></html>";
        let dom = DomWrapper::from_html(html, None);
        let mut engine = TaffyLayoutEngine::new(800.0, 600.0);
        assert!(engine.compute(&dom).is_ok());

        // 应该能找到 body 的布局（index 至少包含 html/body/div）
        let all_nodes = engine.get_all_layout_nodes();
        assert!(!all_nodes.is_empty());
    }

    #[test]
    fn test_find_by_tag() {
        use crate::DomWrapper;
        let html = "<html><body><div>1</div><div>2</div><p>3</p></body></html>";
        let dom = DomWrapper::from_html(html, None);
        let mut engine = TaffyLayoutEngine::new(800.0, 600.0);
        assert!(engine.compute(&dom).is_ok());

        let divs = engine.find_by_tag("div");
        assert!(divs.len() >= 2);

        let ps = engine.find_by_tag("p");
        assert!(ps.len() >= 1);
    }
}
