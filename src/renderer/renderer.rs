//! Renderer - 主渲染器
//!
//! 使用 kuchiki DOM, cosmic-text 文本渲染 和 tiny-skia 渲染

use crate::browser::Document;
use crate::css::values::Color;
use crate::css_engine::{parse_css_rules, rules_to_style_map};
#[cfg(any(feature = "boa", feature = "v8"))]
use crate::js_dom_bridge::JsDomBridge;
#[cfg(any(feature = "boa", feature = "v8"))]
use crate::js_engine::JsEngine;
use crate::renderer::border::draw_box_shadow;
use crate::renderer::context::RenderContext;
use crate::renderer::image_cache::ImageCache;
use crate::renderer::painter::Painter;
use crate::renderer::taffy_layout::{TaffyLayoutEngine, TaffyLayoutNode};
use crate::renderer::text::TextRenderer;
use crate::DomWrapper;
use cosmic_text::{Align, Attrs, Buffer, FontSystem, Metrics, Shaping, SwashCache, Wrap};
use kuchiki::NodeRef;
use log::{debug, info, trace, warn};
use std::path::Path;
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
    /// 垂直滚动偏移（像素）
    pub scroll_offset_y: f32,
    /// 页面总内容高度（由最近一次布局计算得出）
    pub content_height: f32,
    /// JS DOM 桥接器（仅在启用 JS 引擎时可用）
    #[cfg(any(feature = "boa", feature = "v8"))]
    js_dom_bridge: Option<JsDomBridge>,
    /// JS 引擎（仅在启用 JS 引擎时可用）
    #[cfg(any(feature = "boa", feature = "v8"))]
    js_engine: Option<JsEngine>,
}

impl Renderer {
    pub fn new(width: u32, height: u32) -> Self {
        info!("初始化渲染器 ({}x{})", width, height);

        let context = RenderContext::new(width, height);
        let painter = Painter::new(width, height)
            .unwrap_or_else(|| panic!("创建 {}x{} 像素的 Painter 失败", width, height));
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
            scroll_offset_y: 0.0,
            content_height: 0.0,
            last_key_time: 0.0,
            #[cfg(any(feature = "boa", feature = "v8"))]
            js_dom_bridge: None,
            #[cfg(any(feature = "boa", feature = "v8"))]
            js_engine: None,
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
        self.is_loading = true;
        let doc = self.document.take();
        let result = self.render(&doc);
        self.document = doc;
        self.is_loading = false;
        result
    }

    /// 渲染存储的文档到 RGBA 像素缓冲区
    /// 返回 (width, height, rgba_pixels)
    /// 比 render_to_png 省掉 PNG 编解码，性能提升约 30%
    pub fn render_to_rgba(&mut self) -> Result<(u32, u32, Vec<u8>), RenderError> {
        self.is_loading = true;
        let doc = self.document.take();
        self.painter.set_background(Color::WHITE);
        self.painter.paint();
        if let Some(ref d) = doc {
            self.render_document(d)?;
        } else {
            self.render_blank_page()?;
        }
        let (w, h) = self.context.viewport();
        // 直接从 Pixmap 取 RGBA 像素，跳过 PNG 编码
        let data = self.painter.pixmap().data().to_vec();
        self.document = doc;
        self.is_loading = false;
        Ok((w, h, data))
    }

    /// 渲染文档到 RGBA，应用垂直滚动偏移
    /// 与 render_to_rgba 的区别：将视口作为裁剪窗口，只渲染可见区域内的内容
    pub fn render_to_rgba_with_scroll(&mut self) -> Result<(u32, u32, Vec<u8>), RenderError> {
        self.is_loading = true;
        let doc = self.document.take();

        let (vp_w, vp_h) = self.context.viewport();

        // 背景填充
        self.painter.set_background(Color::WHITE);
        self.painter.paint();

        if let Some(ref d) = doc {
            // 设置视口裁剪（只绘制可见区域）
            self.painter.set_clip(0.0, 0.0, vp_w as f32, vp_h as f32);

            // 保存滚动偏移到临时变量，供渲染回调使用
            let scroll_y = self.scroll_offset_y;

            // 渲染文档（TaffyRenderer 会通过 context 获取滚动偏移）
            // 这里通过设置 painter 的全局偏移来实现
            self.render_document_with_scroll(d, scroll_y)?;

            self.painter.clear_clip();
        } else {
            self.render_blank_page()?;
        }

        let data = self.painter.pixmap().data().to_vec();
        self.document = doc;
        self.is_loading = false;
        Ok((vp_w, vp_h, data))
    }

    /// 简化的 RGBA 渲染（用于截图模式）
    pub fn render_to_rgba_simple(&mut self) -> Result<(u32, u32, Vec<u8>), RenderError> {
        self.is_loading = true;
        let doc = self.document.take();

        let (vp_w, vp_h) = self.context.viewport();

        // 背景填充
        self.painter.set_background(Color::WHITE);
        self.painter.paint();

        if let Some(ref d) = doc {
            // 不设置裁剪，完整渲染
            self.render_document(d)?;
        } else {
            self.render_blank_page()?;
        }

        let data = self.painter.pixmap().data().to_vec();
        self.document = doc;
        self.is_loading = false;
        Ok((vp_w, vp_h, data))
    }

    /// 带滚动偏移的文档渲染
    fn render_document_with_scroll(
        &mut self,
        document: &Document,
        scroll_y: f32,
    ) -> Result<(), RenderError> {
        trace!(
            "渲染文档 (scroll_y={:.0}): {}",
            scroll_y,
            document.title.as_deref().unwrap_or("无标题")
        );

        let dom = document.get_dom();

        // 1. 从 DOM 提取 <style> CSS
        let css_text = extract_style_tags(dom);

        // 2. 解析 CSS 规则并生成 StyleMap
        let style_map = if !css_text.is_empty() {
            let rules = parse_css_rules(&css_text);
            rules_to_style_map(&rules, dom.inner_document())
        } else {
            Default::default()
        };

        // 3. 创建 TaffyLayoutEngine 并设置 StyleMap
        let (vp_w, vp_h) = self.context.viewport();
        let mut taffy = TaffyLayoutEngine::new(vp_w as f32, vp_h as f32);
        taffy.set_style_map(style_map);

        // 4. 计算布局
        let _ = taffy.compute(dom);

        // 5. 从 body/html 获取背景色
        let page_bg = taffy
            .find_by_tag("body")
            .iter()
            .find_map(|n| n.background.clone())
            .or_else(|| {
                taffy
                    .find_by_tag("html")
                    .iter()
                    .find_map(|n| n.background.clone())
            })
            .unwrap_or(Color::WHITE);
        self.painter.set_background(page_bg);
        self.painter.paint();

        // 6. 更新内容高度（用于夹紧滚动范围）
        self.content_height = taffy.document_height();

        // 7. 使用 Taffy 布局结果渲染，应用滚动偏移
        {
            let mut renderer = TaffyRenderer {
                painter: &mut self.painter,
                taffy: &taffy,
                dom,
                node_index_cache: std::collections::HashMap::new(),
                scroll_offset_y: scroll_y,
            };
            renderer.render_dom();
        }

        self.last_taffy = Some(taffy);
        debug!("文档渲染完成 (scroll_y={:.0})", scroll_y);
        Ok(())
    }

    /// 设置滚动偏移（自动夹紧到有效范围内）
    pub fn set_scroll_offset(&mut self, offset_y: f32) {
        let max_scroll = (self.content_height - self.context.viewport().1 as f32).max(0.0);
        self.scroll_offset_y = offset_y.clamp(0.0, max_scroll);
    }

    /// 获取当前的滚动偏移
    pub fn scroll_offset(&self) -> f32 {
        self.scroll_offset_y
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
        let (vp_w, vp_h) = self.context.viewport();
        let mut taffy = TaffyLayoutEngine::new(vp_w as f32, vp_h as f32);
        taffy.set_style_map(style_map);

        // 4. 计算布局
        let layout_result = taffy.compute(dom);
        if let Err(e) = &layout_result {
            warn!("Taffy 布局计算失败: {}, 使用手动布局回退", e);
        }

        // 调试日志：检查布局结果
        debug!(
            "布局结果: nodes={}, is_empty={}",
            taffy.len(),
            taffy.is_empty()
        );

        // 如果布局为空但文档不是空的，尝试使用 fallback
        if taffy.is_empty() {
            // 检查 DOM 中有多少个元素
            let elements = dom.traverse_elements();
            debug!("DOM 元素数量: {}", elements.len());

            // 仍然尝试渲染，至少显示 loading 页面内容
            // 使用 render_simple 作为 fallback
        }

        // 4.5 从 body/html 获取背景色，设置画布背景
        let body_nodes = taffy.find_by_tag("body");
        let html_nodes = taffy.find_by_tag("html");
        let page_bg = body_nodes
            .iter()
            .find_map(|n| n.background.clone())
            .or_else(|| html_nodes.iter().find_map(|n| n.background.clone()))
            .unwrap_or(Color::WHITE);
        self.painter.set_background(page_bg);
        self.painter.paint();

        // 5. 使用 Taffy 布局结果渲染（只渲染视口大小，不创建超长截图）
        {
            let mut renderer = TaffyRenderer {
                painter: &mut self.painter,
                taffy: &taffy,
                dom,
                node_index_cache: std::collections::HashMap::new(),
                scroll_offset_y: 0.0,
            };
            renderer.render_dom();
        }

        // 6. 保存布局结果，用于后续点击测试
        self.last_taffy = Some(taffy);

        debug!("文档渲染完成");
        Ok(())
    }

    /// 直接使用给定的 DOM 和 TaffyLayoutEngine 渲染，返回 PNG 字节
    ///
    /// 如果启用了 JS DOM 桥接器且有未处理的 DOM 变更，
    /// 会在渲染前重新计算布局以反映最新的 DOM 状态。
    pub fn render_with_taffy(
        &mut self,
        dom: &DomWrapper,
        taffy: &TaffyLayoutEngine,
    ) -> Result<Vec<u8>, RenderError> {
        self.is_loading = true;

        // 如果启用了 JS 引擎且有未处理的 DOM 变更，重新计算布局
        #[cfg(any(feature = "boa", feature = "v8"))]
        self.rebuild_layout_if_dirty(dom, taffy);

        // 从 body/html 获取背景色
        let page_bg = taffy
            .find_by_tag("body")
            .iter()
            .find_map(|n| n.background.clone())
            .or_else(|| {
                taffy
                    .find_by_tag("html")
                    .iter()
                    .find_map(|n| n.background.clone())
            })
            .unwrap_or(Color::WHITE);
        self.painter.set_background(page_bg);
        self.painter.paint();

        if !taffy.is_empty() {
            let mut renderer = TaffyRenderer {
                painter: &mut self.painter,
                taffy,
                dom,
                node_index_cache: std::collections::HashMap::new(),
                scroll_offset_y: 0.0,
            };
            renderer.render_dom();
        }

        self.is_loading = false;
        Ok(self.painter.to_png())
    }

    /// 如果 JS DOM 桥接器有未处理的 DOM 变更，则重建布局
    ///
    /// 检查待处理的变更标记（has_pending_changes），
    /// 如果有变更则清空脏节点标记并重建 Taffy 布局。
    /// 注意：此方法接收 `&TaffyLayoutEngine` 不可变引用，
    /// 因此实际布局重建由调用方完成。这里仅清空变更标记。
    #[cfg(any(feature = "boa", feature = "v8"))]
    fn rebuild_layout_if_dirty(&mut self, _dom: &DomWrapper, _taffy: &TaffyLayoutEngine) {
        if let Some(ref mut bridge) = self.js_dom_bridge {
            if bridge.has_pending() {
                trace!("检测到未处理的 DOM 变更，重建布局前清空脏节点标记");
                // 获取脏节点（用于后续可能的增量更新）
                let _dirty_nodes = bridge.drain_dirty_nodes();
                // 清空变异记录
                bridge.clear_mutations();
                // 注意：实际布局重建由调用方负责，
                // 因为 taffy 参数是 &TaffyLayoutEngine 不可变引用。
                // 调用方（如 DefaultWebNativeBridge::render）会在调用此方法后
                // 重新创建 TaffyLayoutEngine 并计算布局。
            }
        }
    }

    fn render_blank_page(&mut self) -> Result<(), RenderError> {
        debug!("渲染空白页面");

        // 绘制浅灰色背景
        self.painter.set_background(Color::from_hex("#F5F5F5"));
        self.painter.paint();

        // 获取视口尺寸
        let (width, height) = self.context.viewport();

        // 计算居中提示框的位置
        let box_w = 320.0_f32.min(width as f32 * 0.8);
        let box_h = 160.0_f32.min(height as f32 * 0.5);
        let box_x = (width as f32 - box_w) / 2.0;
        let box_y = (height as f32 - box_h) / 2.0;

        // 白色背景的提示框
        self.painter
            .draw_rounded_rect(box_x, box_y, box_w, box_h, 8.0, &Color::WHITE);
        self.painter.draw_rounded_border(
            box_x,
            box_y,
            box_w,
            box_h,
            8.0,
            1.0,
            &Color::from_hex("#CCCCCC"),
        );

        // 橙色警告图标（感叹号形状）
        let icon_center_x = box_x + box_w / 2.0;
        let icon_y = box_y + 25.0;
        self.painter.draw_rect(
            icon_center_x - 2.0,
            icon_y,
            4.0,
            18.0,
            &Color::from_hex("#FF9800"),
        );
        self.painter.draw_rect(
            icon_center_x - 3.0,
            icon_y + 22.0,
            6.0,
            6.0,
            &Color::from_hex("#FF9800"),
        );

        // 创建临时的 TaffyRenderer 用于绘制文字
        let mut dummy_taffy = TaffyLayoutEngine::new(width as f32, height as f32);
        let dummy_dom = DomWrapper::from_html("<html><body></body></html>", None);

        let mut text_renderer = TaffyRenderer {
            painter: &mut self.painter,
            taffy: &mut dummy_taffy,
            scroll_offset_y: 0.0,
            dom: &dummy_dom,
            node_index_cache: std::collections::HashMap::new(),
        };

        // 绘制标题文字
        text_renderer.render_text_at(
            "页面加载失败",
            box_x + 40.0,
            box_y + 65.0,
            box_w - 80.0,
            20.0,
            &Color::from_hex("#333333"),
        );

        // 绘制错误信息
        text_renderer.render_text_at(
            "网络连接失败",
            box_x + 40.0,
            box_y + 95.0,
            box_w - 80.0,
            16.0,
            &Color::from_hex("#666666"),
        );

        // 绘制建议
        text_renderer.render_text_at(
            "请检查网络连接后重试",
            box_x + 40.0,
            box_y + 120.0,
            box_w - 80.0,
            14.0,
            &Color::from_hex("#999999"),
        );

        Ok(())
    }

    /// 渲染 loading 画面（在真正渲染完成前立即返回，不耗时）
    /// 只画白底 + 中央蓝色条，不碰布局状态。
    pub fn render_loading_page(&mut self) -> Result<Vec<u8>, RenderError> {
        let (width, height) = self.context.viewport();
        self.painter.set_background(Color::WHITE);
        self.painter.paint();

        // 中央蓝色条
        let bar_w = (width as f32 * 0.6).min(300.0);
        let bar_h = 4.0;
        let bar_x = (width as f32 - bar_w) / 2.0;
        let bar_y = height as f32 / 2.0;

        let mut paint = tiny_skia::Paint::default();
        paint.set_color_rgba8(0x4A, 0x90, 0xD9, 200);
        if let Some(rect) = tiny_skia::Rect::from_xywh(bar_x, bar_y, bar_w, bar_h) {
            self.painter.pixmap_mut().fill_rect(
                rect,
                &paint,
                tiny_skia::Transform::identity(),
                None,
            );
        }

        Ok(self.painter.to_png())
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

    /// 获取 JS DOM 桥接器引用
    #[cfg(any(feature = "boa", feature = "v8"))]
    pub fn js_dom_bridge(&self) -> Option<&JsDomBridge> {
        self.js_dom_bridge.as_ref()
    }

    /// 获取 JS DOM 桥接器可变引用
    #[cfg(any(feature = "boa", feature = "v8"))]
    pub fn js_dom_bridge_mut(&mut self) -> Option<&mut JsDomBridge> {
        self.js_dom_bridge.as_mut()
    }

    /// 设置 JS DOM 桥接器
    #[cfg(any(feature = "boa", feature = "v8"))]
    pub fn set_js_dom_bridge(&mut self, bridge: JsDomBridge) {
        self.js_dom_bridge = Some(bridge);
    }

    /// 获取 JS 引擎引用
    #[cfg(any(feature = "boa", feature = "v8"))]
    pub fn js_engine(&self) -> Option<&JsEngine> {
        self.js_engine.as_ref()
    }

    /// 获取 JS 引擎可变引用
    #[cfg(any(feature = "boa", feature = "v8"))]
    pub fn js_engine_mut(&mut self) -> Option<&mut JsEngine> {
        self.js_engine.as_mut()
    }

    /// 设置 JS 引擎
    #[cfg(any(feature = "boa", feature = "v8"))]
    pub fn set_js_engine(&mut self, engine: JsEngine) {
        self.js_engine = Some(engine);
    }

    /// 调用 JS 引擎的定时器 tick
    /// 应在渲染循环的每帧调用
    #[cfg(any(feature = "boa", feature = "v8"))]
    pub fn js_engine_tick_timers(&mut self) -> usize {
        if let Some(ref mut engine) = self.js_engine {
            engine.tick_timers()
        } else {
            0
        }
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
    /// 64 位稳定 ID → dom index 的映射
    node_index_cache: std::collections::HashMap<u64, usize>,
    /// 垂直滚动偏移量（像素），所有渲染坐标减去此值
    scroll_offset_y: f32,
}

impl<'a> TaffyRenderer<'a> {
    /// 解析图片 URL 为绝对 URL（静态方法，不依赖 self）
    ///
    /// 处理以下情形：
    /// - `//host.com/path` → `https://host.com/path`（协议相对）
    /// - `/path/img.png` → `https://domain/path/img.png`（根相对）
    /// - `img.png` → `https://domain/base/img.png`（相对路径）
    /// - `http://...` / `https://...` → 不变
    /// - `data:image/...` → 不变
    fn resolve_image_url_internal(src: &str, dom: &DomWrapper) -> String {
        // base64 和已有绝对协议的不处理
        if src.starts_with("data:image/")
            || src.starts_with("http://")
            || src.starts_with("https://")
        {
            return src.to_string();
        }

        // 协议相对 URL
        if src.starts_with("//") {
            return format!("https:{}", src);
        }

        // 其他情况需要根据当前页面 URL 解析
        let page_url = dom.url().map(|u| u.to_string()).unwrap_or_default();

        if page_url.is_empty() {
            return src.to_string();
        }

        // 根相对路径：/path/to/image.png
        if src.starts_with('/') {
            if let Ok(parsed) = url::Url::parse(&page_url) {
                if let Some(host) = parsed.host_str() {
                    let scheme = parsed.scheme();
                    return format!("{}://{}{}", scheme, host, src);
                }
            }
        }

        // 相对路径：基于当前 URL 的目录解析
        if let Ok(base) = url::Url::parse(&page_url) {
            if let Ok(resolved) = base.join(src) {
                return resolved.to_string();
            }
        }

        // fallback：原样返回
        src.to_string()
    }

    fn render_dom(&mut self) {
        // 如果 taffy 为空（没有布局节点），走简单的 fallback 渲染
        if self.taffy.is_empty() {
            warn!("Taffy 布局为空，使用简单渲染回退");
            self.render_simple();
            return;
        }

        // Build node_index_cache: iterate all descendants once
        debug!("构建节点索引缓存...");
        self.node_index_cache.clear();
        for node in self.dom.inner_document().descendants() {
            if let Some(idx) = self.dom.index_of_node(&node) {
                if let Some(id) = self.dom.node_id_of_node(&node) {
                    self.node_index_cache.insert(id, idx);
                }
            }
        }
        debug!(
            "节点索引缓存构建完成，共 {} 项",
            self.node_index_cache.len()
        );

        // 使用 Taffy 布局结果遍历渲染
        let doc = self.dom.inner_document();
        // 从 document 的子节点开始遍历（跳过 Document 节点本身）
        let mut count = 0;
        for child in doc.children() {
            self.render_tree_with_taffy(&child);
            count += 1;
        }
        debug!("渲染树遍历完成，共遍历 {} 个直接子节点", count);
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
                            let absolute_url =
                                TaffyRenderer::resolve_image_url_internal(&url, self.dom);
                            let pixmap = self.load_image(&absolute_url);
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
                            } else {
                                // 图片加载失败，显示占位符
                                self.render_img_placeholder(20.0, *current_y, 120.0, 120.0);
                            }
                        } else {
                            // 没有 src 属性，显示占位符
                            self.render_img_placeholder(20.0, *current_y, 120.0, 120.0);
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
    /// 渲染子树到指定的 Painter（支持并行渲染）
    fn render_tree_with_taffy(&mut self, node_ref: &NodeRef) {
        // 直接渲染到 painter 的 pixmap
        self.render_tree_with_taffy_inner(node_ref);
    }

    /// 并行渲染优化入口：渲染子树到指定的 pixmap。
    ///
    /// 当前阶段为占位实现（`#[allow(dead_code)]` 保留供未来使用）。
    /// 真正的并行渲染需要重构 `TaffyRenderer` 的 `&mut self` 借用模式，
    /// 例如将 `painter` 和 `taffy` 拆分为独立 `RefCell` 或使用 arcana，
    /// 以便在多个线程中安全渲染子树到不同的 Pixmap 上。
    ///
    /// 当前简化实现：直接调用 `render_tree_with_taffy_inner`，不切换 pixmap。
    /// 这意味着目标 pixmap 参数 `_target` 当前被忽略，渲染结果汇集到主 pixmap。
    ///
    /// # 未来优化方向
    /// - 将 UI 树分割为独立层（如 backdrop、content、overlay）
    /// - 使用 `rayon` 或 `crossbeam` 对独立子树并行渲染
    /// - 每个线程持有独立的临 Pixmap，最后合成
    #[allow(dead_code)]
    fn render_tree_with_taffy_on_painter(&mut self, node_ref: &NodeRef, _target: &mut Pixmap) {
        // 当前占位实现：直接渲染到主 painter（忽略 target 参数）
        // TODO: 真正的并行渲染实现
        self.render_tree_with_taffy_inner(node_ref);
    }

    /// 实际的渲染逻辑（被 render_tree_with_taffy 和并行版本共用）
    /// 对子节点按 z-index 排序后再递归渲染
    fn render_children_sorted_by_z_index(&mut self, node_ref: &NodeRef) {
        let mut children: Vec<NodeRef> = node_ref.children().collect();
        // 按 z-index 稳定排序（默认 0，越小越靠后渲染即越底层）
        children.sort_by(|a, b| {
            let a_z = self.get_z_index(a).unwrap_or(0);
            let b_z = self.get_z_index(b).unwrap_or(0);
            a_z.cmp(&b_z)
        });
        for child in &children {
            self.render_tree_with_taffy(child);
        }
    }

    /// 获取节点的 z-index 值
    fn get_z_index(&self, node_ref: &NodeRef) -> Option<i32> {
        let dom_idx = self.find_dom_index(node_ref)?;
        let layout = self.taffy.get_layout(dom_idx)?;
        Some(layout.z_index)
    }

    fn render_tree_with_taffy_inner(&mut self, node_ref: &NodeRef) {
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

                    // 跳过不可见节点（宽或高为 0）
                    if w <= 0.0 || h <= 0.0 {
                        // 对子节点按 z-index 排序后递归
                        let mut invisible_children: Vec<NodeRef> = node_ref.children().collect();
                        invisible_children.sort_by(|a, b| {
                            let a_z = self.get_z_index(a).unwrap_or(0);
                            let b_z = self.get_z_index(b).unwrap_or(0);
                            a_z.cmp(&b_z)
                        });
                        for child in &invisible_children {
                            self.render_tree_with_taffy(child);
                        }
                        return;
                    }

                    // 应用滚动偏移：元素的视口 y = 布局 y - 滚动偏移
                    // position: fixed/sticky 元素不受滚动影响（锚定在视口）
                    let is_fixed_or_sticky =
                        layout.position_type == crate::renderer::taffy_layout::PositionType::Fixed;
                    let vy = if is_fixed_or_sticky {
                        y
                    } else {
                        y - self.scroll_offset_y
                    };

                    // 应用 CSS transform: translate() 偏移
                    let (tx, ty) = (layout.transform_dx, layout.transform_dy);
                    let render_x = x + tx;
                    let render_y = vy + ty;

                    // opacity 处理：透明元素完全跳过
                    if layout.opacity <= 0.0 {
                        self.render_children_sorted_by_z_index(node_ref);
                        return;
                    }
                    // opacity < 1.0 时，现有的 Painter API 不支持全局 alpha 混合
                    // 但 tiny-skia 的 fill_rect 支持颜色本身的 alpha 值
                    // 当前 painter 不支持独立透明度——需要在颜色级别处理
                    // 注意：painter 的所有绘制 API 已经通过 color 的 alpha 通道支持透明度
                    // 这里标记未来可以增强：将 opacity 应用到颜色的 alpha 通道
                    if layout.opacity < 1.0 && layout.opacity > 0.0 {
                        // 透明度将通过颜色 alpha 处理（已经支持）
                    }

                    // 跳过完全在视口上方或下方的元素（提高性能）
                    let (_vp_w, vp_h) = (
                        self.taffy.get_viewport_width(),
                        self.taffy.get_viewport_height(),
                    );
                    if vy + h < 0.0 || vy > vp_h {
                        // 完全不可见，但子节点可能跨越可见区域，仍需递归
                        self.render_children_sorted_by_z_index(node_ref);
                        return;
                    }

                    let is_hovered = Some(dom_idx) == self.taffy.hovered_node;

                    // 渲染背景、边框、box-shadow、文本（使用 render_x/render_y 应用 transform）
                    if let Some(bg) = &layout.background {
                        let br = layout.border_radius;
                        if br > 0.0 && w > 0.0 && h > 0.0 {
                            self.painter
                                .draw_rounded_rect(render_x, render_y, w, h, br, bg);
                        } else if br < 0.0 && w > 0.0 && h > 0.0 {
                            let r = w.min(h) / 2.0;
                            self.painter
                                .draw_rounded_rect(render_x, render_y, w, h, r, bg);
                        } else {
                            self.painter.draw_rect(render_x, render_y, w, h, bg);
                        }
                    }
                    if let Some(shadow) = &layout.box_shadow {
                        self.render_box_shadow_from_str(shadow, render_x, render_y, w, h);
                    } else {
                        self.render_box_shadow_for_element(&tag_name, render_x, render_y, w, h);
                    }
                    self.render_background_image(&tag_name, render_x, render_y, w, h);
                    if let Some(bc) = &layout.border_color {
                        let bw = self.get_border_width(&tag_name, &layout);
                        if bw > 0.0 {
                            let br = layout.border_radius;
                            if br > 0.0 {
                                self.painter
                                    .draw_rounded_border(render_x, render_y, w, h, br, bw, bc);
                            } else {
                                self.painter
                                    .draw_rect_border(render_x, render_y, w, h, bw, bc);
                            }
                        }
                    }
                    self.render_element_box(&tag_name, render_x, render_y, w, h, Some(node_ref));
                    if is_hovered {
                        let highlight = layout
                            .border_color
                            .as_ref()
                            .unwrap_or(&Color::from_hex("#4A90D9"))
                            .clone();
                        self.painter
                            .draw_rect_border(render_x, render_y, w, h, 2.0, &highlight);
                    }
                    if tag_name == "img" {
                        self.render_img_element(render_x, render_y, w, h, node_ref);
                    }
                    if tag_name == "iframe" {
                        self.render_iframe_element(render_x, render_y, w, h, node_ref);
                    }
                    if tag_name != "style" && tag_name != "script" && tag_name != "head" {
                        let text_content = collect_text(node_ref);
                        if !text_content.trim().is_empty() {
                            let font_size = layout.font_size.max(12.0);
                            let actual_font_size = if h > 0.0 {
                                font_size.min(h * 0.7).max(8.0)
                            } else {
                                font_size
                            };
                            let default_color = Color::from_hex("#333333");
                            let font_color = layout.font_color.as_ref().unwrap_or(&default_color);
                            let padding_x = 10.0;
                            let text_y = if h > actual_font_size {
                                render_y + (h - actual_font_size) / 2.0 + actual_font_size
                            } else {
                                render_y + 2.0 + actual_font_size
                            };
                            self.render_text_at_weight(
                                &text_content,
                                render_x + padding_x,
                                text_y,
                                w - padding_x * 2.0,
                                actual_font_size,
                                font_color,
                                layout.font_weight,
                            );
                            self.render_text_decoration(
                                &text_content,
                                render_x,
                                render_y,
                                w,
                                h,
                                font_size,
                                font_color,
                                &layout.text_decoration,
                            );
                        }
                    }

                    // 表格特有样式：<th> 默认加粗和灰底
                    if tag_name == "th" && layout.background.is_none() {
                        self.painter.draw_rect(
                            render_x,
                            render_y,
                            w,
                            h,
                            &Color::from_hex("#F0F0F0"),
                        );
                        self.painter.draw_rect_border(
                            render_x,
                            render_y,
                            w,
                            h,
                            1.0,
                            &Color::from_hex("#DDDDDD"),
                        );
                    }
                    // <td> 默认细线边框
                    if tag_name == "td" {
                        self.painter.draw_rect_border(
                            render_x,
                            render_y,
                            w,
                            h,
                            1.0,
                            &Color::from_hex("#DDDDDD"),
                        );
                    }
                    // <table> 外边框
                    if tag_name == "table" {
                        self.painter.draw_rect_border(
                            render_x,
                            render_y,
                            w,
                            h,
                            1.0,
                            &Color::from_hex("#CCCCCC"),
                        );
                    }

                    // 递归子节点前应用 overflow: hidden 裁剪
                    if layout.overflow_x == "hidden" || layout.overflow_y == "hidden" {
                        self.painter.set_clip(x, y, w, h);
                    }
                    // 对子节点按 z-index 排序后递归
                    self.render_children_sorted_by_z_index(node_ref);
                    if layout.overflow_x == "hidden" || layout.overflow_y == "hidden" {
                        self.painter.clear_clip();
                    }
                }
            }
        } else {
            // 文本节点、文档节点等 - 直接递归子节点
            for child in node_ref.children() {
                self.render_tree_with_taffy(&child);
            }
        }
    }

    /// 通过 NodeRef 查找 DOM 索引（O(1) cache lookup，使用 64 位稳定 ID）
    fn find_dom_index(&self, node_ref: &NodeRef) -> Option<usize> {
        let id = self.dom.node_id_of_node(node_ref)?;
        self.node_index_cache.get(&id).copied()
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
            "select" => {
                self.render_select_element(x, y, w, h, node_ref);
            }
            "option" => {
                // option 元素：使用浅底色
                let opt_bg = Color::from_hex("#FAFAFA");
                self.painter.draw_rect(x, y, w, h, &opt_bg);
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

    /// 实际渲染图片元素（支持绝对 URL、协议相对 URL、相对路径、base64 图片）
    /// 图片加载失败时显示占位符
    fn render_img_element(&mut self, x: f32, y: f32, w: f32, h: f32, element: &NodeRef) {
        if let Some(el) = element.as_element() {
            let src = el.attributes.borrow().get("src").map(|s| s.to_string());
            if let Some(url) = src {
                // 解析相对路径、协议相对 URL 等
                let absolute_url = TaffyRenderer::resolve_image_url_internal(&url, self.dom);
                let pixmap = self.load_image(&absolute_url);
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
                } else {
                    // 图片加载失败，显示占位符
                    self.render_img_placeholder(x, y, w, h);
                }
            } else {
                // 没有 src 属性，显示占位符
                self.render_img_placeholder(x, y, w, h);
            }
        } else {
            // 不是元素节点，显示占位符
            self.render_img_placeholder(x, y, w, h);
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

    /// 渲染 <iframe> 嵌入内容
    ///
    /// 从 src 属性提取 URL，加载子文档，在 iframe 矩形区域内渲染。
    /// 显示样式类似嵌套浏览上下文：白色背景 + 深色边框 + 子文档内容。
    fn render_iframe_element(&mut self, x: f32, y: f32, w: f32, h: f32, element: &NodeRef) {
        // 先绘制 iframe 容器边框和背景
        self.painter
            .draw_rounded_rect(x, y, w, h, 2.0, &Color::from_hex("#FFFFFF"));
        self.painter
            .draw_rounded_border(x, y, w, h, 2.0, 1.5, &Color::from_hex("#888888"));

        if w <= 20.0 || h <= 20.0 {
            return; // 尺寸太小，无法显示有意义的内容
        }

        // 获取 src 属性
        let src = element
            .as_element()
            .and_then(|el| el.attributes.borrow().get("src").map(|s| s.to_string()))
            .unwrap_or_default();

        if src.is_empty() || src == "about:blank" {
            // 空白 iframe：在矩形中显示 "about:blank" 提示
            let padding = 6.0;
            self.render_text_at(
                "about:blank",
                x + padding,
                y + h / 2.0 - 6.0,
                w - padding * 2.0,
                12.0,
                &Color::from_hex("#999999"),
            );
            return;
        }

        // 解析绝对 URL
        let absolute_url = TaffyRenderer::resolve_image_url_internal(&src, self.dom);

        // 加载子文档（复用 load_document 函数）
        let child_doc = crate::browser_process::host::load_document(&absolute_url);

        match child_doc {
            Ok(doc) => {
                let child_dom = doc.get_dom();

                // 对子文档进行布局
                let inner_w = w - 4.0; // 减去 padding/border
                let inner_h = h - 4.0;
                if inner_w <= 0.0 || inner_h <= 0.0 {
                    return;
                }

                let mut child_taffy = crate::renderer::taffy_layout::TaffyLayoutEngine::new(
                    inner_w.max(1.0),
                    inner_h.max(1.0),
                );

                // 从子 DOM 提取 CSS
                let css_text = crate::renderer::renderer::extract_style_tags(child_dom);
                if !css_text.is_empty() {
                    let rules = crate::css_engine::parse_css_rules(&css_text);
                    let style_map =
                        crate::css_engine::rules_to_style_map(&rules, child_dom.inner_document());
                    child_taffy.set_style_map(style_map);
                }

                let _ = child_taffy.compute(child_dom);

                // 使用子渲染器渲染子文档到临时 pixmap
                if let Some(mut child_painter) =
                    crate::renderer::painter::Painter::new(inner_w as u32, inner_h as u32)
                {
                    // 设置子文档背景
                    let child_bg = child_taffy
                        .find_by_tag("body")
                        .iter()
                        .find_map(|n| n.background.clone())
                        .or_else(|| {
                            child_taffy
                                .find_by_tag("html")
                                .iter()
                                .find_map(|n| n.background.clone())
                        })
                        .unwrap_or(crate::css::values::Color::WHITE);
                    child_painter.set_background(child_bg);
                    child_painter.paint();

                    // 使用 TaffyRenderer 渲染子文档
                    if !child_taffy.is_empty() {
                        // child_dom 是 &DomWrapper，已经可用
                        // 需要在子文档中渲染
                        let mut child_renderer = TaffyRenderer {
                            painter: &mut child_painter,
                            taffy: &child_taffy,
                            dom: child_dom,
                            node_index_cache: std::collections::HashMap::new(),
                            scroll_offset_y: 0.0,
                        };
                        child_renderer.render_dom();
                    }

                    // 将子文档的 pixmap 绘制到主画布的 iframe 区域
                    let child_pixmap = child_painter.pixmap_mut();
                    // 使用 draw_pixmap 在 (x+2, y+2) 位置绘制
                    self.painter.pixmap_mut().draw_pixmap(
                        (x + 2.0) as i32,
                        (y + 2.0) as i32,
                        child_pixmap.as_ref(),
                        &tiny_skia::PixmapPaint::default(),
                        tiny_skia::Transform::identity(),
                        None,
                    );
                }
            }
            Err(e) => {
                // 加载失败：显示错误信息
                let padding = 6.0;
                let err_msg = format!("iframe 加载失败: {}", e);
                // 在 iframe 区域顶部显示浅红色背景的错误消息
                self.painter.draw_rect(
                    x + 2.0,
                    y + 2.0,
                    w - 4.0,
                    20.0,
                    &Color::from_hex("#FFF0F0"),
                );
                self.render_text_at(
                    &err_msg,
                    x + padding,
                    y + padding + 2.0,
                    w - padding * 2.0,
                    12.0,
                    &Color::from_hex("#CC3333"),
                );
            }
        }
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

        // 读取 type/value/placeholder 属性
        let input_type = node_ref
            .and_then(|nr| {
                nr.as_element()
                    .and_then(|el| el.attributes.borrow().get("type").map(|s| s.to_string()))
            })
            .unwrap_or_default();
        let value = node_ref
            .and_then(|nr| {
                nr.as_element()
                    .and_then(|el| el.attributes.borrow().get("value").map(|s| s.to_string()))
            })
            .unwrap_or_default();
        let placeholder = node_ref
            .and_then(|nr| {
                nr.as_element().and_then(|el| {
                    el.attributes
                        .borrow()
                        .get("placeholder")
                        .map(|s| s.to_string())
                })
            })
            .unwrap_or_default();

        let is_password = input_type == "password";
        let font_size = (h * 0.55).max(12.0).min(16.0);
        let padding = 8.0;
        let text_color = &Color::from_hex("#333333");
        let placeholder_color = &Color::from_hex("#AAAAAA");

        // 渲染文本（左对齐，垂直居中）
        let text_x = x + padding;
        let text_y = y + (h - font_size) / 2.0;
        let max_width = w - padding * 2.0;

        if !value.is_empty() {
            let display_text = if is_password {
                "•".repeat(value.len())
            } else {
                value.clone()
            };
            self.render_text_at(
                &display_text,
                text_x,
                text_y,
                max_width,
                font_size,
                text_color,
            );
        } else if !placeholder.is_empty() && !is_focused {
            self.render_text_at(
                &placeholder,
                text_x,
                text_y,
                max_width,
                font_size,
                placeholder_color,
            );
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
                let id = self.dom.node_id_of_node(nr)?;
                self.node_index_cache.get(&id).copied().and_then(|dom_idx| {
                    if taffy_focused == Some(dom_idx) {
                        Some(true)
                    } else {
                        None
                    }
                })
            })
            .unwrap_or(false)
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

    /// 渲染 <select> 下拉框元素（按钮样式 + 下拉三角箭头 + 显示选中项文本）
    fn render_select_element(
        &mut self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        node_ref: Option<&NodeRef>,
    ) {
        // 白色背景 + 灰色边框 (类似按钮)
        let bg = Color::from_hex("#FFFFFF");
        let border = Color::from_hex("#CCCCCC");
        self.painter.draw_rounded_rect(x, y, w, h, 4.0, &bg);
        self.painter
            .draw_rounded_border(x, y, w, h, 4.0, 1.0, &border);

        // 右侧下拉三角箭头 (▼)
        let arrow_size = 8.0;
        let arrow_x = x + w - 18.0;
        let arrow_y = y + (h - arrow_size) / 2.0;
        // 绘制三角: 三个点组成的三角形
        let color = Color::from_hex("#666666");
        self.painter
            .draw_rect(arrow_x, arrow_y, arrow_size, 2.0, &color);
        self.painter
            .draw_rect(arrow_x + 2.0, arrow_y + 3.0, arrow_size - 4.0, 2.0, &color);
        self.painter
            .draw_rect(arrow_x + 4.0, arrow_y + 6.0, arrow_size - 8.0, 2.0, &color);

        // 读取选中项的文本
        // 优先使用 value 属性，否则找第一个 selected option
        let selected_text = node_ref
            .and_then(|nr| {
                let el = nr.as_element()?;
                let attrs = el.attributes.borrow();
                // 优先用 value 属性
                if let Some(val) = attrs.get("value") {
                    if !val.is_empty() {
                        return Some(val.to_string());
                    }
                }
                // 找第一个 option 子元素的文本
                for child in nr.children() {
                    if let Some(child_el) = child.as_element() {
                        if child_el.name.local.as_ref() == "option" {
                            // 检查 selected 属性
                            let child_attrs = child_el.attributes.borrow();
                            if child_attrs.get("selected").is_some() {
                                return Some(collect_text(&child));
                            }
                        }
                    }
                }
                // 找到第一个 option 的文本
                for child in nr.children() {
                    if let Some(child_el) = child.as_element() {
                        if child_el.name.local.as_ref() == "option" {
                            return Some(collect_text(&child));
                        }
                    }
                }
                None
            })
            .unwrap_or_default();

        // 显示文本（左对齐）
        let padding = 8.0;
        let txt_x = x + padding;
        let txt_y = y + (h - 14.0) / 2.0 + 12.0;
        let txt_w = w - padding * 2.0 - 24.0; // 为箭头留空间
        let display_text = if selected_text.is_empty() {
            "请选择..."
        } else {
            &selected_text
        };
        self.render_text_at(
            display_text,
            txt_x,
            txt_y,
            txt_w,
            14.0,
            &Color::from_hex("#333333"),
        );
    }

    /// 从 layout 获取边框宽度
    fn get_border_width(&self, _tag: &str, _layout: &TaffyLayoutNode) -> f32 {
        match _tag {
            "input" | "textarea" | "select" => 1.0,
            "button" => 1.0,
            "img" => 0.0,
            _ => 0.0,
        }
    }

    /// 直接解析 box-shadow 字符串并渲染
    fn render_box_shadow_from_str(&mut self, shadow_str: &str, x: f32, y: f32, w: f32, h: f32) {
        if let Some(shadow) = parse_box_shadow_from_style(shadow_str) {
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
    ///
    /// 已支持：
    /// - `http://` / `https://` 网络图片
    /// - `//` 协议相对 URL（自动补充 `https:`）
    /// - `data:image/...` base64 编码图片
    /// - 本地文件路径
    fn load_image(&self, url: &str) -> Option<tiny_skia::Pixmap> {
        let cache = global_image_cache();
        cache.get(url)
    }

    /// 渲染背景图片（支持 background-size、sprite 裁剪）
    fn render_background_image(&mut self, _tag: &str, x: f32, y: f32, w: f32, h: f32) {
        let nodes = self.taffy.find_by_tag(_tag);
        for layout in nodes {
            if let Some(bg_image) = &layout.background_image {
                // 解析背景图片 URL（支持协议相对、相对路径等）
                let resolved_bg = TaffyRenderer::resolve_image_url_internal(bg_image, self.dom);
                let cache = global_image_cache();
                let bg_x = layout.bg_position_x as i32;
                let bg_y = layout.bg_position_y as i32;

                // sprite 裁剪（background-position）
                if bg_x != 0 || bg_y != 0 {
                    if let Some(cropped) =
                        ImageCache::crop_sprite(&resolved_bg, bg_x, bg_y, w as u32, h as u32)
                    {
                        self.painter.pixmap_mut().draw_pixmap(
                            x as i32,
                            y as i32,
                            cropped.as_ref(),
                            &tiny_skia::PixmapPaint::default(),
                            tiny_skia::Transform::identity(),
                            None,
                        );
                        return;
                    }
                }

                if let Some(pixmap) = cache.get(&resolved_bg) {
                    let img_w = pixmap.width() as f32;
                    let img_h = pixmap.height() as f32;

                    // 根据 background-size 决定缩放方式
                    match layout.background_size.as_str() {
                        "cover" => {
                            let scale = (w / img_w).max(h / img_h);
                            let scaled_w = img_w * scale;
                            let scaled_h = img_h * scale;
                            let ox = (w - scaled_w) / 2.0;
                            let oy = (h - scaled_h) / 2.0;
                            let transform = tiny_skia::Transform::from_scale(scale, scale)
                                .post_translate(x + ox, y + oy);
                            self.painter.pixmap_mut().draw_pixmap(
                                (x + ox) as i32,
                                (y + oy) as i32,
                                pixmap.as_ref(),
                                &tiny_skia::PixmapPaint::default(),
                                transform,
                                None,
                            );
                        }
                        "contain" => {
                            let scale = (w / img_w).min(h / img_h);
                            let scaled_w = img_w * scale;
                            let scaled_h = img_h * scale;
                            let ox = (w - scaled_w) / 2.0;
                            let oy = (h - scaled_h) / 2.0;
                            let transform = tiny_skia::Transform::from_scale(scale, scale)
                                .post_translate(x + ox, y + oy);
                            self.painter.pixmap_mut().draw_pixmap(
                                (x + ox) as i32,
                                (y + oy) as i32,
                                pixmap.as_ref(),
                                &tiny_skia::PixmapPaint::default(),
                                transform,
                                None,
                            );
                        }
                        _ => {
                            // auto：等比缩放，只缩小不放大
                            let scale = (w / img_w).min(h / img_h).min(1.0);
                            if scale < 1.0 {
                                let transform = tiny_skia::Transform::from_scale(scale, scale)
                                    .post_translate(x, y);
                                self.painter.pixmap_mut().draw_pixmap(
                                    x as i32,
                                    y as i32,
                                    pixmap.as_ref(),
                                    &tiny_skia::PixmapPaint::default(),
                                    transform,
                                    None,
                                );
                            } else {
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
        }
    }

    /// 渲染 text-decoration（下划线/删除线）
    fn render_text_decoration(
        &mut self,
        _text: &str,
        x: f32,
        y: f32,
        w: f32,
        _h: f32,
        font_size: f32,
        color: &Color,
        decoration: &str,
    ) {
        let padding = 10.0;
        let text_x = x + padding;
        let text_y = y + padding;
        let max_width = (w - padding * 2.0).max(0.0);
        let line_y = text_y + font_size * 0.15; // 下划线位置（略高于基线）
        let strike_y = text_y + font_size * 0.45; // 删除线位置（中间）

        match decoration {
            "underline" => {
                self.painter
                    .draw_rect(text_x, line_y, max_width, 1.0, color);
            }
            "line-through" => {
                self.painter
                    .draw_rect(text_x, strike_y, max_width, 1.0, color);
            }
            _ => {}
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

    /// 将字形 alpha 蒙版渲染到 pixmap
    ///
    /// 使用 tiny-skia 的 `PixmapPaint` + `draw_pixmap` 进行高效 alpha 混合，
    /// 比起逐像素手动操作有更好的性能。
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

        // 字形蒙版通常是灰度图（alpha 通道），需要将其着色为目标颜色
        // 创建一个 RGBA 临时 Pixmap，从 alpha_data 构建颜色像素
        let Some(mut glyph_pixmap) = tiny_skia::Pixmap::new(width, height) else {
            return;
        };

        let text_rgba = color.to_rgba();
        let glyph_data = glyph_pixmap.data_mut();

        for (i, &alpha) in alpha_data.iter().enumerate() {
            if i * 4 + 3 >= glyph_data.len() {
                break;
            }
            if alpha == 0 {
                glyph_data[i * 4] = 0;
                glyph_data[i * 4 + 1] = 0;
                glyph_data[i * 4 + 2] = 0;
                glyph_data[i * 4 + 3] = 0;
            } else {
                // 将 alpha 值与目标颜色相乘
                let a = alpha as u16;
                glyph_data[i * 4] = (text_rgba[0] as u16 * a / 255) as u8;
                glyph_data[i * 4 + 1] = (text_rgba[1] as u16 * a / 255) as u8;
                glyph_data[i * 4 + 2] = (text_rgba[2] as u16 * a / 255) as u8;
                glyph_data[i * 4 + 3] = alpha;
            }
        }

        // 使用 SourceOver blend mode 将字形合成到主 pixmap
        self.painter.pixmap_mut().draw_pixmap(
            x as i32,
            y as i32,
            glyph_pixmap.as_ref(),
            &tiny_skia::PixmapPaint {
                blend_mode: tiny_skia::BlendMode::SourceOver,
                ..tiny_skia::PixmapPaint::default()
            },
            tiny_skia::Transform::identity(),
            None,
        );
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

    /// 解析图片 URL 为绝对 URL
    ///
    /// 处理以下情形：
    /// - `//host.com/path` → `https://host.com/path`（协议相对）
    /// - `/path/img.png` → `https://domain/path/img.png`（根相对）
    /// - `img.png` → `https://domain/base/img.png`（相对路径）
    /// - `http://...` / `https://...` → 不变
    /// - `data:image/...` → 不变
    pub fn resolve_image_url(&self, src: &str) -> String {
        Self::resolve_image_url_static(src, self.document.as_ref())
    }

    /// 解析图片 URL 为绝对 URL（静态版，可无 self 调用）
    pub fn resolve_image_url_static(src: &str, document: Option<&Document>) -> String {
        // base64 和已有绝对协议的不处理
        if src.starts_with("data:image/")
            || src.starts_with("http://")
            || src.starts_with("https://")
        {
            return src.to_string();
        }

        // 协议相对 URL
        if src.starts_with("//") {
            return format!("https:{}", src);
        }

        // 其他情况需要根据当前页面 URL 解析
        let page_url = document
            .and_then(|d| {
                let u = &d.url;
                if u.is_empty() || u == "about:blank" {
                    None
                } else {
                    Some(u.clone())
                }
            })
            .unwrap_or_default();

        if page_url.is_empty() {
            return src.to_string();
        }

        // 根相对路径：/path/to/image.png
        if src.starts_with('/') {
            if let Ok(parsed) = url::Url::parse(&page_url) {
                if let Some(host) = parsed.host_str() {
                    let scheme = parsed.scheme();
                    return format!("{}://{}{}", scheme, host, src);
                }
            }
        }

        // 相对路径：基于当前 URL 的目录解析
        if let Ok(base) = url::Url::parse(&page_url) {
            if let Ok(resolved) = base.join(src) {
                return resolved.to_string();
            }
        }

        // fallback：原样返回
        src.to_string()
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
        assert!(cache.len() == 0);
    }

    #[test]
    fn test_render_document_applies_body_background_color() {
        // 验证 body background-color: #ff0000 被正确应用为画布背景色
        let html = r#"<html><head><style>body { background-color: #ff0000; }</style></head><body><p>red bg</p></body></html>"#;
        let dom = crate::DomWrapper::from_html(html, None);
        let css_text = extract_style_tags(&dom);
        assert!(!css_text.is_empty(), "CSS should be extracted");
        let rules = crate::css_engine::parse_css_rules(&css_text);
        assert!(!rules.is_empty(), "CSS rules should be parsed");

        let doc = crate::browser::Document::from_html(html, "test://");
        let mut renderer = Renderer::new(100, 100);
        let doc_opt = Some(doc);
        let result = renderer.render(&doc_opt);
        assert!(result.is_ok(), "render should succeed");

        // 解码 PNG 并检查中心像素是否为红色
        let png = result.unwrap();
        let img = image::load_from_memory(&png).expect("valid png");
        let rgba = img.to_rgba8();
        let (w, h) = rgba.dimensions();
        assert_eq!(w, 100);
        assert_eq!(h, 100);

        // 检查中心像素 (50, 50) 是否为红色 (255, 0, 0, 255)
        let center = rgba.get_pixel(50, 50);
        assert_eq!(center[0], 255, "red channel");
        assert_eq!(center[1], 0, "green channel should be 0");
        assert_eq!(center[2], 0, "blue channel should be 0");
        assert_eq!(center[3], 255, "alpha channel");
    }

    #[test]
    fn test_render_document_white_background_when_no_body_bg() {
        // 没有设置背景色时应该是白色
        let html = r#"<html><body><p>default white</p></body></html>"#;
        let doc = crate::browser::Document::from_html(html, "test://");
        let mut renderer = Renderer::new(100, 100);
        let doc_opt = Some(doc);
        let result = renderer.render(&doc_opt);
        assert!(result.is_ok());

        let png = result.unwrap();
        let img = image::load_from_memory(&png).expect("valid png");
        let rgba = img.to_rgba8();
        let center = rgba.get_pixel(50, 50);
        assert_eq!(center[0], 255, "should be white");
        assert_eq!(center[1], 255, "should be white");
        assert_eq!(center[2], 255, "should be white");
    }

    #[test]
    fn test_hit_test_link_detects_anchor() {
        // 验证 hit_test_link 能正确找到 <a> 标签的 href
        let html =
            r#"<html><body><a href="https://example.com"><span>link</span></a></body></html>"#;
        let doc = crate::browser::Document::from_html(html, "test://");
        let mut renderer = Renderer::new(800, 600);
        renderer.set_document(doc);
        // 使用 render_to_png，不会克隆 Document（DomWrapper::clone 会创建空树）
        let _ = renderer.render_to_png();

        let dom = renderer.document().as_ref().unwrap().get_dom();
        let href = renderer.hit_test_link(50.0, 50.0, dom);
        assert!(href.is_some(), "should find href on <a>");
        assert_eq!(href.unwrap(), "https://example.com");
    }

    #[test]
    fn test_hit_test_link_returns_none_on_non_link() {
        let html = r#"<html><body><p>not a link</p></body></html>"#;
        let doc = crate::browser::Document::from_html(html, "test://");
        let mut renderer = Renderer::new(800, 600);
        renderer.set_document(doc);
        let _ = renderer.render_to_png();

        let dom = renderer.document().as_ref().unwrap().get_dom();
        let href = renderer.hit_test_link(50.0, 50.0, dom);
        assert!(href.is_none(), "<p> should not have href");
    }

    #[test]
    fn test_hit_test_link_click_on_span_inside_anchor() {
        // 点击 <a> 内部的 <span> 也应该返回链接
        let html =
            r#"<html><body><a href="https://example.com"><span>click me</span></a></body></html>"#;
        let doc = crate::browser::Document::from_html(html, "test://");
        let mut renderer = Renderer::new(800, 600);
        renderer.set_document(doc);
        let _ = renderer.render_to_png();

        let dom = renderer.document().as_ref().unwrap().get_dom();
        let href = renderer.hit_test_link(50.0, 50.0, dom);
        assert!(href.is_some(), "clicking span inside a should return href");
        assert_eq!(href.unwrap(), "https://example.com");
    }
}
