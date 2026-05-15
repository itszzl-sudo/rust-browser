//! DOM 模块 - 文档对象模型
//!
//! 处理 HTML 文档的树形结构

pub mod node;
pub mod visitor;

pub use node::{DomNode, NodeType};
