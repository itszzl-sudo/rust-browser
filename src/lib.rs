//! Rust Browser - 基于 obscura + taffy + vello 的浏览器引擎
//!
//! 这是一个轻量级的浏览器渲染引擎，使用 Rust 实现。
//!
//! # 特性
//!
//! - obscura-net 网络请求
//! - obscura-dom DOM 解析
//! - Taffy 布局引擎
//! - Vello 高性能 GPU 渲染
//! - cosmic-text 文本渲染
//! - CSS 解析和应用
//! - 多标签页支持
//! - Chrome 风格 UI
//! - 截图功能
//!
//! # 示例
//!
//! ```rust
//! use rust_browser::Browser;
//!
//! let mut browser = Browser::new().unwrap();
//! browser.navigate("https://example.com").unwrap();
//! browser.screenshot("screenshot.png").unwrap();
//! ```

pub mod browser;
pub mod css;
pub mod dom;
pub mod html;
pub mod renderer;
pub mod network;
pub mod dom_wrapper;

// Re-exports for convenience
pub use browser::{Browser, BrowserEngine, Document, BrowserError, DEFAULT_HOME_URL};
pub use browser::{Tab, TabManager};
pub use browser::{ChromeUi, ChromeUiConfig, ChromeUiState, ChromeButton};
pub use css::stylesheet::{Stylesheet, Selector, Rule};
pub use css::values::Color;
pub use dom::node::DomTree;
pub use html::parser::HtmlParser;
pub use renderer::Renderer;
pub use network::NetworkClient;
pub use dom_wrapper::DomWrapper;
