//! Web → Native 桥接层
//!
//! 统一封装 DOM 读写、JS 执行、样式修改、布局计算、事件绑定和渲染触发，
//! 供你的 web-to-native 工具产出的 Rust 代码直接调用。
//!
//! # 工作流
//!
//! ```ignore
//! let mut bridge = WebNativeBridge::new(1280, 720);
//!
//! // ① 写入 DOM
//! bridge.set_html(r#"<div id="app"><button id="btn">Click</button></div>"#);
//!
//! // ② 绑定事件
//! bridge.on_click("#btn", || {
//!     println!("按钮被点击了！");
//! });
//!
//! // ③ 执行 JS
//! bridge.eval_js("console.log('hello from Vite')");
//!
//! // ④ 渲染
//! bridge.render();
//!
//! // ⑤ Rust 侧修改 → 触发重新渲染
//! bridge.set_style("#btn", "background-color", "red");
//! bridge.render();
//!
//! // ⑥ 获取元素位置
//! let rect = bridge.get_rect("#btn");
//! ```

use crate::css::values::Color;
use crate::css_engine::{get_declaration, parse_inline_style, Declaration};
use crate::dom_wrapper::DomWrapper;
use crate::renderer::taffy_layout::{TaffyLayoutEngine, TaffyLayoutNode};
use crate::renderer::Renderer;
use std::collections::HashMap;

#[cfg(feature = "js")]
use crate::js_engine::JsEngine;

/// 事件处理器：Rust 闭包，接收点击位置的 (x, y)
pub type EventHandler = Box<dyn FnMut(f32, f32) + Send>;

/// 表单提交处理器：Rust 闭包，接收表单数据 (field_name → value)
pub type FormHandler = Box<dyn FnMut(HashMap<String, String>) + Send>;

/// Web → Native 桥接器
///
/// 串联 DOM / CSS / JS / 布局 / 渲染 / 事件，是 web-to-native 的核心入口。
pub struct WebNativeBridge {
    /// 当前 DOM
    dom: DomWrapper,
    /// 渲染器
    renderer: Renderer,
    /// 布局引擎（每次渲染后更新）
    layout: TaffyLayoutEngine,
    /// 视口尺寸
    width: u32,
    height: u32,
    /// 当前页面的原始 HTML（用于重新解析）
    html: String,
    /// 当前 URL
    url: String,
    /// 内联 <style> 累积内容（由 set_css / set_style 写入）
    inline_styles: String,

    // ── 事件绑定 ──
    /// 点击事件：CSS 选择器 → 处理器
    click_handlers: HashMap<String, EventHandler>,
    /// 表单提交：CSS 选择器 → 处理器
    form_handlers: HashMap<String, FormHandler>,

    #[cfg(feature = "js")]
    js_engine: JsEngine,
}

impl WebNativeBridge {
    /// 创建新的桥接器
    pub fn new(width: u32, height: u32) -> Self {
        let dom = DomWrapper::from_html(
            "<!DOCTYPE html><html><body></body></html>",
            Some("about:blank"),
        );
        let renderer = Renderer::new(width, height);
        let layout = TaffyLayoutEngine::new(width as f32, height as f32);

        Self {
            dom,
            renderer,
            layout,
            width,
            height,
            html: String::new(),
            url: "about:blank".to_string(),
            inline_styles: String::new(),
            click_handlers: HashMap::new(),
            form_handlers: HashMap::new(),
            #[cfg(feature = "js")]
            js_engine: JsEngine::new(),
        }
    }

    // ═══════════════════════════════════════════════════════════
    // ① DOM 读写
    // ═══════════════════════════════════════════════════════════

    /// 设置页面 HTML（替换当前 DOM）
    pub fn set_html(&mut self, html: &str) {
        self.html = html.to_string();
        self.dom = DomWrapper::from_html(html, Some(&self.url));

        #[cfg(feature = "js")]
        {
            let _ = self.js_engine.initialize(&self.url);
            self.js_engine.set_url(&self.url);
            let _ = self.js_engine.evaluate("document.body.innerHTML = ''");
        }
    }

    /// 获取 DOM 包装器引用
    pub fn dom(&self) -> &DomWrapper {
        &self.dom
    }

    /// 获取 DOM 包装器可变引用（修改后需要调 render() 刷新）
    pub fn dom_mut(&mut self) -> &mut DomWrapper {
        &mut self.dom
    }

    /// 按 CSS 选择器获取元素，返回 DOM 节点 ID
    pub fn query(&self, selector: &str) -> Option<usize> {
        self.dom.select_first(selector)
    }

    /// 按 CSS 选择器获取所有匹配元素
    pub fn query_all(&self, selector: &str) -> Vec<usize> {
        self.dom.select(selector)
    }

    /// 获取元素标签名
    pub fn tag_name(&self, node_id: usize) -> Option<String> {
        self.dom.tag_name(node_id)
    }

    /// 获取元素属性
    pub fn get_attr(&self, node_id: usize, name: &str) -> Option<String> {
        self.dom.attribute(node_id, name)
    }

    /// 设置元素属性（修改后需调 render()）
    pub fn set_attr(&mut self, node_id: usize, name: &str, value: &str) {
        self.dom.set_attribute(node_id, name, value);
    }

    /// 获取元素文本内容
    pub fn text(&self, node_id: usize) -> Option<String> {
        self.dom.text_content(node_id)
    }

    /// 按选择器获取元素文本
    pub fn query_text(&self, selector: &str) -> Option<String> {
        let id = self.query(selector)?;
        self.text(id)
    }

    /// 获取元素在页面上的矩形位置（像素）
    pub fn get_rect(&self, selector: &str) -> Option<(f32, f32, f32, f32)> {
        let id = self.query(selector)?;
        self.layout.get_layout_rect(id)
    }

    /// 获取所有布局节点的位置信息
    pub fn all_rects(&self) -> Vec<(usize, String, f32, f32, f32, f32)> {
        self.layout
            .get_all_layout_nodes()
            .into_iter()
            .map(|n| (n.dom_node, n.tag_name.clone(), n.x, n.y, n.width, n.height))
            .collect()
    }

    /// 点击测试：返回点击位置下的布局节点
    pub fn hit_test(&self, x: f32, y: f32) -> Option<&TaffyLayoutNode> {
        self.layout.hit_test(x, y)
    }

    // ═══════════════════════════════════════════════════════════
    // ② CSS / 样式操作
    // ═══════════════════════════════════════════════════════════

    /// 添加 CSS 规则到页面
    ///
    /// 等价于在 `<style>` 标签中写入 CSS。
    /// 写入后需要调 render() 才会生效。
    pub fn set_css(&mut self, css_text: &str) {
        if !self.inline_styles.is_empty() {
            self.inline_styles.push('\n');
        }
        self.inline_styles.push_str(css_text);
    }

    /// 给元素设置内联 style 属性
    ///
    /// 等价于 `element.style.property = value`。
    /// 写入后需要调 render()。
    pub fn set_style(&mut self, selector: &str, property: &str, value: &str) {
        if let Some(id) = self.query(selector) {
            let existing = self.dom.attribute(id, "style").unwrap_or_default();
            let mut decls = parse_inline_style(&existing);
            // 移除同名的旧声明
            decls.retain(|d| d.property != property);
            decls.push(Declaration {
                property: property.to_string(),
                value: value.to_string(),
            });
            let new_style = decls
                .iter()
                .map(|d| format!("{}: {}", d.property, d.value))
                .collect::<Vec<_>>()
                .join("; ");
            self.dom.set_attribute(id, "style", &new_style);
        }
    }

    /// 清除所有自定义 CSS
    pub fn clear_css(&mut self) {
        self.inline_styles.clear();
    }

    // ═══════════════════════════════════════════════════════════
    // ③ JS 执行
    // ═══════════════════════════════════════════════════════════

    /// 执行 JavaScript 代码
    ///
    /// 需要 `--features js` 编译。不带 feature 时返回 `"undefined"`.
    pub fn eval_js(&mut self, code: &str) -> String {
        #[cfg(feature = "js")]
        {
            if !self.js_engine.is_ready() {
                let _ = self.js_engine.initialize(&self.url);
            }
            self.js_engine
                .evaluate(code)
                .unwrap_or_else(|e| format!("JS Error: {}", e))
        }
        #[cfg(not(feature = "js"))]
        {
            let _ = code;
            "undefined".to_string()
        }
    }

    // ═══════════════════════════════════════════════════════════
    // ④ 渲染
    // ═══════════════════════════════════════════════════════════

    /// 渲染当前 DOM + CSS + 布局结果，返回 PNG 字节
    pub fn render(&mut self) -> Vec<u8> {
        // 1. 如果有累积的内联 CSS，注入到 <head> 中
        if !self.inline_styles.is_empty() {
            // 检测是否有 <style> 标签，没有则创建
            let has_style = self.dom.select("style").len() > 0
                || self.dom.select_first("style").is_some()
                || self.dom.select("head style").len() > 0;
            // 通过 DomWrapper 无法直接创建 DOM 节点，
            // 这里用重新拼接 HTML + 注入 <style> 的方式
            let head_close = "</head>";
            let style_tag = format!("<style>{}</style>", self.inline_styles);
            if let Some(pos) = self.html.find(head_close) {
                let new_html = format!("{}{}{}", &self.html[..pos], style_tag, &self.html[pos..]);
                self.html = new_html;
                self.dom = DomWrapper::from_html(&self.html, Some(&self.url));
            }
            self.inline_styles.clear();
        }

        // 2. 执行 JS（如果有）— 这里假设 JS 已经通过 eval_js 执行过了

        // 3. 构建文档
        let doc = crate::browser::Document::from_html(&self.html, &self.url);

        // 4. 提取 CSS 并计算布局
        let doc_ref = self.dom.inner_document();
        let all_css = {
            let mut css = String::new();
            // <style> 标签
            let style_text = crate::renderer::extract_style_tags(&self.dom);
            css.push_str(&style_text);
            css
        };

        let rules = crate::css_engine::parse_css_rules(&all_css);
        let style_map = crate::css_engine::rules_to_style_map(&rules, doc_ref);

        self.layout = TaffyLayoutEngine::new(self.width as f32, self.height as f32);
        self.layout.set_style_map(style_map);
        let _ = self.layout.compute(&self.dom);

        // 5. 渲染
        let png = self
            .renderer
            .render_with_taffy(&self.dom, &self.layout)
            .unwrap_or_default();

        png
    }

    // ═══════════════════════════════════════════════════════════
    // ⑤ 事件绑定
    // ═══════════════════════════════════════════════════════════

    /// 绑定点击事件
    ///
    /// 命中 `selector` 的元素时调用 `handler(x, y)`。
    pub fn on_click(&mut self, selector: &str, handler: EventHandler) {
        self.click_handlers.insert(selector.to_string(), handler);
    }

    /// 绑定表单提交事件
    ///
    /// 命中 `selector` 的 `<form>` 提交时调用 `handler(field_map)`。
    pub fn on_form_submit(&mut self, selector: &str, handler: FormHandler) {
        self.form_handlers.insert(selector.to_string(), handler);
    }

    /// 处理鼠标点击（由你的原生窗口调用）
    ///
    /// 按命中测试触发已注册的事件处理器。
    /// 返回 `true` 表示事件被消费。
    pub fn handle_click(&mut self, x: f32, y: f32) -> bool {
        // 点击测试
        if let Some(node) = self.layout.hit_test(x, y) {
            // 检查已注册的点击处理器
            for (selector, handler) in self.click_handlers.iter_mut() {
                if self
                    .dom
                    .select_first(selector)
                    .map(|id| id == node.dom_node)
                    .unwrap_or(false)
                {
                    handler(x, y);
                    return true;
                }
            }

            // 如果是 <a> 标签，返回 false 让调用方处理导航
            if node.tag_name == "a" {
                return false;
            }
        }
        false
    }

    /// 处理表单提交（由你的代码在其他逻辑中调用）
    ///
    /// 收集表单字段值并调用已注册的处理器。
    pub fn handle_form_submit(&mut self, form_selector: &str) {
        let form_id = match self.query(form_selector) {
            Some(id) => id,
            None => return,
        };

        // 收集表单字段
        let mut fields = HashMap::new();
        self.collect_form_fields(form_id, &mut fields);

        // 克隆 key 以避免借用冲突
        let keys: Vec<String> = self.form_handlers.keys().cloned().collect();
        for key in &keys {
            if self.query(key) == Some(form_id) {
                if let Some(handler) = self.form_handlers.get_mut(key) {
                    handler(fields.clone());
                }
            }
        }
    }

    fn collect_form_fields(&self, node_id: usize, fields: &mut HashMap<String, String>) {
        // 遍历子节点，收集 input/select/textarea
        for child_id in self.dom.children(node_id) {
            if let Some(tag) = self.dom.tag_name(child_id) {
                match tag.as_str() {
                    "input" => {
                        let name = self.dom.attribute(child_id, "name");
                        let value = self.dom.attribute(child_id, "value").unwrap_or_default();
                        if let Some(n) = name {
                            fields.insert(n, value);
                        }
                    }
                    "textarea" | "select" => {
                        let name = self.dom.attribute(child_id, "name");
                        let value = self.dom.text_content_recursive(child_id);
                        if let Some(n) = name {
                            fields.insert(n, value.trim().to_string());
                        }
                    }
                    _ => {
                        // 递归
                        self.collect_form_fields(child_id, fields);
                    }
                }
            }
        }
    }

    // ═══════════════════════════════════════════════════════════
    // ⑥ 工具
    // ═══════════════════════════════════════════════════════════

    /// 获取渲染器引用（用于高级操作）
    pub fn renderer(&self) -> &Renderer {
        &self.renderer
    }

    /// 获取布局引擎引用
    pub fn layout(&self) -> &TaffyLayoutEngine {
        &self.layout
    }

    /// 设置视口尺寸
    pub fn set_viewport(&mut self, width: u32, height: u32) {
        self.width = width;
        self.height = height;
        self.renderer.set_viewport(width, height);
        self.layout.set_viewport(width as f32, height as f32);
    }

    /// 获取视口尺寸
    pub fn viewport(&self) -> (u32, u32) {
        (self.width, self.height)
    }
}

impl Default for WebNativeBridge {
    fn default() -> Self {
        Self::new(1280, 720)
    }
}
