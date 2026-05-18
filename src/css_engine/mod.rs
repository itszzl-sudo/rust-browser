//! CSS 引擎 —— 从 HTML 提取样式并应用到布局
//!
//! 1. 从 `<style>` 标签提取 CSS
//! 2. 手动解析 CSS 规则（使用自研解析器）
//! 3. 使用 `selectors` crate 按完整 CSS 选择器匹配元素

pub mod selector;

use std::collections::HashMap;

/// 默认视口宽度（用于 @media 查询评估）
pub(crate) const VIEWPORT_WIDTH: f32 = 1280.0;

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
/// 支持 @media 查询：当前只支持 screen 和 (min-width: Xpx) 条件
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

        // 检测 @media 块
        if bytes[pos] == b'@' {
            if let Some(end) = parse_media_block(css, &mut pos, bytes) {
                // pos is updated inside; if media block condition is met, merge inner rules
                if let Some(inner_rules) = end {
                    rules.extend(inner_rules);
                }
            }
            continue;
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

/// 解析 @media 块
/// 返回 Some(inner_rules) 如果条件满足，否则返回 Some(empty vec)
/// 返回 None 表示解析失败
fn parse_media_block(css: &str, pos: &mut usize, bytes: &[u8]) -> Option<Option<Vec<CssRule>>> {
    // 当前 pos 在 '@' 上
    // 提取 @media 条件部分：从 '@' 到第一个 '{'
    let brace_start = match css[*pos..].find('{') {
        Some(i) => *pos + i,
        None => return None,
    };

    let media_condition = css[*pos..brace_start].trim();
    *pos = brace_start + 1;

    // 找到匹配的 }（@media 的最外层）
    let mut depth = 1u32;
    let mut brace_end = *pos;
    while brace_end < bytes.len() && depth > 0 {
        match bytes[brace_end] {
            b'{' => depth += 1,
            b'}' => depth -= 1,
            _ => {}
        }
        brace_end += 1;
    }
    if depth > 0 {
        return None;
    }

    let block = &css[*pos..brace_end - 1];
    *pos = brace_end;

    // 解析 @media 条件
    // 只支持 screen 和 (min-width: Xpx)
    let condition_met = evaluate_media_condition(media_condition);

    if condition_met {
        // 条件满足，递归解析内部的规则（传给 css 字符串的 block 部分）
        // 但 block 内部的规则是普通 CSS 规则，调用 parse_css_rules 递归
        let inner_rules = parse_css_rules(block);
        Some(Some(inner_rules))
    } else {
        // 条件不满足，跳过
        Some(Some(Vec::new()))
    }
}

/// 判断 @media 条件是否满足
/// 当前只支持：
/// - `screen`
/// - `(min-width: Xpx)`
/// - `screen and (min-width: Xpx)`
fn evaluate_media_condition(condition: &str) -> bool {
    let trimmed = condition.trim();
    // 移除开头的 @media
    let cond = trimmed
        .strip_prefix("@media")
        .map(|s| s.trim())
        .unwrap_or(trimmed);

    // 默认 screen 总是 true（我们支持 screen）
    if cond.eq_ignore_ascii_case("screen") || cond.is_empty() || cond.eq_ignore_ascii_case("all") {
        return true;
    }

    // 检查 screen and (...) 或 only screen and (...)
    let inner = if let Some(rest) = cond
        .strip_prefix("screen")
        .or_else(|| cond.strip_prefix("only screen"))
    {
        rest.trim()
    } else {
        cond
    };

    // 检查 and (min-width: Xpx)
    if let Some(rest) = inner.strip_prefix("and") {
        let paren_part = rest.trim();
        evaluate_parenthesized_condition(paren_part)
    } else if inner.starts_with('(') {
        evaluate_parenthesized_condition(inner)
    } else {
        // 不支持的媒体类型，返回 false
        false
    }
}

/// 解析括号内的条件，如 (min-width: 768px)
fn evaluate_parenthesized_condition(cond: &str) -> bool {
    let trimmed = cond.trim();
    // 去掉首尾括号
    let inner = if trimmed.starts_with('(') && trimmed.ends_with(')') {
        &trimmed[1..trimmed.len() - 1]
    } else {
        trimmed
    };

    let inner = inner.trim();

    // min-width: Xpx
    if let Some(value_str) = inner.strip_prefix("min-width:") {
        let value_str = value_str.trim();
        if let Some(px_value) = parse_length(value_str) {
            return px_value <= VIEWPORT_WIDTH;
        }
    }

    // max-width: Xpx
    if let Some(value_str) = inner.strip_prefix("max-width:") {
        let value_str = value_str.trim();
        if let Some(px_value) = parse_length(value_str) {
            return px_value >= VIEWPORT_WIDTH;
        }
    }

    // 其他条件暂不支持，默认返回 true 以兼容
    true
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
    parse_declarations(style_attr)
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

/// 解析颜色（委托给 css::values::Color）
pub fn parse_color(value: &str) -> Option<(u8, u8, u8)> {
    let v = value.trim();
    if v.starts_with('#') {
        let color = crate::css::values::Color::from_hex(v);
        Some((color.r, color.g, color.b))
    } else {
        None
    }
}
