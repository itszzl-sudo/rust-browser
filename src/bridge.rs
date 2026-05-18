//! Web → Native 桥接层 - 独立接口定义
//!
//! 定义 [`WebNativeBridge`] trait，供 web-to-native 工具产出的 Rust 代码调用。
//! 不依赖任何浏览器引擎内部类型，外部项目可以实现此 trait 接入自己的渲染引擎。
//!
//! # 工作流
//!
//! ```ignore
//! let mut bridge: Box<dyn WebNativeBridge> = /* 你的实现 */;
//!
//! // ① 写入 DOM
//! bridge.set_html(r#"<div id="app"><button id="btn">Click</button></div>"#);
//!
//! // ② 绑定事件
//! bridge.on_click("#btn", Box::new(|x, y| {
//!     println!("按钮被点击了！");
//! }));
//!
//! // ③ 执行 JS
//! bridge.eval_js("console.log('hello')");
//!
//! // ④ 渲染
//! let png = bridge.render();
//!
//! // ⑤ 修改样式 → 重新渲染
//! bridge.set_style("#btn", "background-color", "red");
//! let png2 = bridge.render();
//!
//! // ⑥ 获取元素位置
//! let rect = bridge.get_rect("#btn");
//! ```

use std::collections::HashMap;

// ---------------------------------------------------------------------------
// 独立类型定义（不依赖外部 crate）
// ---------------------------------------------------------------------------

/// RGBA 颜色
#[derive(Debug, Clone, PartialEq)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    pub const BLACK: Color = Color {
        r: 0,
        g: 0,
        b: 0,
        a: 255,
    };
    pub const WHITE: Color = Color {
        r: 255,
        g: 255,
        b: 255,
        a: 255,
    };
    pub const RED: Color = Color {
        r: 255,
        g: 0,
        b: 0,
        a: 255,
    };
    pub const TRANSPARENT: Color = Color {
        r: 0,
        g: 0,
        b: 0,
        a: 0,
    };

    pub fn from_rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    pub fn from_hex(hex: &str) -> Self {
        let hex = hex.trim_start_matches('#');
        let len = hex.len();
        let parse_hex = |s: &str| u8::from_str_radix(s, 16).unwrap_or(0);
        match len {
            3 => Self {
                r: parse_hex(&hex[0..1]) * 17,
                g: parse_hex(&hex[1..2]) * 17,
                b: parse_hex(&hex[2..3]) * 17,
                a: 255,
            },
            6 => Self {
                r: parse_hex(&hex[0..2]),
                g: parse_hex(&hex[2..4]),
                b: parse_hex(&hex[4..6]),
                a: 255,
            },
            8 => Self {
                r: parse_hex(&hex[0..2]),
                g: parse_hex(&hex[2..4]),
                b: parse_hex(&hex[4..6]),
                a: parse_hex(&hex[6..8]),
            },
            _ => Self {
                r: 0,
                g: 0,
                b: 0,
                a: 255,
            },
        }
    }
}

/// 元素在页面上的布局矩形
#[derive(Debug, Clone, Copy)]
pub struct LayoutRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

/// 单个布局节点的详细信息
#[derive(Debug, Clone)]
pub struct LayoutNode {
    pub dom_node: usize,
    pub tag_name: String,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub background: Option<Color>,
}

/// CSS 声明
#[derive(Debug, Clone)]
pub struct Declaration {
    pub property: String,
    pub value: String,
}

// ---------------------------------------------------------------------------
// 事件处理器类型
// ---------------------------------------------------------------------------

/// 点击事件处理器：接收点击坐标 (x, y)
pub type EventHandler = Box<dyn FnMut(f32, f32) + Send>;

/// 表单提交处理器：接收字段名 → 值的映射
pub type FormHandler = Box<dyn FnMut(HashMap<String, String>) + Send>;

// ---------------------------------------------------------------------------
// Bridge Trait
// ---------------------------------------------------------------------------

/// Web → Native 桥接器接口
///
/// 所有方法都有默认实现或可直接调用，外部项目实现此 trait 即可。
pub trait WebNativeBridge {
    /// 创建桥接器
    fn new(width: u32, height: u32) -> Self
    where
        Self: Sized;

    // ── DOM 读写 ──

    /// 设置页面 HTML
    fn set_html(&mut self, html: &str);

    /// 按 CSS 选择器查找第一个元素，返回 DOM 节点 ID
    fn query(&self, selector: &str) -> Option<usize>;

    /// 按 CSS 选择器查找所有匹配元素
    fn query_all(&self, selector: &str) -> Vec<usize>;

    /// 获取元素标签名
    fn tag_name(&self, node_id: usize) -> Option<String>;

    /// 获取元素属性
    fn get_attr(&self, node_id: usize, name: &str) -> Option<String>;

    /// 设置元素属性
    fn set_attr(&mut self, node_id: usize, name: &str, value: &str);

    /// 获取元素文本内容
    fn text(&self, node_id: usize) -> Option<String>;

    /// 获取父节点 ID
    fn parent_node(&self, node_id: usize) -> Option<usize>;

    /// 按选择器获取元素文本
    fn query_text(&self, selector: &str) -> Option<String> {
        let id = self.query(selector)?;
        self.text(id)
    }

    // ── 布局 ──

    /// 获取元素在页面上的位置
    fn get_rect(&self, selector: &str) -> Option<LayoutRect>;

    /// 获取所有布局节点
    fn all_rects(&self) -> Vec<LayoutNode>;

    /// 点击测试
    fn hit_test(&self, x: f32, y: f32) -> Option<LayoutNode>;

    // ── CSS 操作 ──

    /// 添加 CSS 规则
    fn set_css(&mut self, css_text: &str);

    /// 设置元素内联样式
    fn set_style(&mut self, selector: &str, property: &str, value: &str);

    /// 清除自定义 CSS
    fn clear_css(&mut self);

    // ── JS 执行 ──

    /// 执行 JavaScript 代码
    fn eval_js(&mut self, code: &str) -> String;

    // ── 渲染 ──

    /// 渲染当前页面，返回 PNG 字节
    fn render(&mut self) -> Vec<u8>;

    // ── 事件绑定 ──

    /// 绑定点击事件
    fn on_click(&mut self, selector: &str, handler: EventHandler);

    /// 绑定表单提交事件
    fn on_form_submit(&mut self, selector: &str, handler: FormHandler);

    /// 处理鼠标点击（事件冒泡）
    fn handle_click(&mut self, x: f32, y: f32) -> bool;

    /// 处理表单提交
    fn handle_form_submit(&mut self, form_selector: &str);

    // ── 工具 ──

    /// 设置视口尺寸
    fn set_viewport(&mut self, width: u32, height: u32);

    /// 获取视口尺寸
    fn viewport(&self) -> (u32, u32);
}
