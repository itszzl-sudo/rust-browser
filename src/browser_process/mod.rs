//! Browser Process - 主进程
//!
//! 管理 UI、标签页、导航，通过 Mojo IPC 与渲染器进程通信。
//! 类似 Chrome 的 BrowserProcess。

pub mod host;
pub mod interfaces;

pub use host::BrowserProcessHost;
