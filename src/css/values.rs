//! CSS 值 - CSS 属性值类型
//!
//! 定义颜色、长度、关键字等 CSS 值类型

use std::collections::HashMap;
use std::fmt;
use std::sync::{Mutex, OnceLock};

static COLOR_CACHE: OnceLock<Mutex<HashMap<String, Color>>> = OnceLock::new();

fn get_color_cache() -> &'static Mutex<HashMap<String, Color>> {
    COLOR_CACHE.get_or_init(|| {
        let mut map = HashMap::new();
        // 预置常用颜色
        insert_named_colors(&mut map);
        Mutex::new(map)
    })
}

fn insert_named_colors(map: &mut HashMap<String, Color>) {
    // 预置常用十六进制颜色
    map.insert("333333".to_string(), Color::rgb(51, 51, 51));
    map.insert("cccccc".to_string(), Color::rgb(204, 204, 204));
    map.insert("e0e0e0".to_string(), Color::rgb(224, 224, 224));
    map.insert("666666".to_string(), Color::rgb(102, 102, 102));
    map.insert("999999".to_string(), Color::rgb(153, 153, 153));
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

    /// 转换为像素
    ///
    /// # 参数
    /// - `reference` — 父容器尺寸（用于 em、百分比等）
    /// - `root_font_size` — 根字体大小（用于 rem）
    /// - `viewport_width` — 视口宽度（用于 vw）
    /// - `viewport_height` — 视口高度（用于 vh）
    pub fn to_px_with_viewport(
        &self,
        reference: f64,
        root_font_size: f64,
        viewport_width: f64,
        viewport_height: f64,
    ) -> f64 {
        match self.unit {
            LengthUnit::Px => self.value,
            LengthUnit::Em => self.value * reference,
            LengthUnit::Rem => self.value * root_font_size,
            LengthUnit::Percent => self.value * reference / 100.0,
            LengthUnit::Vw => self.value * viewport_width / 100.0,
            LengthUnit::Vh => self.value * viewport_height / 100.0,
            LengthUnit::Pt => self.value * 1.333, // 1pt = 1.333px
            LengthUnit::Cm => self.value * 37.795, // 1cm = 37.795px
            LengthUnit::Mm => self.value * 3.7795, // 1mm = 3.7795px
            LengthUnit::In => self.value * 96.0,  // 1in = 96px
        }
    }

    /// 转换为像素（向后兼容，默认视口尺寸为 1280x720）
    pub fn to_px(&self, reference: f64, root_font_size: f64) -> f64 {
        self.to_px_with_viewport(reference, root_font_size, 1280.0, 720.0)
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
    /// 从十六进制创建（使用动态缓存优化）
    pub fn from_hex(hex: &str) -> Self {
        let hex_clean = hex.trim_start_matches('#');

        // 尝试从缓存获取
        {
            let cache = get_color_cache().lock().unwrap();
            if let Some(cached) = cache.get(hex_clean) {
                return cached.clone();
            }
        }

        let color = match hex_clean.len() {
            3 => {
                let r = u8::from_str_radix(&hex_clean[0..1].repeat(2), 16).unwrap_or(0);
                let g = u8::from_str_radix(&hex_clean[1..2].repeat(2), 16).unwrap_or(0);
                let b = u8::from_str_radix(&hex_clean[2..3].repeat(2), 16).unwrap_or(0);
                Self { r, g, b, a: 255 }
            }
            4 => {
                let r = u8::from_str_radix(&hex_clean[0..1].repeat(2), 16).unwrap_or(0);
                let g = u8::from_str_radix(&hex_clean[1..2].repeat(2), 16).unwrap_or(0);
                let b = u8::from_str_radix(&hex_clean[2..3].repeat(2), 16).unwrap_or(0);
                let a = u8::from_str_radix(&hex_clean[3..4].repeat(2), 16).unwrap_or(255);
                Self { r, g, b, a }
            }
            6 => {
                let r = u8::from_str_radix(&hex_clean[0..2], 16).unwrap_or(0);
                let g = u8::from_str_radix(&hex_clean[2..4], 16).unwrap_or(0);
                let b = u8::from_str_radix(&hex_clean[4..6], 16).unwrap_or(0);
                Self { r, g, b, a: 255 }
            }
            8 => {
                let r = u8::from_str_radix(&hex_clean[0..2], 16).unwrap_or(0);
                let g = u8::from_str_radix(&hex_clean[2..4], 16).unwrap_or(0);
                let b = u8::from_str_radix(&hex_clean[4..6], 16).unwrap_or(0);
                let a = u8::from_str_radix(&hex_clean[6..8], 16).unwrap_or(255);
                Self { r, g, b, a }
            }
            _ => Self::BLACK,
        };

        // 写入动态缓存
        {
            let mut cache = get_color_cache().lock().unwrap();
            // 只缓存标准长度（6位十六进制），避免缓存膨胀
            if hex_clean.len() == 6 && !cache.contains_key(hex_clean) {
                let _ = cache.insert(hex_clean.to_string(), color.clone());
            }
        }

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

    /// 从名称创建（使用缓存优化）
    pub fn from_name(name: &str) -> Option<Self> {
        let name_lower = name.to_lowercase();
        // 基本命名颜色
        match name_lower.as_str() {
            "black" => Some(Self::BLACK),
            "white" => Some(Self::WHITE),
            "red" => Some(Self::RED),
            "green" => Some(Self::GREEN),
            "blue" => Some(Self::BLUE),
            "yellow" => Some(Self::YELLOW),
            "cyan" => Some(Self::CYAN),
            "magenta" => Some(Self::MAGENTA),
            "transparent" => Some(Self::TRANSPARENT),
            "gray" | "grey" => Some(Color::rgb(128, 128, 128)),
            "darkgray" | "darkgrey" => Some(Color::rgb(169, 169, 169)),
            "lightgray" | "lightgrey" => Some(Color::rgb(211, 211, 211)),
            "silver" => Some(Color::rgb(192, 192, 192)),
            "maroon" => Some(Color::rgb(128, 0, 0)),
            "purple" => Some(Color::rgb(128, 0, 128)),
            "fuchsia" => Some(Color::rgb(255, 0, 255)),
            "lime" => Some(Color::rgb(0, 255, 0)),
            "olive" => Some(Color::rgb(128, 128, 0)),
            "navy" => Some(Color::rgb(0, 0, 128)),
            "teal" => Some(Color::rgb(0, 128, 128)),
            "aqua" => Some(Color::rgb(0, 255, 255)),
            "orange" => Some(Color::rgb(255, 165, 0)),
            "brown" => Some(Color::rgb(165, 42, 42)),
            "coral" => Some(Color::rgb(255, 127, 80)),
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
    pub const BLACK: Self = Self {
        r: 0,
        g: 0,
        b: 0,
        a: 255,
    };
    pub const WHITE: Self = Self {
        r: 255,
        g: 255,
        b: 255,
        a: 255,
    };
    pub const RED: Self = Self {
        r: 255,
        g: 0,
        b: 0,
        a: 255,
    };
    pub const GREEN: Self = Self {
        r: 0,
        g: 128,
        b: 0,
        a: 255,
    };
    pub const BLUE: Self = Self {
        r: 0,
        g: 0,
        b: 255,
        a: 255,
    };
    pub const YELLOW: Self = Self {
        r: 255,
        g: 255,
        b: 0,
        a: 255,
    };
    pub const CYAN: Self = Self {
        r: 0,
        g: 255,
        b: 255,
        a: 255,
    };
    pub const MAGENTA: Self = Self {
        r: 255,
        g: 0,
        b: 255,
        a: 255,
    };
    pub const TRANSPARENT: Self = Self {
        r: 0,
        g: 0,
        b: 0,
        a: 0,
    };
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
        if let Some(color) = parse_rgb_rgba(value) {
            return PropertyValue::Color(color);
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

/// 解析 rgb/rgba 颜色字符串
/// 支持格式: rgb(r, g, b), rgba(r, g, b, a)
fn parse_rgb_rgba(value: &str) -> Option<Color> {
    let value = value.trim();

    let (inner, expected_parts, has_alpha) = if value.starts_with("rgba(") && value.ends_with(')') {
        (&value[5..value.len() - 1], 4, true)
    } else if value.starts_with("rgb(") && value.ends_with(')') {
        (&value[4..value.len() - 1], 3, false)
    } else {
        return None;
    };

    let parts: Vec<&str> = inner.split(',').collect();
    if parts.len() != expected_parts {
        return None;
    }

    let r = parts[0].trim().parse::<u8>().ok()?;
    let g = parts[1].trim().parse::<u8>().ok()?;
    let b = parts[2].trim().parse::<u8>().ok()?;

    if has_alpha {
        let a = parts[3].trim().parse::<f64>().ok()?;
        let a_byte = (a.clamp(0.0, 1.0) * 255.0).round() as u8;
        Some(Color::rgba(r, g, b, a_byte))
    } else {
        Some(Color::rgb(r, g, b))
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

    #[test]
    fn test_length_to_px_with_viewport() {
        let vw = Length {
            value: 50.0,
            unit: LengthUnit::Vw,
        };
        assert_eq!(vw.to_px_with_viewport(0.0, 16.0, 800.0, 600.0), 400.0);

        let vh = Length {
            value: 25.0,
            unit: LengthUnit::Vh,
        };
        assert_eq!(vh.to_px_with_viewport(0.0, 16.0, 800.0, 600.0), 150.0);

        let em = Length {
            value: 2.0,
            unit: LengthUnit::Em,
        };
        assert_eq!(em.to_px(16.0, 16.0), 32.0);

        let rem = Length {
            value: 1.5,
            unit: LengthUnit::Rem,
        };
        assert_eq!(rem.to_px(16.0, 24.0), 36.0);
    }

    #[test]
    fn test_color_from_named() {
        assert_eq!(Color::from_name("red"), Some(Color::RED));
        assert_eq!(Color::from_name("BLUE"), Some(Color::BLUE));
        assert_eq!(Color::from_name("Transparent"), Some(Color::TRANSPARENT));
        assert_eq!(Color::from_name("unknown_color"), None);
    }

    #[test]
    fn test_color_from_hex_edge_cases() {
        // 3位
        let c = Color::from_hex("#f00");
        assert_eq!(c, Color::rgb(255, 0, 0));

        // 6位
        let c = Color::from_hex("#00ff00");
        assert_eq!(c, Color::rgb(0, 255, 0));

        // 4位 (带 alpha) - 每位重复扩展: #f80f = rgba(255, 136, 0, 255)
        let c = Color::from_hex("#f80f");
        assert_eq!(c, Color::rgba(255, 136, 0, 255));

        // 8位 (带 alpha)
        let c = Color::from_hex("#ff000080");
        assert_eq!(c, Color::rgba(255, 0, 0, 128));

        // 不带 # 前缀
        let c = Color::from_hex("0000ff");
        assert_eq!(c, Color::rgb(0, 0, 255));

        // 非法格式返回黑色
        let c = Color::from_hex("#xyz");
        assert_eq!(c, Color::BLACK);
    }

    #[test]
    fn test_property_value_parse_rgb() {
        // rgb() 解析
        if let PropertyValue::Color(c) = PropertyValue::parse("rgb(100, 150, 200)") {
            assert_eq!(c, Color::rgb(100, 150, 200));
        } else {
            panic!("Expected Color");
        }

        // rgba() 解析
        if let PropertyValue::Color(c) = PropertyValue::parse("rgba(255, 0, 0, 0.5)") {
            assert_eq!(c, Color::rgba(255, 0, 0, 128));
        } else {
            panic!("Expected Color");
        }
    }

    #[test]
    fn test_property_value_parse_hex() {
        if let PropertyValue::Color(c) = PropertyValue::parse("#ff0000") {
            assert_eq!(c, Color::rgb(255, 0, 0));
        } else {
            panic!("Expected Color");
        }
    }

    #[test]
    fn test_property_value_parse_named() {
        if let PropertyValue::Color(c) = PropertyValue::parse("red") {
            assert_eq!(c, Color::RED);
        } else {
            panic!("Expected Color");
        }
    }

    #[test]
    fn test_property_value_parse_length() {
        if let PropertyValue::Length(l) = PropertyValue::parse("42") {
            assert_eq!(l.value, 42.0);
            assert_eq!(l.unit, LengthUnit::Px);
        } else {
            panic!("Expected Length");
        }
    }

    #[test]
    fn test_property_value_parse_keyword() {
        if let PropertyValue::Keyword(k) = PropertyValue::parse("auto") {
            assert_eq!(k, "auto");
        } else {
            panic!("Expected Keyword");
        }
    }

    #[test]
    fn test_color_to_hex() {
        assert_eq!(Color::rgb(255, 0, 0).to_hex(), "#ff0000");
        assert_eq!(Color::rgba(255, 0, 0, 128).to_hex(), "#ff000080");
    }

    #[test]
    fn test_color_to_rgba() {
        assert_eq!(Color::rgb(100, 150, 200).to_rgba(), [100, 150, 200, 255]);
    }

    #[test]
    fn test_length_default() {
        let l = Length::default();
        assert_eq!(l.value, 0.0);
        assert_eq!(l.unit, LengthUnit::Px);
    }

    #[test]
    fn test_color_cache_dynamic() {
        // 第一次解析 -> 缓存
        let c1 = Color::from_hex("aabbcc");
        assert_eq!(c1, Color::rgb(170, 187, 204));

        // 第二次解析 -> 从缓存读取（不同对象但值相同）
        let c2 = Color::from_hex("aabbcc");
        assert_eq!(c2, Color::rgb(170, 187, 204));
    }

    #[test]
    fn test_parse_rgb_rgba_edge_cases() {
        // 带空格的 rgb
        let c = super::parse_rgb_rgba("rgb( 10 , 20 , 30 )").unwrap();
        assert_eq!(c, Color::rgb(10, 20, 30));

        // rgba 透明度为 0
        let c = super::parse_rgb_rgba("rgba(0, 0, 0, 0.0)").unwrap();
        assert_eq!(c, Color::rgba(0, 0, 0, 0));

        // 非法格式
        assert!(super::parse_rgb_rgba("not-a-color").is_none());
        assert!(super::parse_rgb_rgba("rgb(1,2)").is_none());
    }
}
