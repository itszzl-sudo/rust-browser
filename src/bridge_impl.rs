//! WebNativeBridge 默认实现 —— 基于 rust-browser 渲染引擎
//!
//! 实现 [`bridge::WebNativeBridge`] trait，接入 kuchiki DOM、
//! Taffy 布局、tiny-skia 渲染和 Boa/obscura-js JS 引擎。

use std::collections::HashMap;

use crate::bridge::{
    self, Color, Declaration, EventHandler, FormHandler, LayoutNode, LayoutRect, WebNativeBridge,
};
use crate::css::values::Color as CssColor;
use crate::css_engine::{get_declaration, parse_inline_style, Declaration as CssDeclaration};
use crate::dom_wrapper::DomWrapper;
use crate::renderer::taffy_layout::{TaffyLayoutEngine, TaffyLayoutNode};
use crate::renderer::Renderer;

#[cfg(any(feature = "js", feature = "boa"))]
use crate::js_engine::JsEngine;

/// Web → Native 桥接器默认实现
///
/// 串联 DOM / CSS / JS / 布局 / 渲染 / 事件。
pub struct DefaultWebNativeBridge {
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

    #[cfg(any(feature = "js", feature = "boa"))]
    js_engine: JsEngine,
}

impl DefaultWebNativeBridge {
    fn css_color_to_bridge(c: &CssColor) -> Color {
        let rgba = c.to_rgba();
        Color::from_rgba(rgba[0], rgba[1], rgba[2], rgba[3])
    }

    fn layout_node_to_bridge(n: &TaffyLayoutNode) -> LayoutNode {
        LayoutNode {
            dom_node: n.dom_node,
            tag_name: n.tag_name.clone(),
            x: n.x,
            y: n.y,
            width: n.width,
            height: n.height,
            background: n.background.as_ref().map(Self::css_color_to_bridge),
        }
    }
}

impl WebNativeBridge for DefaultWebNativeBridge {
    fn new(width: u32, height: u32) -> Self {
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
            #[cfg(any(feature = "js", feature = "boa"))]
            js_engine: JsEngine::new(),
        }
    }

    // ── DOM 读写 ──

    fn set_html(&mut self, html: &str) {
        self.html = html.to_string();
        self.dom = DomWrapper::from_html(html, Some(&self.url));

        #[cfg(any(feature = "js", feature = "boa"))]
        {
            let _ = self.js_engine.initialize(&self.url);
            self.js_engine.set_url(&self.url);
            let _ = self.js_engine.evaluate("document.body.innerHTML = ''");
        }
    }

    fn query(&self, selector: &str) -> Option<usize> {
        self.dom.select_first(selector)
    }

    fn query_all(&self, selector: &str) -> Vec<usize> {
        self.dom.select(selector)
    }

    fn tag_name(&self, node_id: usize) -> Option<String> {
        self.dom.tag_name(node_id)
    }

    fn get_attr(&self, node_id: usize, name: &str) -> Option<String> {
        self.dom.attribute(node_id, name)
    }

    fn set_attr(&mut self, node_id: usize, name: &str, value: &str) {
        self.dom.set_attribute(node_id, name, value);
    }

    fn text(&self, node_id: usize) -> Option<String> {
        self.dom.text_content(node_id)
    }

    fn parent_node(&self, node_id: usize) -> Option<usize> {
        self.dom.parent(node_id)
    }

    // ── 布局 ──

    fn get_rect(&self, selector: &str) -> Option<LayoutRect> {
        let id = self.query(selector)?;
        self.layout
            .get_layout_rect(id)
            .map(|(x, y, w, h)| LayoutRect {
                x,
                y,
                width: w,
                height: h,
            })
    }

    fn all_rects(&self) -> Vec<LayoutNode> {
        self.layout
            .get_all_layout_nodes()
            .into_iter()
            .map(|n| Self::layout_node_to_bridge(n))
            .collect()
    }

    fn hit_test(&self, x: f32, y: f32) -> Option<LayoutNode> {
        self.layout
            .hit_test(x, y)
            .map(|n| Self::layout_node_to_bridge(n))
    }

    // ── CSS 操作 ──

    fn set_css(&mut self, css_text: &str) {
        if !self.inline_styles.is_empty() {
            self.inline_styles.push('\n');
        }
        self.inline_styles.push_str(css_text);
    }

    fn set_style(&mut self, selector: &str, property: &str, value: &str) {
        if let Some(id) = self.query(selector) {
            let existing = self.dom.attribute(id, "style").unwrap_or_default();
            let mut decls = parse_inline_style(&existing);
            decls.retain(|d| d.property != property);
            decls.push(CssDeclaration {
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

    fn clear_css(&mut self) {
        self.inline_styles.clear();
    }

    // ── JS 执行 ──

    fn eval_js(&mut self, code: &str) -> String {
        #[cfg(any(feature = "js", feature = "boa"))]
        {
            if !self.js_engine.is_ready() {
                let _ = self.js_engine.initialize(&self.url);
            }
            self.js_engine
                .evaluate(code)
                .unwrap_or_else(|e| format!("JS Error: {}", e))
        }
        #[cfg(not(any(feature = "js", feature = "boa")))]
        {
            let _ = code;
            "undefined".to_string()
        }
    }

    // ── 渲染 ──

    fn render(&mut self) -> Vec<u8> {
        // 1. 如果有累积的内联 CSS，注入到 <head> 中
        if !self.inline_styles.is_empty() {
            let head_close = "</head>";
            let style_tag = format!("<style>{}</style>", self.inline_styles);
            if let Some(pos) = self.html.find(head_close) {
                let new_html = format!("{}{}{}", &self.html[..pos], style_tag, &self.html[pos..]);
                self.html = new_html;
                self.dom = DomWrapper::from_html(&self.html, Some(&self.url));
            }
            self.inline_styles.clear();
        }

        // 2. 提取 CSS 并计算布局
        let doc_ref = self.dom.inner_document();
        let all_css = {
            let style_text = crate::renderer::extract_style_tags(&self.dom);
            style_text
        };

        let rules = crate::css_engine::parse_css_rules(&all_css);
        let style_map = crate::css_engine::rules_to_style_map(&rules, doc_ref);

        self.layout = TaffyLayoutEngine::new(self.width as f32, self.height as f32);
        self.layout.set_style_map(style_map);
        let _ = self.layout.compute(&self.dom);

        // 3. 渲染
        self.renderer
            .render_with_taffy(&self.dom, &self.layout)
            .unwrap_or_default()
    }

    // ── 事件绑定 ──

    fn on_click(&mut self, selector: &str, handler: EventHandler) {
        self.click_handlers.insert(selector.to_string(), handler);
    }

    fn on_form_submit(&mut self, selector: &str, handler: FormHandler) {
        self.form_handlers.insert(selector.to_string(), handler);
    }

    fn handle_click(&mut self, x: f32, y: f32) -> bool {
        let hit_node = match self.layout.hit_test(x, y) {
            Some(node) => node.dom_node,
            None => return false,
        };

        // 向上遍历 DOM 祖先链
        fn collect_ancestors(dom: &DomWrapper, start: usize) -> Vec<usize> {
            let mut chain = Vec::new();
            let mut current = Some(start);
            while let Some(id) = current {
                chain.push(id);
                current = dom.parent(id);
            }
            chain
        }
        let ancestor_chain = collect_ancestors(&self.dom, hit_node);

        for (selector, handler) in self.click_handlers.iter_mut() {
            if let Some(sel_id) = self.dom.select_first(selector) {
                if ancestor_chain.contains(&sel_id) {
                    handler(x, y);
                    return true;
                }
            }
        }

        if let Some(node) = self.layout.hit_test(x, y) {
            if node.tag_name == "a" {
                return false;
            }
        }

        false
    }

    fn handle_form_submit(&mut self, form_selector: &str) {
        let form_id = match self.query(form_selector) {
            Some(id) => id,
            None => return,
        };

        let mut fields = HashMap::new();
        self.collect_form_fields(form_id, &mut fields);

        let keys: Vec<String> = self.form_handlers.keys().cloned().collect();
        for key in &keys {
            if self.query(key) == Some(form_id) {
                if let Some(handler) = self.form_handlers.get_mut(key) {
                    handler(fields.clone());
                }
            }
        }
    }

    // ── 工具 ──

    fn set_viewport(&mut self, width: u32, height: u32) {
        self.width = width;
        self.height = height;
        self.renderer.set_viewport(width, height);
        self.layout.set_viewport(width as f32, height as f32);
    }

    fn viewport(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    // ── 网络请求 ──

    fn navigate(&mut self, url: &str) -> Result<(), String> {
        use crate::network::NetworkClient;
        
        let client = NetworkClient::new();
        let response = client.get(url)
            .map_err(|e| format!("Network error: {}", e))?;
        
        // 更新 URL
        self.url = response.final_url.clone();
        
        // 解析 HTML
        let html = String::from_utf8_lossy(&response.body).to_string();
        self.set_html(&html);
        
        Ok(())
    }

    fn current_url(&self) -> String {
        self.url.clone()
    }

    fn http_get(&mut self, url: &str) -> Result<crate::network::HttpResponse, String> {
        use crate::network::NetworkClient;
        
        let client = NetworkClient::new();
        client.get(url).map_err(|e| format!("HTTP GET error: {}", e))
    }

    fn http_post(&mut self, url: &str, body: &[u8], content_type: &str) -> Result<crate::network::HttpResponse, String> {
        use crate::network::NetworkClient;
        
        let client = NetworkClient::new();
        client.post(url, body, content_type)
            .map_err(|e| format!("HTTP POST error: {}", e))
    }

    // ── 文件操作 ──

    fn download_file(&mut self, url: &str, path: &str) -> Result<u64, String> {
        use crate::network::NetworkClient;

        let client = NetworkClient::new();
        let response = client.get(url)
            .map_err(|e| format!("Download error: {}", e))?;

        std::fs::write(path, &response.body)
            .map_err(|e| format!("Write file error: {}", e))?;

        Ok(response.body.len() as u64)
    }

    fn write_file(&mut self, path: &str, data: &[u8]) -> Result<(), String> {
        std::fs::write(path, data)
            .map_err(|e| format!("Write file error: {}", e))
    }

    fn read_file(&mut self, path: &str) -> Result<Vec<u8>, String> {
        std::fs::read(path)
            .map_err(|e| format!("Read file error: {}", e))
    }
}

impl DefaultWebNativeBridge {
    fn collect_form_fields(&self, node_id: usize, fields: &mut HashMap<String, String>) {
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
                        self.collect_form_fields(child_id, fields);
                    }
                }
            }
        }
    }
}

impl Default for DefaultWebNativeBridge {
    fn default() -> Self {
        Self::new(1280, 720)
    }
}
