//! Painter - 使用 tiny-skia 绘制
//!
//! 将布局结果绘制到像素图

use crate::css::stylesheet::ComputedStyle;
use crate::css::values::Color;
use crate::dom::node::{DomNode, NodeType};
use crate::renderer::layout::{LayoutEngine, LayoutResult};
use log::{debug, trace};
use tiny_skia::{IntRect, Paint, Pixmap, Rect, Transform};

/// 绘制器
pub struct Painter {
    /// 像素图
    pixmap: Pixmap,
    /// 布局引擎
    layout_engine: LayoutEngine,
    /// 默认背景色
    background: Color,
}

impl Painter {
    /// 创建新的绘制器
    pub fn new(width: u32, height: u32) -> Option<Self> {
        let pixmap = Pixmap::new(width, height)?;
        let layout_engine = LayoutEngine::new();
        
        debug!("创建绘制器 ({}x{})", width, height);
        
        Some(Self {
            pixmap,
            layout_engine,
            background: Color::WHITE,
        })
    }

    /// 设置背景色
    pub fn set_background(&mut self, color: Color) {
        self.background = color;
    }

    /// 绘制 DOM 树
    pub fn paint(&mut self, nodes: &[DomNode], _styles: &[ComputedStyle]) {
        trace!("开始绘制");
        
        // 填充背景
        self.fill_background();
        
        // 获取所有布局结果
        let results = self.layout_engine.results().to_vec();
        
        // 遍历节点绘制
        for (i, node) in nodes.iter().enumerate() {
            if let Some(layout) = results.get(i) {
                self.paint_node(node, layout);
            }
        }
    }

    /// 填充背景
    fn fill_background(&mut self) {
        let color = self.background.to_rgba();
        
        self.pixmap.fill(tiny_skia::Color::from_rgba8(color[0], color[1], color[2], color[3]));
    }

    /// 绘制单个节点
    fn paint_node(&mut self, node: &DomNode, layout: &LayoutResult) {
        if layout.width <= 0.0 || layout.height <= 0.0 {
            return;
        }

        match &node.node_type {
            NodeType::Element(data) => {
                self.paint_element(data, layout);
            }
            NodeType::Text(text) => {
                self.paint_text(text, layout);
            }
            _ => {}
        }
    }

    /// 绘制元素
    fn paint_element(&mut self, data: &crate::dom::node::ElementData, layout: &LayoutResult) {
        trace!("绘制元素: {:?}", data.tag_name);
        
        // 绘制背景
        self.draw_rect(
            layout.x,
            layout.y,
            layout.width,
            layout.height,
            &Color::from_hex("#e0e0e0"),
        );
    }

    /// 绘制文本
    fn paint_text(&mut self, text: &str, layout: &LayoutResult) {
        if text.trim().is_empty() {
            return;
        }
        
        trace!("绘制文本: {} chars", text.len());
        
        // 简化文本绘制
        let mut paint = Paint::default();
        paint.set_color_rgba8(0, 0, 0, 255);
        
        // 绘制文本矩形表示
        if let Some(rect) = Rect::from_xywh(
            layout.x + 2.0,
            layout.y,
            (layout.width - 4.0).max(0.0),
            layout.height,
        ) {
            self.pixmap.fill_rect(rect, &paint, Transform::identity(), None);
        }
    }

    /// 绘制矩形
    fn draw_rect(&mut self, x: f32, y: f32, width: f32, height: f32, color: &Color) {
        let rgba = color.to_rgba();
        
        let mut paint = Paint::default();
        paint.set_color_rgba8(rgba[0], rgba[1], rgba[2], rgba[3]);
        
        if let Some(rect) = Rect::from_xywh(x, y, width, height) {
            self.pixmap.fill_rect(rect, &paint, Transform::identity(), None);
        }
    }

    /// 公开的矩形绘制方法
    pub fn paint_rect(&mut self, x: f32, y: f32, width: f32, height: f32, color: &Color) {
        self.draw_rect(x, y, width, height, color);
    }

    /// 获取布局引擎
    pub fn layout_engine(&self) -> &LayoutEngine {
        &self.layout_engine
    }

    /// 获取可变布局引擎
    pub fn layout_engine_mut(&mut self) -> &mut LayoutEngine {
        &mut self.layout_engine
    }

    /// 获取像素图
    pub fn pixmap(&self) -> &Pixmap {
        &self.pixmap
    }

    /// 获取 PNG 数据
    pub fn to_png(&self) -> Vec<u8> {
        self.pixmap.encode_png().unwrap_or_default()
    }

    /// 保存为 PNG
    pub fn save_png(&self, path: &str) -> Result<(), Box<dyn std::error::Error>> {
        self.pixmap.save_png(path)?;
        Ok(())
    }
}
