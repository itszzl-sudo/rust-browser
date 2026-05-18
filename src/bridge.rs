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

    // ── 网络请求 ──

    /// 导航到 URL
    fn navigate(&mut self, url: &str) -> Result<(), String>;

    /// 获取当前 URL
    fn current_url(&self) -> String;

    /// 发送 HTTP GET 请求
    fn http_get(&mut self, url: &str) -> Result<crate::network::HttpResponse, String>;

    /// 发送 HTTP POST 请求
    fn http_post(
        &mut self,
        url: &str,
        body: &[u8],
        content_type: &str,
    ) -> Result<crate::network::HttpResponse, String>;
}

// =========================================================================
// 基于 Mock 的 Trait 测试
// =========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    struct MockBridge {
        width: u32,
        height: u32,
        css_rules: Vec<String>,
        styles: HashMap<String, Vec<(String, String)>>,
        click_handlers: HashMap<String, EventHandler>,
        form_handlers: HashMap<String, FormHandler>,
        click_log: Vec<String>,
        form_log: Vec<String>,
        js_log: Vec<String>,
        js_return: String,
        nodes: Vec<MockNode>,
    }

    #[derive(Clone)]
    struct MockNode {
        id: usize,
        tag: String,
        text: String,
        attrs: HashMap<String, String>,
        parent: Option<usize>,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        bg: Option<Color>,
    }

    impl MockBridge {
        fn parse_html(&mut self, html: &str) {
            self.nodes.clear();
            let mut id_counter = 1usize;
            let mut rest = html;
            while let Some(tag_start) = rest.find('<') {
                let tag_end = rest[tag_start..].find('>').map(|p| tag_start + p + 1);
                match tag_end {
                    Some(end) => {
                        let tag_content = &rest[tag_start + 1..end - 1];
                        if tag_content.starts_with("!--") || tag_content.starts_with('/') {
                            rest = &rest[end..];
                            continue;
                        }
                        let parts: Vec<&str> = tag_content.split_whitespace().collect();
                        let tag_name = parts.first().map(|s| s.to_string()).unwrap_or_default();
                        let mut attrs = HashMap::new();
                        for part in parts.iter().skip(1) {
                            if let Some((k, v)) = part.split_once('=') {
                                let val = v.trim_matches('"');
                                attrs.insert(k.to_string(), val.to_string());
                            }
                        }
                        let node_id = id_counter;
                        id_counter += 1;
                        self.nodes.push(MockNode {
                            id: node_id,
                            tag: tag_name,
                            text: String::new(),
                            attrs,
                            parent: None,
                            x: 0.0,
                            y: 0.0,
                            w: 0.0,
                            h: 0.0,
                            bg: None,
                        });
                        rest = &rest[end..];
                    }
                    None => break,
                }
            }
        }

        fn find_by_selector(&self, selector: &str) -> Option<&MockNode> {
            if let Some(id_str) = selector.strip_prefix('#') {
                self.nodes
                    .iter()
                    .find(|n| n.attrs.get("id").map(|s| s.as_str()) == Some(id_str))
            } else {
                self.nodes.iter().find(|n| n.tag == selector)
            }
        }
    }

    impl WebNativeBridge for MockBridge {
        fn new(width: u32, height: u32) -> Self {
            Self {
                width,
                height,
                css_rules: Vec::new(),
                styles: HashMap::new(),
                click_handlers: HashMap::new(),
                form_handlers: HashMap::new(),
                click_log: Vec::new(),
                form_log: Vec::new(),
                js_log: Vec::new(),
                js_return: "undefined".to_string(),
                nodes: Vec::new(),
            }
        }

        fn set_html(&mut self, html: &str) {
            self.parse_html(html);
        }
        fn query(&self, selector: &str) -> Option<usize> {
            self.find_by_selector(selector).map(|n| n.id)
        }
        fn query_all(&self, selector: &str) -> Vec<usize> {
            if selector == "*" {
                return self.nodes.iter().map(|n| n.id).collect();
            }
            if let Some(id_str) = selector.strip_prefix('#') {
                self.nodes
                    .iter()
                    .find(|n| n.attrs.get("id").map(|s| s.as_str()) == Some(id_str))
                    .map(|n| vec![n.id])
                    .unwrap_or_default()
            } else {
                self.nodes
                    .iter()
                    .filter(|n| n.tag == selector)
                    .map(|n| n.id)
                    .collect()
            }
        }
        fn tag_name(&self, node_id: usize) -> Option<String> {
            self.nodes
                .iter()
                .find(|n| n.id == node_id)
                .map(|n| n.tag.clone())
        }
        fn get_attr(&self, node_id: usize, name: &str) -> Option<String> {
            self.nodes
                .iter()
                .find(|n| n.id == node_id)
                .and_then(|n| n.attrs.get(name).cloned())
        }
        fn set_attr(&mut self, node_id: usize, name: &str, value: &str) {
            if let Some(n) = self.nodes.iter_mut().find(|n| n.id == node_id) {
                n.attrs.insert(name.to_string(), value.to_string());
            }
        }
        fn text(&self, node_id: usize) -> Option<String> {
            self.nodes
                .iter()
                .find(|n| n.id == node_id)
                .map(|n| n.text.clone())
        }
        fn parent_node(&self, node_id: usize) -> Option<usize> {
            self.nodes
                .iter()
                .find(|n| n.id == node_id)
                .and_then(|n| n.parent)
        }
        fn get_rect(&self, selector: &str) -> Option<LayoutRect> {
            self.find_by_selector(selector).map(|n| LayoutRect {
                x: n.x,
                y: n.y,
                width: n.w,
                height: n.h,
            })
        }
        fn all_rects(&self) -> Vec<LayoutNode> {
            self.nodes
                .iter()
                .map(|n| LayoutNode {
                    dom_node: n.id,
                    tag_name: n.tag.clone(),
                    x: n.x,
                    y: n.y,
                    width: n.w,
                    height: n.h,
                    background: n.bg.clone(),
                })
                .collect()
        }
        fn hit_test(&self, x: f32, y: f32) -> Option<LayoutNode> {
            self.nodes
                .iter()
                .find(|n| x >= n.x && x <= n.x + n.w && y >= n.y && y <= n.y + n.h)
                .map(|n| LayoutNode {
                    dom_node: n.id,
                    tag_name: n.tag.clone(),
                    x: n.x,
                    y: n.y,
                    width: n.w,
                    height: n.h,
                    background: n.bg.clone(),
                })
        }
        fn set_css(&mut self, css_text: &str) {
            self.css_rules.push(css_text.to_string());
        }
        fn set_style(&mut self, selector: &str, property: &str, value: &str) {
            self.styles
                .entry(selector.to_string())
                .or_default()
                .push((property.to_string(), value.to_string()));
        }
        fn clear_css(&mut self) {
            self.css_rules.clear();
            self.styles.clear();
        }
        fn eval_js(&mut self, code: &str) -> String {
            self.js_log.push(code.to_string());
            self.js_return.clone()
        }
        fn render(&mut self) -> Vec<u8> {
            vec![0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]
        }
        fn on_click(&mut self, selector: &str, handler: EventHandler) {
            self.click_handlers.insert(selector.to_string(), handler);
        }
        fn on_form_submit(&mut self, selector: &str, handler: FormHandler) {
            self.form_handlers.insert(selector.to_string(), handler);
        }
        fn handle_click(&mut self, x: f32, y: f32) -> bool {
            if let Some(sel) = self
                .click_handlers
                .keys()
                .find(|sel| self.query(sel).is_some())
                .cloned()
            {
                if let Some(handler) = self.click_handlers.get_mut(&sel) {
                    handler(x, y);
                    self.click_log
                        .push(format!("{} @ ({:.0},{:.0})", sel, x, y));
                    return true;
                }
            }
            false
        }
        fn handle_form_submit(&mut self, form_selector: &str) {
            if let Some(handler) = self.form_handlers.get_mut(form_selector) {
                handler(HashMap::new());
                self.form_log.push(format!("form: {}", form_selector));
            }
        }
        fn set_viewport(&mut self, width: u32, height: u32) {
            self.width = width;
            self.height = height;
        }
        fn viewport(&self) -> (u32, u32) {
            (self.width, self.height)
        }

        fn navigate(&mut self, _url: &str) -> Result<(), String> {
            Ok(())
        }
        fn current_url(&self) -> String {
            "about:blank".to_string()
        }
        fn http_get(&mut self, _url: &str) -> Result<crate::network::HttpResponse, String> {
            Err("Mock: no network".to_string())
        }
        fn http_post(
            &mut self,
            _url: &str,
            _body: &[u8],
            _content_type: &str,
        ) -> Result<crate::network::HttpResponse, String> {
            Err("Mock: no network".to_string())
        }
    }

    fn setup() -> MockBridge {
        MockBridge::new(1280, 720)
    }

    // ── 类型测试 ──

    #[test]
    fn test_color_from_hex_3digit() {
        assert_eq!(
            Color::from_hex("#f00"),
            Color {
                r: 255,
                g: 0,
                b: 0,
                a: 255
            }
        );
    }

    #[test]
    fn test_color_from_hex_6digit() {
        assert_eq!(
            Color::from_hex("#ff8800"),
            Color {
                r: 255,
                g: 136,
                b: 0,
                a: 255
            }
        );
    }

    #[test]
    fn test_color_from_hex_8digit() {
        assert_eq!(
            Color::from_hex("#ff880080"),
            Color {
                r: 255,
                g: 136,
                b: 0,
                a: 128
            }
        );
    }

    #[test]
    fn test_color_constants() {
        assert_eq!(
            Color::BLACK,
            Color {
                r: 0,
                g: 0,
                b: 0,
                a: 255
            }
        );
        assert_eq!(
            Color::WHITE,
            Color {
                r: 255,
                g: 255,
                b: 255,
                a: 255
            }
        );
        assert_eq!(
            Color::RED,
            Color {
                r: 255,
                g: 0,
                b: 0,
                a: 255
            }
        );
        assert_eq!(
            Color::TRANSPARENT,
            Color {
                r: 0,
                g: 0,
                b: 0,
                a: 0
            }
        );
    }

    #[test]
    fn test_layout_rect_fields() {
        let r = LayoutRect {
            x: 10.0,
            y: 20.0,
            width: 100.0,
            height: 50.0,
        };
        assert_eq!(r.width, 100.0);
    }

    #[test]
    fn test_layout_node_background() {
        let bg = Color::from_hex("#ff0000");
        let n = LayoutNode {
            dom_node: 1,
            tag_name: "div".into(),
            x: 0.0,
            y: 0.0,
            width: 0.0,
            height: 0.0,
            background: Some(bg.clone()),
        };
        assert_eq!(n.background, Some(bg));
    }

    #[test]
    fn test_declaration() {
        let d = Declaration {
            property: "color".into(),
            value: "red".into(),
        };
        assert_eq!(d.property, "color");
    }

    // ── DOM 操作测试 ──

    #[test]
    fn test_set_html_and_query() {
        let mut b = setup();
        b.set_html(r#"<div id="app"><p id="text">Hello</p></div>"#);
        assert!(b.query("#app").is_some());
        assert!(b.query("#text").is_some());
        assert_eq!(b.query("#nonexistent"), None);
    }

    #[test]
    fn test_query_all() {
        let mut b = setup();
        b.set_html(r#"<div></div><div></div><span></span>"#);
        assert_eq!(b.query_all("div").len(), 2);
        assert_eq!(b.query_all("*").len(), 3);
    }

    #[test]
    fn test_tag_name() {
        let mut b = setup();
        b.set_html(r#"<div id="d"></div><span id="s"></span>"#);
        assert_eq!(b.tag_name(b.query("#d").unwrap()), Some("div".into()));
        assert_eq!(b.tag_name(b.query("#s").unwrap()), Some("span".into()));
    }

    #[test]
    fn test_attr_get_set() {
        let mut b = setup();
        b.set_html(r#"<div id="box" class="red"></div>"#);
        let id = b.query("#box").unwrap();
        assert_eq!(b.get_attr(id, "class"), Some("red".into()));
        b.set_attr(id, "class", "blue");
        assert_eq!(b.get_attr(id, "class"), Some("blue".into()));
    }

    #[test]
    fn test_query_text_default_impl() {
        let mut b = setup();
        b.set_html(r#"<p id="p1"></p>"#); // Mock 不解析文本内容，所以测试空文本
        assert!(b.query_text("#p1").is_some());
        assert_eq!(b.query_text("#nonexistent"), None);
    }

    // ── CSS 操作测试 ──

    #[test]
    fn test_set_css() {
        let mut b = setup();
        b.set_css("body { margin: 0; }");
        assert_eq!(b.css_rules.len(), 1);
    }

    #[test]
    fn test_set_style() {
        let mut b = setup();
        b.set_html(r#"<div id="box"></div>"#);
        b.set_style("#box", "color", "red");
        b.set_style("#box", "font-size", "16px");
        assert_eq!(b.styles.get("#box").unwrap().len(), 2);
    }

    #[test]
    fn test_clear_css() {
        let mut b = setup();
        b.set_css("body { margin: 0; }");
        b.set_style("#box", "color", "red");
        b.clear_css();
        assert!(b.css_rules.is_empty());
        assert!(b.styles.is_empty());
    }

    // ── 事件测试 ──

    #[test]
    fn test_on_click_and_handle_click() {
        let mut b = setup();
        b.set_html(r#"<div id="btn">Click</div>"#);
        let clicked = std::sync::Arc::new(std::sync::Mutex::new(false));
        let c = clicked.clone();
        b.on_click(
            "#btn",
            Box::new(move |x, y| {
                assert!((x - 100.0).abs() < 0.001);
                *c.lock().unwrap() = true;
            }),
        );
        assert!(b.handle_click(100.0, 200.0));
        assert!(*clicked.lock().unwrap());
        assert_eq!(b.click_log.len(), 1);
    }

    #[test]
    fn test_handle_click_unregistered() {
        let mut b = setup();
        b.set_html(r#"<div id="btn"></div>"#);
        assert!(!b.handle_click(50.0, 50.0));
    }

    #[test]
    fn test_on_form_submit() {
        let mut b = setup();
        b.set_html(r#"<form id="f"></form>"#);
        let submitted = std::sync::Arc::new(std::sync::Mutex::new(false));
        let s = submitted.clone();
        b.on_form_submit(
            "#f",
            Box::new(move |_| {
                *s.lock().unwrap() = true;
            }),
        );
        b.handle_form_submit("#f");
        assert!(*submitted.lock().unwrap());
        assert_eq!(b.form_log.len(), 1);
    }

    // ── 视口测试 ──

    #[test]
    fn test_viewport_default() {
        assert_eq!(setup().viewport(), (1280, 720));
    }

    #[test]
    fn test_set_viewport() {
        let mut b = setup();
        b.set_viewport(800, 600);
        assert_eq!(b.viewport(), (800, 600));
    }

    #[test]
    fn test_viewport_after_new() {
        assert_eq!(MockBridge::new(1920, 1080).viewport(), (1920, 1080));
    }

    // ── 渲染测试 ──

    #[test]
    fn test_render_returns_bytes() {
        let mut b = setup();
        let png = b.render();
        assert_eq!(&png[..4], &[0x89, 0x50, 0x4e, 0x47]);
        assert!(!png.is_empty());
    }

    // ── 综合流程测试 ──

    #[test]
    fn test_full_bridge_workflow() {
        let mut b = MockBridge::new(1024, 768);
        b.set_html(
            r#"<div id="header"><p id="text">Hello</p></div><button id="btn">Click</button>"#,
        );
        assert!(b.query("#header").is_some());
        assert!(b.query("#btn").is_some());
        b.set_css("body { margin: 0; }");
        b.set_style("#btn", "background", "blue");
        assert_eq!(b.css_rules.len(), 1);
        b.eval_js("console.log('test')");
        assert_eq!(b.js_log.len(), 1);
        let png = b.render();
        assert!(!png.is_empty());
        b.set_viewport(800, 600);
        assert_eq!(b.viewport(), (800, 600));
    }
}
