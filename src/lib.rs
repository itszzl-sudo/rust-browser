//! Rust Browser - 基于 obscura + taffy + kuchiki 的浏览器引擎
//!
//! 这是一个轻量级的浏览器渲染引擎，使用 Rust 实现。
//!
//! # 特性
//!
//! - obscura-net 网络请求（超时、Cookie、gzip/brotli解压）
//! - kuchiki DOM 解析和 CSS 选择器
//! - Taffy 布局引擎
//! - tiny-skia 像素渲染
//! - egui 界面层（崩溃隔离）
//! - 多标签页支持
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
pub mod browser_process;
pub mod css;
pub mod dom_wrapper;
pub mod mojo;
pub mod network;
pub mod renderer;
pub mod renderer_process;
pub mod task_queue;

pub mod html;

#[allow(deprecated)]
pub mod dom;

pub use browser::{Browser, BrowserEngine, BrowserError, Document, DEFAULT_HOME_URL};
pub use browser::{Tab, TabManager};
pub use css::values::Color;
#[allow(deprecated)]
pub use dom::node::DomTree;
pub use dom_wrapper::DomWrapper;
pub use html::parser::HtmlParser;
pub use network::NetworkClient;
pub use renderer::Renderer;
