//! Renderer 模块 - 页面渲染
//!
//! 整合 Taffy 布局和 Vello 渲染

pub mod async_pipeline;
pub mod border;
pub mod context;
pub mod cursor;
pub mod image_cache;
pub mod image_pipeline;
pub mod layout_cache;
pub mod painter;
pub mod taffy_layout;
pub mod text;

pub use async_pipeline::AsyncPipeline;
pub use border::draw_box_shadow;
pub use context::RenderContext;
pub use image_pipeline::ImagePipeline;
pub use layout_cache::{LayoutCache, LayoutFingerprint};
pub use painter::Painter;
pub use renderer::{extract_style_tags, BoxShadowValue, Renderer};
pub use taffy_layout::TaffyLayoutEngine;
pub use text::TextRenderer;

mod renderer;
