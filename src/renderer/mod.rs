//! Renderer 模块 - 页面渲染
//!
//! 整合 Taffy 布局和 Vello 渲染

pub mod context;
pub mod layout;
pub mod painter;
pub mod text;
pub mod vello;
// pub mod taffy_layout; // 暂时禁用 Taffy 布局

pub use context::RenderContext;
pub use layout::{LayoutEngine, LayoutResult};
pub use painter::Painter;
pub use renderer::Renderer;
pub use text::TextRenderer;
pub use vello::{VelloError, VelloRenderer};
// pub use taffy_layout::TaffyLayoutEngine;

mod renderer;
