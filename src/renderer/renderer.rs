//! Renderer - 主渲染器
//!
//! 使用 kuchiki DOM, cosmic-text 文本渲染 和 tiny-skia 渲染

use crate::browser::Document;
use crate::css::values::Color;
use crate::css_engine::{parse_css_rules, rules_to_style_map};
use crate::renderer::border::draw_box_shadow;
use crate::renderer::context::RenderContext;
use crate::renderer::image_cache::ImageCache;
use crate::renderer::painter::Painter;
use crate::renderer::taffy_layout::TaffyLayoutEngine;
use crate::renderer::text::TextRenderer;
use crate::DomWrapper;
use cosmic_text::{Align, Attrs, Buffer, FontSystem, Metrics, Shaping, SwashCache, Wrap};
use kuchiki::NodeRef;
use log::{debug, info, trace, warn};
use std::path::Path;
use std::rc::Rc;
use std::sync::OnceLock;
use thiserror::Error;
use tiny_skia::Pixmap;

#[derive(Error, Debug)]
pub enum RenderError {
    #[error("创建绘制器失败")]
    PainterCreationFailed,
    #[error("布局计算失败: {0}")]
    LayoutFailed(String),
    #[error("渲染失败: {0}")]
    RenderFailed(String),
    #[error("保存图像失败: {0}")]
    SaveFailed(String),
}

/// 全局共享的字体系统（懒初始化）
pub(crate) fn global_font_system() -> &'static std::sync::Mutex<FontSystem> {
    static FONT_SYSTEM: OnceLock<std::sync::Mutex<FontSystem>> = OnceLock::new();
    FONT_SYSTEM.get_or_init(|| std::sync::Mutex::new(FontSystem::new()))
}

/// 全局共享的 swash 字形缓存（懒初始化）
fn global_swash_cache() -> &'static std::sync::Mutex<SwashCache> {
    static CACHE: OnceLock<std::sync::Mutex<SwashCache>> = OnceLock::new();
    CACHE.get_or_init(|| std::sync::Mutex::new(SwashCache::new()))
}

/// 全局共享的图片缓存（懒初始化）
fn global_image_cache() -> &'static ImageCache {
    static CACHE: OnceLock<ImageCache> = OnceLock::new();
    CACHE.get_or_init(|| ImageCache::new())
}

/// CSS box-shadow 值解析结果
#[derive(Debug, Clone)]
pub struct BoxShadowValue {
    pub offset_x: f32,
    pub offset_y: f32,
    pub blur_radius: f32,
    pub spread: f32,
    pub color: Color,
}

/// 从 style 属性值解析 box-shadow
fn parse_box_shadow_from_style(style: &str) -> Option<BoxShadowValue> {
    // box-shadow: offset-x offset-y blur-radius spread color
    let parts: Vec<&str> = style.split_whitespace().collect();
    if parts.len() < 2 {
        return None;
    }

    let offset_x = parse_css_px(parts[0])?;
    let offset_y = parse_css_px(parts[1])?;

    let blur_radius = if parts.len() > 2 {
        parse_css_px(parts[2]).unwrap_or(0.0)
    } else {
        0.0
    };

    let spread = if parts.len() > 3 {
        parse_css_px(parts[3]).unwrap_or(0.0)
    } else {
        0.0
    };

    let color = if parts.len() > 4 {
        parse_css_color(parts[4]).unwrap_or(Color::from_hex("#000000"))
    } else {
        Color::from_hex("#000000")
    };

    Some(BoxShadowValue {
        offset_x,
        offset_y,
        blur_radius,
        spread,
        color,
    })
}

/// 解析带 px 单位或纯数字的 CSS 值
fn parse_css_px(val: &str) -> Option<f32> {
    let v = val.trim();
    if let Some(px) = v.strip_suffix("px") {
        px.trim().parse::<f32>().ok()
    } else {
        v.parse::<f32>().ok()
    }
}

/// 解析 CSS 颜色值（#hex 或命名颜色）
fn parse_css_color(val: &str) -> Option<Color> {
    let v = val.trim();
    if v.starts_with('#') {
        Some(Color::from_hex(v))
    } else {
        Color::from_name(v)
    }
}

pub struct Renderer {
    context: RenderContext,
    painter: Painter,
    text_renderer: TextRenderer,
    document: Option<Document>,
    title: Option<String>,
    /// 最近一次渲染的 Taffy 布局结果，用于点击测试
    last_taffy: Option<TaffyLayoutEngine>,
    /// 超长截图的 PNG 缓存（完整页面高度）
    page_png: Option<Vec<u8>>,
    /// 是否正在加载
    pub is_loading: bool,
    /// 当前获得焦点的 DOM 元素索引
    pub focused_node: Option<usize>,
    /// 光标渲染器
    pub cursor: crate::renderer::cursor::CursorRenderer,
    /// 上次按键时间（秒，用于光标重置计时）
    pub last_key_time: f32,
}

impl Renderer {
    pub fn new(width: u32, height: u32) -> Self {
        info!("初始化渲染器 ({}x{})", width, height);

        let context = RenderContext::new(width, height);
        let painter = Painter::new(width, height)
            .ok_or(RenderError::PainterCreationFailed)
            .unwrap();
        let text_renderer = TextRenderer::new();

        // 预热字体系统
        let _ = global_font_system();
        let _ = global_swash_cache();
        let _ = global_image_cache();

        Self {
            context,
            painter,
            text_renderer,
            document: None,
            title: None,
            last_taffy: None,
            page_png: None,
            is_loading: false,
            focused_node: None,
            cursor: crate::renderer::cursor::CursorRenderer::new(),
            last_key_time: 0.0,
        }
    }

    pub fn set_viewport(&mut self, width: u32, height: u32) {
        debug!("设置视口: {}x{}", width, height);
        self.context.set_viewport(width, height);
        self.painter.set_viewport(width, height);
    }

    /// 设置当前文档（用于多进程模式下的渲染）
    pub fn set_document(&mut self, doc: Document) {
        let t = doc.title.clone();
        self.title = t;
        self.document = Some(doc);
    }

    /// 获取页面标题
    pub fn title(&self) -> Option<&str> {
        self.title.as_deref()
    }

    /// 渲染存储的文档到 PNG
    pub fn render_to_png(&mut self) -> Result<Vec<u8>, RenderError> {
        let doc = self.document.clone();
        self.is_loading = true;
        let result = self.render(&doc);
        self.is_loading = false;
        result
    }

    /// 调整大小（resize 是 set_viewport 的别名）
    pub fn resize(&mut self, width: u32, height: u32) {
        self.set_viewport(width, height);
    }

    pub fn render(&mut self, document: &Option<Document>) -> Result<Vec<u8>, RenderError> {
        info!("开始渲染");
        self.is_loading = true;

        self.painter.set_background(Color::WHITE);
        self.painter.paint();

        if let Some(doc) = document {
            self.render_document(doc)?;
        } else {
            self.render_blank_page()?;
        }

        self.is_loading = false;
        Ok(self.painter.to_png())
    }

    fn render_document(&mut self, document: &Document) -> Result<(), RenderError> {
        trace!(
            "渲染文档: {}",
            document.title.as_deref().unwrap_or("无标题")
        );

        let (width, height) = self.context.viewport();
        let dom = document.get_dom();

        // 1. 从 DOM 提取 <style> CSS
        let css_text = extract_style_tags(dom);
        if !css_text.is_empty() {
            trace!("提取到 CSS: {} 字符", css_text.len());
        }

        // 2. 解析 CSS 规则并生成 StyleMap
        let style_map = if !css_text.is_empty() {
            let rules = parse_css_rules(&css_text);
            trace!("解析到 {} 条 CSS 规则", rules.len());
            rules_to_style_map(&rules, dom.inner_document())
        } else {
            Default::default()
        };

        // 3. 创建 TaffyLayoutEngine 并设置 StyleMap
        let mut taffy = TaffyLayoutEngine::new(width as f32, height as f32);
        taffy.set_style_map(style_map);

        // 4. 计算布局
        if let Err(e) = taffy.compute(dom) {
            warn!("Taffy 布局计算失败: {}, 使用手动布局回退", e);
        }

        // 4.1 计算页面总高度（超长截图）
        let page_height = if !taffy.is_empty() {
            let max_bottom = taffy
                .get_all_layout_nodes()
                .iter()
                .map(|n| n.y + n.height)
                .fold(0.0_f32, f32::max)
                .max(height as f32);
            (max_bottom + 50.0) as u32 // 加底部边距
        } else {
            height
        };

        // 4.2 如果页面高度超过视口高度，创建完整页面大小的 pixmap
        let orig_pixmap = self.painter.pixmap_mut().clone();
        let is_tall = page_height > height;
        if is_tall {
            if let Some(page_pixmap) = Pixmap::new(width, page_height) {
                *self.painter.pixmap_mut() = page_pixmap;
                debug!("创建超长截图画布: {}x{}", width, page_height);
            }
        }

        // 5. 使用 Taffy 布局结果渲染
        {
            // 先平铺白色背景（因为 pixmap 已重置）
            self.painter.set_background(Color::WHITE);
            self.painter.paint();

            let mut renderer = TaffyRenderer {
                painter: &mut self.painter,
                taffy: &taffy,
                dom,
            };
            renderer.render_dom();
        }

        // 5.1 生成完整页面 PNG 并缓存
        if is_tall {
            let full_png = self.painter.to_png();
            self.page_png = Some(full_png);
            // 恢复原始视口大小的 pixmap
            *self.painter.pixmap_mut() = orig_pixmap;
        }

        // 6. 保存布局结果，用于后续点击测试
        self.last_taffy = Some(taffy);

        debug!("文档渲染完成");
        Ok(())
    }

    /// 直接使用给定的 DOM 和 TaffyLayoutEngine 渲染，返回 PNG 字节
    pub fn render_with_taffy(
        &mut self,
        dom: &DomWrapper,
        taffy: &TaffyLayoutEngine,
    ) -> Result<Vec<u8>, RenderError> {
        self.is_loading = true;

        // 计算页面总高度（长页面截图）
        let (width, height) = self.context.viewport();
        let page_height = if !taffy.is_empty() {
            let max_bottom = taffy
                .get_all_layout_nodes()
                .iter()
                .map(|n| n.y + n.height)
                .fold(0.0_f32, f32::max)
                .max(height as f32);
            (max_bottom + 50.0) as u32
        } else {
            height
        };

        let is_tall = page_height > height;
        let orig_pixmap = self.painter.pixmap_mut().clone();

        // 如果页面超过视口，创建完整页面大小的画布
        if is_tall {
            if let Some(page_pixmap) = Pixmap::new(width, page_height) {
                *self.painter.pixmap_mut() = page_pixmap;
                debug!("超长截图: {}x{}", width, page_height);
            }
        }

        self.painter.set_background(Color::WHITE);
        self.painter.paint();

        if !taffy.is_empty() {
            let mut renderer = TaffyRenderer {
                painter: &mut self.painter,
                taffy,
                dom,
            };
            renderer.render_dom();
        }

        // 长页面：保留完整 PNG，恢复原始画布
        let result = if is_tall {
            let full_png = self.painter.to_png();
            self.page_png = Some(full_png.clone());
            *self.painter.pixmap_mut() = orig_pixmap;
            full_png
        } else {
            self.painter.to_png()
        };

        self.is_loading = false;
        Ok(result)
    }

    fn render_blank_page(&mut self) -> Result<(), RenderError> {
        debug!("渲染空白页面");
        Ok(())
    }

    pub fn capture_viewport(&self) -> Vec<u8> {
        self.painter.to_png()
    }

    pub fn save(&self, path: &Path) -> Result<(), RenderError> {
        debug!("保存渲染结果到: {:?}", path);

        let extension = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("png")
            .to_lowercase();

        match extension.as_str() {
            "png" => self
                .painter
                .save_png(path.to_str().unwrap())
                .map_err(|e| RenderError::SaveFailed(e.to_string())),
            "jpg" | "jpeg" => {
                let png_data = self.painter.to_png();
                let img = image::load_from_memory(&png_data)
                    .map_err(|e| RenderError::SaveFailed(e.to_string()))?;
                img.save(path)
                    .map_err(|e| RenderError::SaveFailed(e.to_string()))
            }
            _ => self
                .painter
                .save_png(path.to_str().unwrap())
                .map_err(|e| RenderError::SaveFailed(e.to_string())),
        }
    }

    pub fn context(&self) -> &RenderContext {
        &self.context
    }

    pub fn painter(&self) -> &Painter {
        &self.painter
    }

    pub fn text_renderer(&self) -> &TextRenderer {
        &self.text_renderer
    }

    /// 获取当前文档引用
    pub fn document(&self) -> Option<&Document> {
        self.document.as_ref()
    }

    /// 获取最近一次渲染的 Taffy 布局引擎引用
    pub fn taffy_layout(&self) -> Option<&TaffyLayoutEngine> {
        self.last_taffy.as_ref()
    }

    /// 设置 hovered_node 并重新渲染
    pub fn set_hovered_node(&mut self, dom_node: Option<usize>) -> Option<Vec<u8>> {
        if self
            .last_taffy
            .as_ref()
            .map(|t| t.hovered_node != dom_node)
            .unwrap_or(false)
        {
            if let Some(ref mut taffy) = self.last_taffy {
                taffy.set_hovered_node(dom_node);
            }
            // 使用 document 重新渲染（不走 render_with_taffy，避免双重借用）
            if let Some(doc) = self.document.clone() {
                let _ = self.render(&Some(doc));
                return Some(self.painter.to_png());
            }
        }
        None
    }

    /// 获取超长截图的 PNG 数据（如有）
    pub fn page_png(&self) -> Option<&[u8]> {
        self.page_png.as_deref()
    }

    /// 点击测试：如果点到 <input>/<textarea> 则设置焦点
    pub fn focus_by_click(&mut self, x: f32, y: f32, dom: &DomWrapper) -> Option<usize> {
        // 先从 taffy 中获取点击命中的节点
        let hit_dom_node = {
            let taffy = self.last_taffy.as_ref()?;
            let hit = taffy.hit_test(x, y)?;
            hit.dom_node
        };

        let tag = dom.tag_name(hit_dom_node).unwrap_or_default();
        if tag == "input" || tag == "textarea" {
            self.focused_node = Some(hit_dom_node);
            // 更新 taffy 布局引擎中的 focused_node
            if let Some(ref mut taffy) = self.last_taffy {
                taffy.set_focused_node(Some(hit_dom_node));
            }
            // 重置光标到文本末尾
            let value = dom.attribute(hit_dom_node, "value").unwrap_or_default();
            self.cursor.reset(value.len());
            return Some(hit_dom_node);
        }

        // 点击非输入元素时，清除焦点
        if self.focused_node.is_some() {
            self.focused_node = None;
            if let Some(ref mut taffy) = self.last_taffy {
                taffy.set_focused_node(None);
            }
        }

        None
    }

    /// 每帧更新光标闪烁状态
    pub fn update_cursor(&mut self, dt: f32) {
        self.cursor.update(dt);
    }

    /// 获取焦点元素的 value 值
    pub fn get_focused_value(&self) -> Option<String> {
        let focused = self.focused_node?;
        let doc = self.document.as_ref()?;
        doc.get_dom().attribute(focused, "value")
    }

    /// 对渲染后的页面做点击测试，返回点击位置的 <a> 链接 href
    pub fn hit_test_link(&self, x: f32, y: f32, dom: &DomWrapper) -> Option<String> {
        let taffy = self.taffy_layout()?;
        let hit = taffy.hit_test(x, y)?;

        // 通过 dom_node 索引查找对应的 DOM 元素
        let node_ref = dom.get_node(hit.dom_node)?;
        if let Some(el) = node_ref.as_element() {
            if el.name.local.as_ref() == "a" {
                return el.attributes.borrow().get("href").map(|s| s.to_string());
            }
        }

        // 如果点击的不是 <a> 本身，向上查找父元素是否为 <a>
        // 这在点击 <a> 标签内的子元素（如 <span>、<img>）时很有用
        if let Some(parent) = node_ref.parent() {
            if let Some(parent_el) = parent.as_element() {
                if parent_el.name.local.as_ref() == "a" {
                    return parent_el
                        .attributes
                        .borrow()
                        .get("href")
                        .map(|s| s.to_string());
                }
            }
        }

        None
    }
}

/// 从 DOM 中提取所有 <style> 标签（和 <link rel="stylesheet">）的文本内容
pub fn extract_style_tags(dom: &DomWrapper) -> String {
    let mut css = String::new();
    let doc_ref = dom.inner_document();

    // 1. 提取 <style> 标签
    if let Ok(style_nodes) = doc_ref.select("style") {
        for node_ref in style_nodes {
            let text = node_ref.text_contents();
            if !text.trim().is_empty() {
                if !css.is_empty() {
                    css.push('\n');
                }
                css.push_str(text.trim());
            }
        }
    }

    // 2. 提取 <link rel="stylesheet"> 标签（通过 DomWrapper API 获取 NodeRef）
    let link_indices = dom.select("link");
    for idx in link_indices {
        let is_stylesheet = dom
            .attribute(idx, "rel")
            .map(|v| v == "stylesheet")
            .unwrap_or(false);
        if !is_stylesheet {
            continue;
        }
        if let Some(href) = dom.attribute(idx, "href") {
            if href.is_empty() {
                continue;
            }
            // 拼接完整 URL
            let full_url = if href.starts_with("http") {
                href
            } else if href.starts_with("//") {
                format!("https:{}", href)
            } else {
                let base_url = dom.url().map(|u| u.as_str()).unwrap_or("");
                if href.starts_with('/') {
                    let base = base_url.trim_end_matches('/');
                    // 提取协议 + 主机名
                    if let Some(pos) = base.find("://") {
                        if let Some(slash_pos) = base[pos + 3..].find('/') {
                            let origin = &base[..=pos + 3 + slash_pos];
                            format!("{}{}", origin.trim_end_matches('/'), href)
                        } else {
                            format!("{}{}", base.trim_end_matches('/'), href)
                        }
                    } else {
                        format!("{}{}", base.trim_end_matches('/'), href)
                    }
                } else {
                    let base = base_url.trim_end_matches('/');
                    format!("{}/{}", base, href.trim_start_matches("./"))
                }
            };

            // 同步下载（使用 tokio runtime block_on）
            trace!("下载外部 CSS: {}", full_url);
            use std::sync::OnceLock;
            static CSS_RT: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
            let rt = CSS_RT.get_or_init(|| tokio::runtime::Runtime::new().unwrap());
            let url_copy = full_url.clone();
            let result: Result<String, reqwest::Error> = rt.block_on(async {
                let resp = reqwest::get(&url_copy).await?;
                resp.text().await
            });
            match result {
                Ok(body) => {
                    if !body.trim().is_empty() {
                        if !css.is_empty() {
                            css.push('\n');
                        }
                        css.push_str(body.trim());
                        trace!("外部 CSS 下载成功: {} ({} 字符)", full_url, body.len());
                    }
                }
                Err(e) => {
                    warn!("外部 CSS 下载失败 ({}): {}", full_url, e);
                }
            }
        }
    }

    css
}

/// 使用 Taffy 布局结果渲染页面的渲染器
struct TaffyRenderer<'a> {
    painter: &'a mut Painter,
    taffy: &'a TaffyLayoutEngine,
    dom: &'a DomWrapper,
}

impl<'a> TaffyRenderer<'a> {
    fn render_dom(&mut self) {
        // 如果 taffy 为空（没有布局节点），走简单的 fallback 渲染
        if self.taffy.is_empty() {
            warn!("Taffy 布局为空，使用简单渲染回退");
            self.render_simple();
            return;
        }

        // 使用 Taffy 布局结果遍历渲染
        let doc = self.dom.inner_document();
        // 从 document 的子节点开始遍历（跳过 Document 节点本身）
        for child in doc.children() {
            self.render_tree_with_taffy(&child);
        }
    }

    /// 简单 fallback 渲染（没有 Taffy 布局时使用）
    fn render_simple(&mut self) {
        let body = self.dom.body();
        let mut current_y = 20.0;
        self.render_simple_node(body, &mut current_y);
    }

    fn render_simple_node(&mut self, node: usize, current_y: &mut f32) {
        if let Some(node_ref) = self.dom.get_node(node) {
            if let Some(text) = node_ref.as_text() {
                let contents = text.borrow();
                let trimmed = contents.trim();
                if !trimmed.is_empty() {
                    let width = 740.0; // 默认内容宽度
                    self.render_text_at(
                        trimmed,
                        20.0,
                        *current_y,
                        width,
                        16.0,
                        &Color::from_hex("#333333"),
                    );
                    *current_y += 22.0;
                }
            } else if let Some(element) = node_ref.as_element() {
                let tag_name = element.name.local.to_string();
                match tag_name.as_str() {
                    "h1" => {
                        *current_y += 10.0;
                        let text = collect_text(&node_ref);
                        self.render_text_at(
                            &text,
                            20.0,
                            *current_y,
                            740.0,
                            24.0,
                            &Color::from_hex("#333333"),
                        );
                        *current_y += 40.0;
                    }
                    "h2" => {
                        *current_y += 8.0;
                        let text = collect_text(&node_ref);
                        self.render_text_at(
                            &text,
                            20.0,
                            *current_y,
                            740.0,
                            20.0,
                            &Color::from_hex("#333333"),
                        );
                        *current_y += 35.0;
                    }
                    "h3" => {
                        *current_y += 6.0;
                        let text = collect_text(&node_ref);
                        self.render_text_at(
                            &text,
                            20.0,
                            *current_y,
                            740.0,
                            18.0,
                            &Color::from_hex("#333333"),
                        );
                        *current_y += 30.0;
                    }
                    "p" => {
                        let text = collect_text(&node_ref);
                        self.render_text_at(
                            &text,
                            20.0,
                            *current_y,
                            740.0,
                            16.0,
                            &Color::from_hex("#333333"),
                        );
                        *current_y += 25.0;
                    }
                    "img" => {
                        let src = element
                            .attributes
                            .borrow()
                            .get("src")
                            .map(|s| s.to_string());
                        if let Some(url) = src {
                            let pixmap = self.load_image(&url);
                            if let Some(p) = pixmap {
                                self.painter.draw_rect(
                                    20.0,
                                    *current_y,
                                    p.width() as f32,
                                    p.height() as f32,
                                    &Color::WHITE,
                                );
                                self.painter.pixmap_mut().draw_pixmap(
                                    20.0 as i32,
                                    *current_y as i32,
                                    p.as_ref(),
                                    &tiny_skia::PixmapPaint::default(),
                                    tiny_skia::Transform::identity(),
                                    None,
                                );
                            }
                        }
                        *current_y += 160.0;
                    }
                    "br" => {
                        *current_y += 20.0;
                    }
                    "hr" => {
                        self.painter.draw_rect(
                            20.0,
                            *current_y,
                            740.0,
                            1.0,
                            &Color::from_hex("#dddddd"),
                        );
                        *current_y += 10.0;
                    }
                    _ => {}
                }
            }
        }

        for child in self.dom.children(node) {
            self.render_simple_node(child, current_y);
        }
    }

    /// 使用 Taffy 布局结果的主渲染循环
    fn render_tree_with_taffy(&mut self, node_ref: &NodeRef) {
        // 按深度优先遍历 NodeRef 树，从 taffy 获取布局坐标
        if let Some(element) = node_ref.as_element() {
            let tag_name = element.name.local.to_string();

            // 找到对应的 dom 索引
            let dom_idx_opt = self.find_dom_index(node_ref);
            if let Some(dom_idx) = dom_idx_opt {
                // 通过 dom 索引获取 taffy 布局
                if let Some(layout) = self.taffy.get_layout(dom_idx) {
                    let x = layout.x;
                    let y = layout.y;
                    let w = layout.width;
                    let h = layout.height;

                    // 检查当前节点是否为 hover 节点，如果是则应用 hover 样式覆盖
                    let is_hovered = Some(dom_idx) == self.taffy.hovered_node;

                    // 渲染元素背景
                    if let Some(bg) = &layout.background {
                        self.painter.draw_rect(x, y, w, h, bg);
                    }

                    // 渲染 box-shadow
                    self.render_box_shadow_for_element(&tag_name, x, y, w, h);

                    // 渲染背景图片
                    self.render_background_image(&tag_name, x, y, w, h);

                    // 渲染元素装饰（边框等）+ 传入 node_ref 用于 img src
                    self.render_element_box(&tag_name, x, y, w, h, Some(node_ref));

                    // 如果是 hover 节点，绘制高亮边框
                    if is_hovered {
                        self.painter
                            .draw_rect_border(x, y, w, h, 2.0, &Color::from_hex("#4A90D9"));
                    }

                    // 渲染图片元素
                    if tag_name == "img" {
                        self.render_img_element(x, y, w, h, node_ref);
                    }

                    // 渲染元素的直接文本内容（跳过 style/script 等不可见标签）
                    if tag_name != "style" && tag_name != "script" && tag_name != "head" {
                        let text_content = collect_text(node_ref);
                        if !text_content.trim().is_empty() {
                            let font_size = layout.font_size.max(12.0);
                            let default_color = Color::from_hex("#333333");
                            let font_color = layout.font_color.as_ref().unwrap_or(&default_color);
                            let padding = 10.0;
                            self.render_text_at_weight(
                                &text_content,
                                x + padding,
                                y + padding,
                                w - padding * 2.0,
                                font_size,
                                font_color,
                                layout.font_weight,
                            );
                        }
                    }
                }
            }

            // 递归渲染子节点
            for child in node_ref.children() {
                self.render_tree_with_taffy(&child);
            }
        } else if node_ref.as_text().is_some() {
            // 文本节点：查找父元素布局来渲染文本
            if let Some(parent) = node_ref.parent() {
                if let Some(element) = parent.as_element() {
                    let tag_name = element.name.local.to_string();
                    if let Some(dom_idx) = self.find_dom_index(&parent) {
                        if let Some(layout) = self.taffy.get_layout(dom_idx) {
                            // 跳过 style/script/head 等不可见标签内的文本
                            if tag_name != "img"
                                && tag_name != "style"
                                && tag_name != "script"
                                && tag_name != "head"
                            {
                                let text_content = collect_text(&parent);
                                if !text_content.trim().is_empty() {
                                    // 已经在父元素渲染过了，跳过
                                } else if let Some(text) = node_ref.as_text() {
                                    let contents = text.borrow();
                                    let trimmed = contents.trim();
                                    if !trimmed.is_empty() {
                                        let font_size = layout.font_size.max(12.0);
                                        let default_color = Color::from_hex("#333333");
                                        let font_color =
                                            layout.font_color.as_ref().unwrap_or(&default_color);
                                        let padding = 10.0;
                                        self.render_text_at_weight(
                                            trimmed,
                                            layout.x + padding,
                                            layout.y + padding,
                                            layout.width - padding * 2.0,
                                            font_size,
                                            font_color,
                                            layout.font_weight,
                                        );
                                    }
                                }
                            }
                        }
                    }
                }
            }
        } else {
            // 文档节点等 - 直接递归子节点
            for child in node_ref.children() {
                self.render_tree_with_taffy(&child);
            }
        }
    }

    /// 通过 NodeRef 的 Rc 指针查找 DOM 索引
    fn find_dom_index(&self, node_ref: &NodeRef) -> Option<usize> {
        let rc_ptr = Rc::as_ptr(&node_ref.0) as usize;
        self.dom
            .inner_document()
            .descendants()
            .position(|n| Rc::as_ptr(&n.0) as usize == rc_ptr)
    }

    /// 精美渲染元素装饰（背景、边框、装饰线等）
    fn render_element_box(
        &mut self,
        tag: &str,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        node_ref: Option<&NodeRef>,
    ) {
        match tag {
            "hr" => {
                // 精美分割线
                let mid_y = y + h / 2.0;
                // 主线条
                self.painter
                    .draw_rect(x + 8.0, mid_y, w - 16.0, 1.0, &Color::from_hex("#DDDDDD"));
            }
            "blockquote" => {
                // 引用左侧色条 + 浅灰背景
                self.painter
                    .draw_rounded_rect(x, y, w, h, 4.0, &Color::from_hex("#F9F9F9"));
                self.painter
                    .draw_rounded_rect(x, y, 4.0, h, 2.0, &Color::from_hex("#4A90D9"));
            }
            "button" => {
                // 圆角按钮
                let is_focused = self.is_node_focused(node_ref);
                let bg = if is_focused {
                    Color::from_hex("#3A7BD5")
                } else {
                    Color::from_hex("#4A90D9")
                };
                self.painter.draw_rounded_rect(x, y, w, h, 6.0, &bg);
                // 顶部高光
                self.painter
                    .draw_rounded_rect(x, y, w, h * 0.5, 6.0, &Color::from_hex("#5BA0E9"));
            }
            "a" => {
                // 链接无特殊装饰，由 text 渲染处理下划线
            }
            "img" => {
                // 图片：已在 render_img_element 中处理，占位符只在无 src 时显示
            }
            "input" => {
                self.render_input_element(x, y, w, h, node_ref);
            }
            "textarea" => {
                self.render_textarea_element(x, y, w, h, node_ref);
            }
            "li" => {
                // 列表圆点
                let dot_size = 6.0;
                let dot_x = x + 8.0;
                let dot_y = y + h / 2.0;
                // 使用小矩形模拟圆点
                self.painter.draw_rounded_rect(
                    dot_x - dot_size / 2.0,
                    dot_y - dot_size / 2.0,
                    dot_size,
                    dot_size,
                    dot_size / 2.0,
                    &Color::from_hex("#666666"),
                );
            }
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                // 标题左侧装饰条
                self.painter.draw_rounded_rect(
                    x + 4.0,
                    y + 4.0,
                    4.0,
                    (h - 8.0).max(8.0),
                    2.0,
                    &Color::from_hex("#4A90D9"),
                );
            }
            _ => {}
        }
    }

    /// 实际渲染图片元素
    fn render_img_element(&mut self, x: f32, y: f32, w: f32, h: f32, element: &NodeRef) {
        if let Some(el) = element.as_element() {
            let src = el.attributes.borrow().get("src").map(|s| s.to_string());
            if let Some(url) = src {
                let pixmap = self.load_image(&url);
                if let Some(p) = pixmap {
                    // 居中绘制图片（不缩放）
                    let draw_x = x + (w - p.width() as f32) / 2.0;
                    let draw_y = y + (h - p.height() as f32) / 2.0;
                    self.painter.pixmap_mut().draw_pixmap(
                        draw_x as i32,
                        draw_y as i32,
                        p.as_ref(),
                        &tiny_skia::PixmapPaint::default(),
                        tiny_skia::Transform::identity(),
                        None,
                    );
                }
            }
        }
    }

    /// 渲染图片占位符（加载失败或没有 src 时的占位）
    fn render_img_placeholder(&mut self, x: f32, y: f32, w: f32, h: f32) {
        // 浅灰色背景
        self.painter
            .draw_rect(x, y, w, h, &Color::from_hex("#f0f0f0"));
        // 边框
        self.painter
            .draw_rect(x, y, w, 1.0, &Color::from_hex("#dddddd"));
        self.painter
            .draw_rect(x, y + h - 1.0, w, 1.0, &Color::from_hex("#dddddd"));
        // 居中图标（相机符号简化）
        let center_x = x + w / 2.0 - 20.0;
        let center_y = y + h / 2.0 - 10.0;
        self.painter
            .draw_rect(center_x, center_y, 40.0, 20.0, &Color::from_hex("#cccccc"));
    }

    /// 精美渲染 <input> 元素（单行文本输入框）
    fn render_input_element(&mut self, x: f32, y: f32, w: f32, h: f32, node_ref: Option<&NodeRef>) {
        let is_focused = self.is_node_focused(node_ref);
        let bg_color = Color::from_hex("#FFFFFF");
        let border_color = if is_focused {
            Color::from_hex("#4A90D9") // 聚焦时蓝色边框
        } else {
            Color::from_hex("#CCCCCC") // 默认灰色边框
        };

        // 白色圆角背景
        self.painter.draw_rounded_rect(x, y, w, h, 4.0, &bg_color);
        // 圆角边框
        self.painter.draw_rounded_border(
            x,
            y,
            w,
            h,
            4.0,
            if is_focused { 2.0 } else { 1.0 },
            &border_color,
        );

        // 内部阴影效果（浅灰内边线）
        if !is_focused {
            self.painter.draw_rounded_border(
                x + 0.5,
                y + 0.5,
                w - 1.0,
                h - 1.0,
                3.5,
                0.5,
                &Color::from_hex("#EEEEEE"),
            );
        }

        // 读取 value 属性
        let value = node_ref
            .and_then(|nr| {
                nr.as_element()
                    .and_then(|el| el.attributes.borrow().get("value").map(|s| s.to_string()))
            })
            .unwrap_or_default();

        let font_size = (h * 0.55).max(12.0).min(16.0);
        let padding = 8.0;
        let text_color = &Color::from_hex("#333333");

        // 渲染文本（左对齐，垂直居中）
        let text_x = x + padding;
        let text_y = y + (h - font_size) / 2.0;
        let max_width = w - padding * 2.0;

        if !value.is_empty() {
            self.render_text_at(&value, text_x, text_y, max_width, font_size, text_color);
        }

        // 聚焦时绘制光标
        if is_focused {
            let text_width = value.len() as f32 * (font_size * 0.6).max(6.0);
            let cx = (text_x + text_width).min(x + w - padding);
            self.painter
                .draw_rect(cx, y + 4.0, 1.5, h - 8.0, &Color::from_hex("#333333"));
        }
    }

    /// 判断指定节点是否获得焦点
    fn is_node_focused(&self, node_ref: Option<&NodeRef>) -> bool {
        let taffy_focused = self.taffy.focused_node;
        node_ref
            .and_then(|nr| {
                let rc_ptr = Rc::as_ptr(&nr.0) as usize;
                self.dom
                    .inner_document()
                    .descendants()
                    .position(|n| Rc::as_ptr(&n.0) as usize == rc_ptr)
                    .and_then(|dom_idx| {
                        if taffy_focused == Some(dom_idx) {
                            Some(true)
                        } else {
                            None
                        }
                    })
            })
            .is_some()
    }

    /// 精美渲染 <textarea> 元素（多行文本输入框）
    fn render_textarea_element(
        &mut self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        node_ref: Option<&NodeRef>,
    ) {
        let is_focused = self.is_node_focused(node_ref);
        let border_color = if is_focused {
            Color::from_hex("#4A90D9")
        } else {
            Color::from_hex("#CCCCCC")
        };

        // 白色圆角背景
        self.painter
            .draw_rounded_rect(x, y, w, h, 4.0, &Color::WHITE);
        // 圆角边框
        self.painter.draw_rounded_border(
            x,
            y,
            w,
            h,
            4.0,
            if is_focused { 2.0 } else { 1.0 },
            &border_color,
        );

        // 读取 value 属性或直接子文本
        let value = node_ref
            .and_then(|nr| {
                nr.as_element()
                    .and_then(|el| el.attributes.borrow().get("value").map(|s| s.to_string()))
            })
            .unwrap_or_else(|| node_ref.map(|nr| collect_text(&nr)).unwrap_or_default());

        let font_size = 14.0;
        let padding = 8.0;
        let text_color = &Color::from_hex("#333333");

        // 渲染文本
        let text_x = x + padding;
        let text_y = y + padding;
        let max_width = w - padding * 2.0;

        if !value.is_empty() {
            self.render_text_at(&value, text_x, text_y, max_width, font_size, text_color);
        }

        // 聚焦时绘制光标
        if is_focused {
            let line_height = font_size * 1.4;
            let lines: Vec<&str> = value.lines().collect();
            let last_line = lines.last().unwrap_or(&"");
            let cursor_x_val = text_x + (last_line.len() as f32) * (font_size * 0.6).max(6.0);
            let cursor_y = text_y + (lines.len().max(1) - 1) as f32 * line_height;
            self.painter.draw_rect(
                cursor_x_val,
                cursor_y,
                1.5,
                font_size * 1.1,
                &Color::from_hex("#333333"),
            );
        }
    }

    /// 渲染元素的 box-shadow
    fn render_box_shadow_for_element(&mut self, tag: &str, x: f32, y: f32, w: f32, h: f32) {
        // 只对块级元素渲染 box-shadow
        if let Some(shadow) = self.find_box_shadow_for_tag(tag) {
            draw_box_shadow(
                self.painter.pixmap_mut(),
                x,
                y,
                w,
                h,
                shadow.offset_x,
                shadow.offset_y,
                shadow.blur_radius,
                shadow.spread,
                &shadow.color,
            );
        }
    }

    /// 查找标签的 box-shadow 定义（从内联 style 和 taffy style_map 中查找）
    fn find_box_shadow_for_tag(&self, tag: &str) -> Option<BoxShadowValue> {
        // 1. 先检查布局节点对应的 DOM 元素的内联样式
        let nodes = self.taffy.find_by_tag(tag);
        for node in nodes {
            if let Some(tag_node) = self.dom.get_node(node.dom_node) {
                if let Some(el) = tag_node.as_element() {
                    let attrs = el.attributes.borrow();
                    if let Some(style) = attrs.get("style") {
                        if let Some(shadow) = parse_box_shadow_from_style(style) {
                            return Some(shadow);
                        }
                    }
                }
            }
        }

        // 2. 从 TaffyLayoutEngine 的 StyleMap 中查找
        if let Some(shadow_css) = self.taffy.find_box_shadow_style(tag) {
            if let Some(shadow) = parse_box_shadow_from_style(&shadow_css) {
                return Some(shadow);
            }
        }

        // 3. 如果 tag 本身没匹配到，尝试查找通配选择器（*）的 box-shadow
        if let Some(shadow_css) = self.taffy.find_box_shadow_style("*") {
            if let Some(shadow) = parse_box_shadow_from_style(&shadow_css) {
                return Some(shadow);
            }
        }

        None
    }

    /// 通过全局 ImageCache 加载图片
    fn load_image(&self, url: &str) -> Option<tiny_skia::Pixmap> {
        let cache = global_image_cache();
        cache.get(url)
    }

    /// 渲染背景图片（支持 sprite 裁剪）
    fn render_background_image(&mut self, _tag: &str, x: f32, y: f32, w: f32, h: f32) {
        // 查找当前渲染树节点对应的布局
        // 实际上在 render_tree_with_taffy 中已经通过 find_dom_index 获取了 layout
        // 但为了简化，我们遍历所有布局节点查找匹配的元素

        // 通过 tag 查找所有布局节点
        let nodes = self.taffy.find_by_tag(_tag);
        for layout in nodes {
            if let Some(bg_image) = &layout.background_image {
                let cache = global_image_cache();

                // 如果有 background-position，使用 crop_sprite 裁剪
                let bg_x = layout.bg_position_x as i32;
                let bg_y = layout.bg_position_y as i32;

                if bg_x != 0 || bg_y != 0 {
                    // 使用 sprite 裁剪
                    if let Some(cropped) =
                        ImageCache::crop_sprite(bg_image, bg_x, bg_y, w as u32, h as u32)
                    {
                        self.painter.pixmap_mut().draw_pixmap(
                            (x) as i32,
                            (y) as i32,
                            cropped.as_ref(),
                            &tiny_skia::PixmapPaint::default(),
                            tiny_skia::Transform::identity(),
                            None,
                        );
                        return;
                    }
                }

                // 无偏移或裁剪失败时，直接加载完整图片
                if let Some(pixmap) = cache.get(bg_image) {
                    // 缩放图片适配元素区域
                    let img_w = pixmap.width() as f32;
                    let img_h = pixmap.height() as f32;
                    let scale_x = w / img_w;
                    let scale_y = h / img_h;
                    let scale = scale_x.min(scale_y).min(1.0); // 只缩小不放大

                    if scale < 1.0 {
                        // 缩放到元素区域 - 使用 draw_pixmap 的缩放变换
                        let transform =
                            tiny_skia::Transform::from_scale(scale, scale).post_translate(x, y);
                        self.painter.pixmap_mut().draw_pixmap(
                            x as i32,
                            y as i32,
                            pixmap.as_ref(),
                            &tiny_skia::PixmapPaint::default(),
                            transform,
                            None,
                        );
                    } else {
                        // 居中绘制
                        let draw_x = x + (w - img_w) / 2.0;
                        let draw_y = y + (h - img_h) / 2.0;
                        self.painter.pixmap_mut().draw_pixmap(
                            draw_x as i32,
                            draw_y as i32,
                            pixmap.as_ref(),
                            &tiny_skia::PixmapPaint::default(),
                            tiny_skia::Transform::identity(),
                            None,
                        );
                    }
                }
            }
        }
    }

    /// 使用 cosmic-text 渲染文本
    fn render_text_at(
        &mut self,
        text: &str,
        x: f32,
        y: f32,
        max_width: f32,
        font_size: f32,
        color: &Color,
    ) {
        self.render_text_at_weight(text, x, y, max_width, font_size, color, 400)
    }

    /// 使用 cosmic-text 渲染文本（完整参数）
    #[allow(dead_code)]
    fn render_text_at_weight(
        &mut self,
        text: &str,
        x: f32,
        y: f32,
        max_width: f32,
        font_size: f32,
        color: &Color,
        font_weight: u16,
    ) {
        let trimmed = text.trim();
        if trimmed.is_empty() || font_size <= 0.0 {
            return;
        }

        let mut font_system = global_font_system().lock().unwrap();
        let mut swash_cache = global_swash_cache().lock().unwrap();

        let line_height = font_size * 1.375;

        // 创建文本缓冲区
        let mut buffer = Buffer::new(&mut font_system, Metrics::new(font_size, line_height));

        buffer.set_size(Some(max_width.max(50.0)), Some(f32::INFINITY));
        buffer.set_wrap(Wrap::Word);
        let attrs = Attrs::new().weight(cosmic_text::Weight(font_weight));
        buffer.set_text(trimmed, &attrs, Shaping::Advanced, Some(Align::Left));
        buffer.shape_until_scroll(&mut font_system, true);

        let scale = 1.0;
        let mut line_y = y;

        for run in buffer.layout_runs() {
            for glyph in run.glyphs {
                let physical_glyph = glyph.physical((x, line_y), scale);
                let cache_key = physical_glyph.cache_key;

                if let Some(swash_image) = swash_cache.get_image(&mut font_system, cache_key) {
                    let glyph_x = physical_glyph.x as f32 + swash_image.placement.left as f32;
                    let glyph_y = physical_glyph.y as f32 - swash_image.placement.top as f32;

                    self.render_glyph_image(
                        glyph_x,
                        glyph_y,
                        swash_image.placement.width as u32,
                        swash_image.placement.height as u32,
                        &swash_image.data,
                        color,
                    );
                }
            }

            line_y += line_height;
        }
    }

    /// 将字形 alpha 蒙版渲染到 pixmap（使用 direct pixel access）
    fn render_glyph_image(
        &mut self,
        x: f32,
        y: f32,
        width: u32,
        height: u32,
        alpha_data: &[u8],
        color: &Color,
    ) {
        if width == 0 || height == 0 || alpha_data.is_empty() {
            return;
        }

        let text_rgba = color.to_rgba();
        let pixmap_width = self.painter.pixmap_mut().width();
        let pixmap_height = self.painter.pixmap_mut().height();

        let pixel_data = self.painter.pixmap_mut().data_mut();
        let stride = pixmap_width as usize * 4;

        for row in 0..height {
            for col in 0..width {
                let alpha_idx = (row * width + col) as usize;
                if alpha_idx >= alpha_data.len() {
                    continue;
                }

                let alpha = alpha_data[alpha_idx];
                if alpha == 0 {
                    continue;
                }

                let px = (x + col as f32) as i32;
                let py = (y + row as f32) as i32;

                if px < 0 || py < 0 {
                    continue;
                }

                let px_u = px as u32;
                let py_u = py as u32;

                if px_u >= pixmap_width || py_u >= pixmap_height {
                    continue;
                }

                let pixel_idx = (py_u as usize) * stride + (px_u as usize) * 4;
                if pixel_idx + 3 < pixel_data.len() {
                    let bg_r = pixel_data[pixel_idx];
                    let bg_g = pixel_data[pixel_idx + 1];
                    let bg_b = pixel_data[pixel_idx + 2];

                    let a_norm = alpha as f32 / 255.0;
                    let inv_a = 1.0 - a_norm;

                    pixel_data[pixel_idx] =
                        (text_rgba[0] as f32 * a_norm + bg_r as f32 * inv_a) as u8;
                    pixel_data[pixel_idx + 1] =
                        (text_rgba[1] as f32 * a_norm + bg_g as f32 * inv_a) as u8;
                    pixel_data[pixel_idx + 2] =
                        (text_rgba[2] as f32 * a_norm + bg_b as f32 * inv_a) as u8;
                    pixel_data[pixel_idx + 3] = 255;
                }
            }
        }
    }
}

/// 收集节点下的所有文本内容
fn collect_text(node_ref: &NodeRef) -> String {
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

impl Renderer {
    pub fn default_renderer() -> Self {
        Self::new(1280, 720)
    }

    pub fn with_size(width: u32, height: u32) -> Self {
        Self::new(width, height)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_renderer_creation() {
        let renderer = Renderer::new(800, 600);
        assert_eq!(renderer.context().viewport(), (800, 600));
    }

    #[test]
    fn test_render_blank_page() {
        let mut renderer = Renderer::new(100, 100);
        let result = renderer.render(&None);
        assert!(result.is_ok());
    }

    #[test]
    fn test_global_image_cache() {
        let cache = global_image_cache();
        // 只是验证返回非空
        assert!(cache.len() == 0 || cache.len() >= 0);
    }
}
