//! CSS 引擎 —— 从 HTML 提取样式并应用到布局
//!
//! 1. 从 `<style>` 标签提取 CSS
//! 2. 手动解析 CSS 规则（使用自研解析器）
//! 3. 使用 `selectors` crate 按完整 CSS 选择器匹配元素

pub mod selector;

use std::collections::HashMap;

/// 单个 CSS 声明
#[derive(Debug, Clone)]
pub struct Declaration {
    pub property: String,
    pub value: String,
}

/// CSS 规则
#[derive(Debug, Clone)]
pub struct CssRule {
    pub selector: String,
    pub declarations: Vec<Declaration>,
}

/// 样式映射：tag_name → declarations
pub type StyleMap = HashMap<String, Vec<Declaration>>;

/// 从 CSS 文本中手动提取规则（无需 cssparser tokenizer）
pub fn parse_css_rules(css: &str) -> Vec<CssRule> {
    let mut rules = Vec::new();
    let mut pos = 0usize;
    let bytes = css.as_bytes();

    while pos < bytes.len() {
        // 跳过空白
        while pos < bytes.len() && bytes[pos].is_ascii_whitespace() {
            pos += 1;
        }
        if pos >= bytes.len() {
            break;
        }

        // 跳过注释 /* ... */
        if pos + 1 < bytes.len() && bytes[pos] == b'/' && bytes[pos + 1] == b'*' {
            if let Some(end) = css[pos + 2..].find("*/") {
                pos += end + 4;
                continue;
            }
            break;
        }

        // 找到 { 的位置
        let brace_start = match css[pos..].find('{') {
            Some(i) => pos + i,
            None => break,
        };

        let selector = css[pos..brace_start].trim();
        pos = brace_start + 1;

        // 找到匹配的 }
        let mut depth = 1u32;
        let mut brace_end = pos;
        while brace_end < bytes.len() && depth > 0 {
            match bytes[brace_end] {
                b'{' => depth += 1,
                b'}' => depth -= 1,
                _ => {}
            }
            brace_end += 1;
        }
        if depth > 0 {
            break;
        }

        let block = &css[pos..brace_end - 1];
        pos = brace_end;

        if selector.is_empty() {
            continue;
        }

        let declarations = parse_declarations(block);
        rules.push(CssRule {
            selector: selector.to_string(),
            declarations,
        });
    }
    rules
}

/// 解析声明块 "property: value; ..."
fn parse_declarations(block: &str) -> Vec<Declaration> {
    let mut decls = Vec::new();
    for part in block.split(';') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        if let Some(colon) = part.find(':') {
            let property = part[..colon].trim().to_string();
            let value = part[colon + 1..].trim().to_string();
            if !property.is_empty() {
                decls.push(Declaration { property, value });
            }
        }
    }
    decls
}

/// 将 CSS 规则转换为按标签名分组的样式映射
///
/// 使用 `selectors` crate 进行完整的 CSS 选择器匹配。
///
/// `rules` — 已解析的 CSS 规则
/// `doc_ref` — kuchiki 文档节点，用于遍历所有后代元素并进行选择器匹配
pub fn rules_to_style_map(rules: &[CssRule], doc_ref: &kuchiki::NodeRef) -> StyleMap {
    selector::rules_to_style_map_with_selectors(rules, doc_ref)
}

/// 解析内联 style 属性
pub fn parse_inline_style(style_attr: &str) -> Vec<Declaration> {
    let mut decls = Vec::new();
    for part in style_attr.split(';') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        if let Some(colon) = part.find(':') {
            let property = part[..colon].trim().to_string();
            let value = part[colon + 1..].trim().to_string();
            if !property.is_empty() {
                decls.push(Declaration { property, value });
            }
        }
    }
    decls
}

/// 从声明列表中获取指定属性的值
pub fn get_declaration(decls: &[Declaration], property: &str) -> Option<String> {
    decls
        .iter()
        .find(|d| d.property == property)
        .map(|d| d.value.clone())
}

/// 解析长度值（px）
pub fn parse_length(value: &str) -> Option<f32> {
    let v = value.trim();
    if let Some(px) = v.strip_suffix("px") {
        px.trim().parse::<f32>().ok()
    } else if let Some(em) = v.strip_suffix("em") {
        Some(em.trim().parse::<f32>().unwrap_or(0.0) * 16.0) // 1em ≈ 16px
    } else if let Some(rem) = v.strip_suffix("rem") {
        Some(rem.trim().parse::<f32>().unwrap_or(0.0) * 16.0)
    } else if let Ok(n) = v.parse::<f32>() {
        Some(n)
    } else {
        None
    }
}

/// 解析颜色
pub fn parse_color(value: &str) -> Option<(u8, u8, u8)> {
    let v = value.trim();
    if v.starts_with('#') {
        let hex = &v[1..];
        match hex.len() {
            3 => Some((
                u8::from_str_radix(&hex[0..1].repeat(2), 16).ok()?,
                u8::from_str_radix(&hex[1..2].repeat(2), 16).ok()?,
                u8::from_str_radix(&hex[2..3].repeat(2), 16).ok()?,
            )),
            6 => Some((
                u8::from_str_radix(&hex[0..2], 16).ok()?,
                u8::from_str_radix(&hex[2..4], 16).ok()?,
                u8::from_str_radix(&hex[4..6], 16).ok()?,
            )),
            _ => None,
        }
    } else {
        None
    }
}
