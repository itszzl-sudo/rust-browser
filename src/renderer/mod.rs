//! Renderer 模块 - 页面渲染
//!
//! 整合 Taffy 布局和 Vello 渲染

pub mod border;
pub mod context;
pub mod cursor;
pub mod image_cache;
pub mod layout;
pub mod painter;
pub mod taffy_layout;
pub mod text;
pub mod vello;

pub use border::draw_box_shadow;
pub use context::RenderContext;
pub use layout::{LayoutEngine, LayoutResult};
pub use painter::Painter;
pub use renderer::{extract_style_tags, BoxShadowValue, Renderer};
pub use taffy_layout::TaffyLayoutEngine;
pub use text::TextRenderer;
pub use vello::{VelloError, VelloRenderer};

mod renderer;
