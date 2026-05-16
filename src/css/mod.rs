//! CSS 模块 - CSS 值类型
//!
//! 提供 CSS 属性值类型（颜色、长度等），选择器功能由 kuchiki 提供

pub mod values;

pub use values::{Color, Length, LengthUnit, PropertyValue};
