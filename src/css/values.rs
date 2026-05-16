//! CSS 值 - CSS 属性值类型
//!
//! 定义颜色、长度、关键字等 CSS 值类型

use std::collections::HashMap;
use std::fmt;
use std::sync::OnceLock;

static COLOR_CACHE: OnceLock<HashMap<&'static str, Color>> = OnceLock::new();

fn get_color_cache() -> &'static HashMap<&'static str, Color> {
    COLOR_CACHE.get_or_init(|| {
        let mut map = HashMap::new();
        map.insert("#333333", Color::rgb(51, 51, 51));
        map.insert("#cccccc", Color::rgb(204, 204, 204));
        map.insert("#e0e0e0", Color::rgb(224, 224, 224));
        map.insert("#666666", Color::rgb(102, 102, 102));
        map.insert("#999999", Color::rgb(153, 153, 153));
        map
    })
}

/// 长度单位
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LengthUnit {
    /// 像素
    Px,
    /// 字符高度
    Em,
    /// 根字符高度
    Rem,
    /// 百分比
    Percent,
    /// 视口宽度
    Vw,
    /// 视口高度
    Vh,
    /// 点
    Pt,
    /// 厘米
    Cm,
    /// 毫米
    Mm,
    /// 英寸
    In,
}

impl Default for LengthUnit {
    fn default() -> Self {
        Self::Px
    }
}

impl fmt::Display for LengthUnit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LengthUnit::Px => write!(f, "px"),
            LengthUnit::Em => write!(f, "em"),
            LengthUnit::Rem => write!(f, "rem"),
            LengthUnit::Percent => write!(f, "%"),
            LengthUnit::Vw => write!(f, "vw"),
            LengthUnit::Vh => write!(f, "vh"),
            LengthUnit::Pt => write!(f, "pt"),
            LengthUnit::Cm => write!(f, "cm"),
            LengthUnit::Mm => write!(f, "mm"),
            LengthUnit::In => write!(f, "in"),
        }
    }
}

/// 长度值
#[derive(Debug, Clone, PartialEq)]
pub struct Length {
    /// 数值
    pub value: f64,
    /// 单位
    pub unit: LengthUnit,
}

impl Length {
    /// 创建像素值
    pub fn px(value: f64) -> Self {
        Self {
            value,
            unit: LengthUnit::Px,
        }
    }

    /// 创建百分比值
    pub fn percent(value: f64) -> Self {
        Self {
            value,
            unit: LengthUnit::Percent,
        }
    }

    /// 转换为像素（给定参考值）
    pub fn to_px(&self, reference: f64, root_font_size: f64) -> f64 {
        match self.unit {
            LengthUnit::Px => self.value,
            LengthUnit::Em => self.value * reference,
            LengthUnit::Rem => self.value * root_font_size,
            LengthUnit::Percent => self.value * reference / 100.0,
            LengthUnit::Vw => self.value * reference / 100.0, // 简化处理
            LengthUnit::Vh => self.value * reference / 100.0, // 简化处理
            LengthUnit::Pt => self.value * 1.333,              // 1pt = 1.333px
            LengthUnit::Cm => self.value * 37.795,            // 1cm = 37.795px
            LengthUnit::Mm => self.value * 3.7795,            // 1mm = 3.7795px
            LengthUnit::In => self.value * 96.0,              // 1in = 96px
        }
    }
}

impl Default for Length {
    fn default() -> Self {
        Self::px(0.0)
    }
}

/// RGBA 颜色
#[derive(Debug, Clone, PartialEq)]
pub struct Color {
    /// 红色 (0-255)
    pub r: u8,
    /// 绿色 (0-255)
    pub g: u8,
    /// 蓝色 (0-255)
    pub b: u8,
    /// 透明度 (0-255)
    pub a: u8,
}

impl Color {
    /// 从十六进制创建（使用缓存优化）
    pub fn from_hex(hex: &str) -> Self {
        let hex = hex.trim_start_matches('#');

        if let Some(cached) = get_color_cache().get(hex) {
            return cached.clone();
        }

        let color = match hex.len() {
            3 => {
                let r = u8::from_str_radix(&hex[0..1].repeat(2), 16).unwrap_or(0);
                let g = u8::from_str_radix(&hex[1..2].repeat(2), 16).unwrap_or(0);
                let b = u8::from_str_radix(&hex[2..3].repeat(2), 16).unwrap_or(0);
                Self { r, g, b, a: 255 }
            }
            4 => {
                let r = u8::from_str_radix(&hex[0..1].repeat(2), 16).unwrap_or(0);
                let g = u8::from_str_radix(&hex[1..2].repeat(2), 16).unwrap_or(0);
                let b = u8::from_str_radix(&hex[2..3].repeat(2), 16).unwrap_or(0);
                let a = u8::from_str_radix(&hex[3..4].repeat(2), 16).unwrap_or(255);
                Self { r, g, b, a }
            }
            6 => {
                let r = u8::from_str_radix(&hex[0..2], 16).unwrap_or(0);
                let g = u8::from_str_radix(&hex[2..4], 16).unwrap_or(0);
                let b = u8::from_str_radix(&hex[4..6], 16).unwrap_or(0);
                Self { r, g, b, a: 255 }
            }
            8 => {
                let r = u8::from_str_radix(&hex[0..2], 16).unwrap_or(0);
                let g = u8::from_str_radix(&hex[2..4], 16).unwrap_or(0);
                let b = u8::from_str_radix(&hex[4..6], 16).unwrap_or(0);
                let a = u8::from_str_radix(&hex[6..8], 16).unwrap_or(255);
                Self { r, g, b, a }
            }
            _ => Self::BLACK,
        };
        color
    }

    /// 从 RGB 创建
    pub fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 255 }
    }

    /// 从 RGBA 创建
    pub fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    /// 从名称创建
    pub fn from_name(name: &str) -> Option<Self> {
        match name.to_lowercase().as_str() {
            "black" => Some(Self::BLACK),
            "white" => Some(Self::WHITE),
            "red" => Some(Self::RED),
            "green" => Some(Self::GREEN),
            "blue" => Some(Self::BLUE),
            "yellow" => Some(Self::YELLOW),
            "cyan" => Some(Self::CYAN),
            "magenta" => Some(Self::MAGENTA),
            "transparent" => Some(Self::TRANSPARENT),
            "currentcolor" => Some(Self::BLACK), // 简化处理
            _ => None,
        }
    }

    /// 转换为 RGBA 数组
    pub fn to_rgba(&self) -> [u8; 4] {
        [self.r, self.g, self.b, self.a]
    }

    /// 转换为十六进制字符串
    pub fn to_hex(&self) -> String {
        if self.a == 255 {
            format!("#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
        } else {
            format!("#{:02x}{:02x}{:02x}{:02x}", self.r, self.g, self.b, self.a)
        }
    }

    // 预定义颜色
    pub const BLACK: Self = Self { r: 0, g: 0, b: 0, a: 255 };
    pub const WHITE: Self = Self { r: 255, g: 255, b: 255, a: 255 };
    pub const RED: Self = Self { r: 255, g: 0, b: 0, a: 255 };
    pub const GREEN: Self = Self { r: 0, g: 128, b: 0, a: 255 };
    pub const BLUE: Self = Self { r: 0, g: 0, b: 255, a: 255 };
    pub const YELLOW: Self = Self { r: 255, g: 255, b: 0, a: 255 };
    pub const CYAN: Self = Self { r: 0, g: 255, b: 255, a: 255 };
    pub const MAGENTA: Self = Self { r: 255, g: 0, b: 255, a: 255 };
    pub const TRANSPARENT: Self = Self { r: 0, g: 0, b: 0, a: 0 };
}

impl Default for Color {
    fn default() -> Self {
        Self::BLACK
    }
}

/// 属性值
#[derive(Debug, Clone, PartialEq)]
pub enum PropertyValue {
    /// 颜色值
    Color(Color),
    /// 长度值
    Length(Length),
    /// 关键字
    Keyword(String),
    /// 多个值
    Multiple(Vec<PropertyValue>),
}

impl PropertyValue {
    /// 解析属性值字符串
    pub fn parse(value: &str) -> Self {
        let value = value.trim();

        // 检查颜色
        if value.starts_with('#') {
            return PropertyValue::Color(Color::from_hex(value));
        }

        // 检查命名颜色
        if let Some(color) = Color::from_name(value) {
            return PropertyValue::Color(color);
        }

        // 检查 rgb/rgba
        if value.starts_with("rgb") || value.starts_with("rgba") {
            // 简化处理
            if let Some(color) = Color::from_name("black") {
                return PropertyValue::Color(color);
            }
        }

        // 检查数值
        let mut has_number = false;
        let mut has_unit = false;
        let mut num_str = String::new();

        for c in value.chars() {
            if c.is_ascii_digit() || c == '.' {
                has_number = true;
                num_str.push(c);
            } else if c.is_ascii_alphabetic() || c == '%' {
                has_unit = true;
            }
        }

        if has_number && !has_unit {
            if let Ok(num) = num_str.parse::<f64>() {
                return PropertyValue::Length(Length {
                    value: num,
                    unit: LengthUnit::Px,
                });
            }
        }

        // 默认作为关键字
        PropertyValue::Keyword(value.to_string())
    }
}

impl Default for PropertyValue {
    fn default() -> Self {
        PropertyValue::Keyword("initial".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_color_from_hex() {
        let color = Color::from_hex("#ff0000");
        assert_eq!(color.r, 255);
        assert_eq!(color.g, 0);
        assert_eq!(color.b, 0);
    }

    #[test]
    fn test_color_from_rgb() {
        let color = Color::rgb(100, 150, 200);
        assert_eq!(color.r, 100);
        assert_eq!(color.g, 150);
        assert_eq!(color.b, 200);
    }

    #[test]
    fn test_length_to_px() {
        let length = Length::px(100.0);
        assert_eq!(length.to_px(16.0, 16.0), 100.0);

        let percent = Length::percent(50.0);
        assert_eq!(percent.to_px(200.0, 16.0), 100.0);
    }
}
