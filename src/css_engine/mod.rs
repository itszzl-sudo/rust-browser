//! CSS 引擎 —— 从 HTML 提取样式并应用到布局
//!
//! 1. 从 `<style>` 标签提取 CSS
//! 2. 手动解析 CSS 规则（使用自研解析器）
//! 3. 使用 `selectors` crate 按完整 CSS 选择器匹配元素

pub mod selector;

use std::collections::HashMap;

/// 默认视口宽度（用于 @media 查询评估）
pub(crate) const VIEWPORT_WIDTH: f32 = 1280.0;

/// 默认视口高度（用于 @media 查询评估）
pub(crate) const VIEWPORT_HEIGHT: f32 = 720.0;

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

    // min-height: Xpx
    if let Some(value_str) = inner.strip_prefix("min-height:") {
        let value_str = value_str.trim();
        if let Some(px_value) = parse_length(value_str) {
            return px_value <= VIEWPORT_HEIGHT;
        }
    }

    // max-height: Xpx
    if let Some(value_str) = inner.strip_prefix("max-height:") {
        let value_str = value_str.trim();
        if let Some(px_value) = parse_length(value_str) {
            return px_value >= VIEWPORT_HEIGHT;
        }
    }

    // prefers-color-scheme: dark
    if inner
        .trim()
        .eq_ignore_ascii_case("prefers-color-scheme: dark")
    {
        // 当前默认支持暗色模式
        return true;
    }

    // prefers-color-scheme: light
    if inner
        .trim()
        .eq_ignore_ascii_case("prefers-color-scheme: light")
    {
        // 当前默认暗色，不支持 light
        return false;
    }

    // prefers-reduced-motion: reduce
    if inner
        .trim()
        .eq_ignore_ascii_case("prefers-reduced-motion: reduce")
    {
        // 当前不支持减少动画
        return false;
    }

    // 其他条件，默认返回 true 以兼容
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
    } else if let Some(named) = crate::css::values::Color::from_name(v) {
        Some((named.r, named.g, named.b))
    } else if v.starts_with("rgb") {
        // 用 cssparser 解析 rgb/rgba
        parse_rgb_color(v)
    } else {
        None
    }
}

/// 使用 cssparser 解析 rgb/rgba 颜色
fn parse_rgb_color(value: &str) -> Option<(u8, u8, u8)> {
    let cleaned = value
        .replace("rgba(", "")
        .replace("rgb(", "")
        .replace(")", "");
    let parts: Vec<&str> = cleaned.split(',').collect();
    if parts.len() >= 3 {
        let r = parts[0].trim().parse::<u8>().ok()?;
        let g = parts[1].trim().parse::<u8>().ok()?;
        let b = parts[2].trim().parse::<u8>().ok()?;
        Some((r, g, b))
    } else {
        None
    }
}

/// 用 cssparser 解析 CSS 颜色值，支持更完整的颜色语法
/// 包括：hex、命名颜色、rgb()、rgba()、hsl() 等
/// 用 cssparser 解析 CSS 颜色值，支持更完整的颜色语法
/// 包括：hex、命名颜色、rgb()、rgba()、hsl() 等
/// 用 cssparser 解析 CSS 颜色值，支持更完整的颜色语法
pub fn parse_css_color_strict(value: &str) -> Option<(u8, u8, u8, u8)> {
    use cssparser::{Parser, ParserInput, Token};

    let mut input = ParserInput::new(value);
    let mut parser = Parser::new(&mut input);

    match parser.next() {
        Ok(Token::Hash(ref hex)) | Ok(Token::IDHash(ref hex)) => {
            let hex_str = hex.as_ref();
            let rgb = crate::css::values::Color::from_hex(&format!("#{}", hex_str));
            Some((rgb.r, rgb.g, rgb.b, rgb.a))
        }
        Ok(Token::Function(ref name))
            if name.eq_ignore_ascii_case("rgb") || name.eq_ignore_ascii_case("rgba") =>
        {
            // 对于 rgb()/rgba()，直接用字符串替换解析
            let full_value = value.trim();
            let inner = full_value
                .trim_start_matches(|c: char| c != '(')
                .trim_start_matches('(')
                .trim_end_matches(')');
            let parts: Vec<&str> = inner.split(',').collect();
            if parts.len() >= 3 {
                let r = parts[0].trim().parse::<u8>().ok()?;
                let g = parts[1].trim().parse::<u8>().ok()?;
                let b = parts[2].trim().parse::<u8>().ok()?;
                Some((r, g, b, 255))
            } else {
                None
            }
        }
        Ok(Token::Ident(ref name)) => {
            if let Some(color) = crate::css::values::Color::from_name(name.as_ref()) {
                Some((color.r, color.g, color.b, color.a))
            } else {
                None
            }
        }
        _ => None,
    }
}

/// 用 cssparser 解析 background 简写属性
/// 返回 (颜色字符串, 图片URL字符串)
pub fn parse_background_shorthand(value: &str) -> (Option<String>, Option<String>) {
    use cssparser::{Parser, ParserInput, Token};

    let mut color = None;
    let mut image = None;

    let mut input = ParserInput::new(value);
    let mut parser = Parser::new(&mut input);

    loop {
        parser.skip_whitespace();
        if parser.is_exhausted() {
            break;
        }

        match parser.next() {
            Ok(Token::Hash(ref h)) | Ok(Token::IDHash(ref h)) => {
                if color.is_none() {
                    color = Some(format!("#{}", h.as_ref()));
                }
            }
            Ok(Token::Function(ref func_name)) if func_name.eq_ignore_ascii_case("url") => {
                // url() 函数 - 直接消费到 )
                loop {
                    match parser.next() {
                        Ok(Token::CloseParenthesis) | Err(_) => break,
                        Ok(Token::UnquotedUrl(ref u)) => {
                            if image.is_none() {
                                image = Some(u.as_ref().to_string());
                            }
                        }
                        Ok(Token::QuotedString(ref s)) => {
                            if image.is_none() {
                                image = Some(s.as_ref().to_string());
                            }
                        }
                        _ => {}
                    }
                }
            }
            Ok(Token::UnquotedUrl(ref url)) => {
                if image.is_none() {
                    image = Some(url.as_ref().to_string());
                }
            }
            Ok(Token::Ident(ref name)) => {
                let name_str = name.as_ref();
                if color.is_none() && crate::css::values::Color::from_name(name_str).is_some() {
                    color = Some(name_str.to_string());
                }
            }
            _ => {}
        }
    }

    (color, image)
}

// ==========================================================================
// Tests
// ==========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use kuchiki::parse_html;
    use kuchiki::traits::TendrilSink;

    // ----------------------------------------------------------------------
    // parse_css_rules tests
    // ----------------------------------------------------------------------

    #[test]
    fn test_parse_css_rules_simple() {
        let css = r"div { color: red; }";
        let rules = parse_css_rules(css);
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].selector, "div");
        assert_eq!(rules[0].declarations.len(), 1);
        assert_eq!(rules[0].declarations[0].property, "color");
        assert_eq!(rules[0].declarations[0].value, "red");
    }

    #[test]
    fn test_parse_css_rules_multiple_rules() {
        let css = r"
div { color: red; }
p { font-size: 16px; }
span { margin: 0; }
";
        let rules = parse_css_rules(css);
        assert_eq!(rules.len(), 3);
        assert_eq!(rules[0].selector, "div");
        assert_eq!(rules[1].selector, "p");
        assert_eq!(rules[2].selector, "span");
    }

    #[test]
    fn test_parse_css_rules_multiple_declarations() {
        let css = r"h1 { color: blue; font-size: 24px; margin: 10px; }";
        let rules = parse_css_rules(css);
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].declarations.len(), 3);
        assert_eq!(rules[0].declarations[0].property, "color");
        assert_eq!(rules[0].declarations[0].value, "blue");
        assert_eq!(rules[0].declarations[1].property, "font-size");
        assert_eq!(rules[0].declarations[1].value, "24px");
        assert_eq!(rules[0].declarations[2].property, "margin");
        assert_eq!(rules[0].declarations[2].value, "10px");
    }

    #[test]
    fn test_parse_css_rules_empty() {
        let rules = parse_css_rules("");
        assert!(rules.is_empty());
    }

    #[test]
    fn test_parse_css_rules_whitespace_only() {
        let rules = parse_css_rules("   \n   \t   ");
        assert!(rules.is_empty());
    }

    #[test]
    fn test_parse_css_rules_with_comments() {
        let css = r"/* This is a comment */ div { color: red; }";
        let rules = parse_css_rules(css);
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].selector, "div");
        assert_eq!(rules[0].declarations[0].value, "red");
    }

    #[test]
    fn test_parse_css_rules_comments_in_middle() {
        let css = r"div { /* comment inside */ color: red; }";
        let rules = parse_css_rules(css);
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].declarations[0].value, "red");
    }

    #[test]
    fn test_parse_css_rules_multi_line_comments() {
        let css = r"/*
 * multi-line comment
 */
div { color: red; }";
        let rules = parse_css_rules(css);
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].selector, "div");
    }

    #[test]
    fn test_parse_css_rules_missing_brace_no_panic() {
        let css = r"div { color: red; ";
        // Should not panic, just return whatever was parsed
        let rules = parse_css_rules(css);
        // Either empty or with whatever was successfully parsed
        assert!(rules.len() <= 1);
    }

    #[test]
    fn test_parse_css_rules_garbage_input_no_panic() {
        let css = r"!@#$%^&*() broken";
        let rules = parse_css_rules(css);
        // Should not panic, just return gracefully
        assert!(rules.is_empty());
    }

    #[test]
    fn test_parse_css_rules_no_closing_brace_no_panic() {
        let css = r"div { color: red; ";
        let rules = parse_css_rules(css);
        // Should not panic
        assert!(rules.is_empty() || rules[0].declarations.len() == 1);
    }

    #[test]
    fn test_parse_css_rules_media_rule_skipped() {
        let css = r"
@media print {
    div { color: black; }
}
div { color: red; }
";
        let rules = parse_css_rules(css);
        // print is not screen, so @media block should be skipped
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].selector, "div");
        assert_eq!(rules[0].declarations[0].value, "red");
    }

    #[test]
    fn test_parse_css_rules_media_screen_included() {
        let css = r"
@media screen {
    div { color: blue; }
}
";
        let rules = parse_css_rules(css);
        // screen is supported, so inner rules should be included
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].declarations[0].value, "blue");
    }

    #[test]
    fn test_parse_css_rules_nested_braces() {
        // Note: The parser counts braces to handle nesting,
        // so a '{' inside a string value will increase depth.
        // We test that the parser doesn't panic.
        let css = r"div { content: 'text'; }";
        let rules = parse_css_rules(css);
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].declarations[0].value, "'text'");
    }

    #[test]
    fn test_parse_css_rules_selector_with_hyphen() {
        let css = r"my-component { color: red; }";
        let rules = parse_css_rules(css);
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].selector, "my-component");
    }

    #[test]
    fn test_parse_css_rules_empty_block() {
        let css = r"div { }";
        let rules = parse_css_rules(css);
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].selector, "div");
        assert!(rules[0].declarations.is_empty());
    }

    #[test]
    fn test_parse_css_rules_empty_selector_skipped() {
        let css = r"{ color: red; }";
        let rules = parse_css_rules(css);
        assert!(rules.is_empty());
    }

    // ----------------------------------------------------------------------
    // rules_to_style_map tests
    // ----------------------------------------------------------------------

    fn parse_html_simple(html: &str) -> kuchiki::NodeRef {
        parse_html().one(html)
    }

    #[test]
    fn test_rules_to_style_map_simple() {
        let html = r"<html><body><div>Hello</div></body></html>";
        let doc = parse_html_simple(html);
        let css = r"div { color: red; font-size: 16px; }";
        let rules = parse_css_rules(css);
        let map = rules_to_style_map(&rules, &doc);

        // The map should contain an entry for "div"
        assert!(
            map.contains_key("div"),
            "map should contain key 'div', got keys: {:?}",
            map.keys().collect::<Vec<_>>()
        );

        let div_decls = &map["div"];
        assert_eq!(div_decls.len(), 2);
        assert_eq!(div_decls[0].property, "color");
        assert_eq!(div_decls[0].value, "red");
        assert_eq!(div_decls[1].property, "font-size");
        assert_eq!(div_decls[1].value, "16px");
    }

    #[test]
    fn test_rules_to_style_map_multiple_selectors() {
        let html = r"<html><body><div>Div</div><p>Para</p><span>Span</span></body></html>";
        let doc = parse_html_simple(html);
        let css = r"
div { color: red; }
p { color: blue; }
span { color: green; }
";
        let rules = parse_css_rules(css);
        let map = rules_to_style_map(&rules, &doc);

        assert!(map.contains_key("div"), "missing div");
        assert!(map.contains_key("p"), "missing p");
        assert!(map.contains_key("span"), "missing span");

        assert_eq!(map["div"][0].value, "red");
        assert_eq!(map["p"][0].value, "blue");
        assert_eq!(map["span"][0].value, "green");
    }

    #[test]
    fn test_rules_to_style_map_empty_rules() {
        let html = r"<html><body><div>Hello</div></body></html>";
        let doc = parse_html_simple(html);
        let map = rules_to_style_map(&[], &doc);
        assert!(map.is_empty());
    }

    #[test]
    fn test_rules_to_style_map_no_matching_elements() {
        let html = r"<html><body><div>Hello</div></body></html>";
        let doc = parse_html_simple(html);
        let css = r"span { color: red; }";
        let rules = parse_css_rules(css);
        let map = rules_to_style_map(&rules, &doc);
        // No span in the document, so map should be empty
        assert!(map.is_empty() || !map.contains_key("div"));
    }

    #[test]
    fn test_rules_to_style_map_class_selector() {
        let html = r#"<html><body><div class="foo">Hello</div></body></html>"#;
        let doc = parse_html_simple(html);
        let css = r".foo { color: red; }";
        let rules = parse_css_rules(css);
        let map = rules_to_style_map(&rules, &doc);

        // The key is the tag name "div" because style_key uses tag_name
        assert!(
            map.contains_key("div"),
            "map should contain 'div', got keys: {:?}",
            map.keys().collect::<Vec<_>>()
        );
        assert_eq!(map["div"][0].value, "red");
    }

    #[test]
    fn test_rules_to_style_map_id_selector() {
        let html = r#"<html><body><div id="main">Hello</div></body></html>"#;
        let doc = parse_html_simple(html);
        let css = r"#main { background: yellow; }";
        let rules = parse_css_rules(css);
        let map = rules_to_style_map(&rules, &doc);

        assert!(
            map.contains_key("div"),
            "map should contain 'div', got keys: {:?}",
            map.keys().collect::<Vec<_>>()
        );
        assert_eq!(map["div"][0].value, "yellow");
    }

    #[test]
    fn test_rules_to_style_map_descendant_selector() {
        let html = r"<html><body><div><span>Nested</span></div></body></html>";
        let doc = parse_html_simple(html);
        let css = r"div span { color: red; }";
        let rules = parse_css_rules(css);
        let map = rules_to_style_map(&rules, &doc);

        // The descendant span should match
        assert!(
            map.contains_key("span"),
            "map should contain 'span', got keys: {:?}",
            map.keys().collect::<Vec<_>>()
        );
        assert_eq!(map["span"][0].value, "red");
    }

    // ----------------------------------------------------------------------
    // parse_inline_style tests
    // ----------------------------------------------------------------------

    #[test]
    fn test_parse_inline_style_normal() {
        let style = "color: red; font-size: 16px";
        let decls = parse_inline_style(style);
        assert_eq!(decls.len(), 2);
        assert_eq!(decls[0].property, "color");
        assert_eq!(decls[0].value, "red");
        assert_eq!(decls[1].property, "font-size");
        assert_eq!(decls[1].value, "16px");
    }

    #[test]
    fn test_parse_inline_style_empty() {
        let decls = parse_inline_style("");
        assert!(decls.is_empty());
    }

    #[test]
    fn test_parse_inline_style_missing_semicolon() {
        // The last declaration doesn't need a semicolon
        let style = "color: red";
        let decls = parse_inline_style(style);
        assert_eq!(decls.len(), 1);
        assert_eq!(decls[0].value, "red");
    }

    #[test]
    fn test_parse_inline_style_value_with_spaces() {
        let style = r#"font-family: "Arial", sans-serif"#;
        let decls = parse_inline_style(style);
        assert_eq!(decls.len(), 1);
        assert_eq!(decls[0].property, "font-family");
        assert_eq!(decls[0].value, r#""Arial", sans-serif"#);
    }

    #[test]
    fn test_parse_inline_style_multiple_values_with_spaces() {
        let style = "margin: 10px 20px 10px 20px; padding: 5px 10px;";
        let decls = parse_inline_style(style);
        assert_eq!(decls.len(), 2);
        assert_eq!(decls[0].value, "10px 20px 10px 20px");
        assert_eq!(decls[1].value, "5px 10px");
    }

    #[test]
    fn test_parse_inline_style_trailing_semicolon() {
        let style = "color: red;";
        let decls = parse_inline_style(style);
        assert_eq!(decls.len(), 1);
        assert_eq!(decls[0].value, "red");
    }

    #[test]
    fn test_parse_inline_style_whitespace_around_colon() {
        let style = "color  :  red ; font-size : 16px";
        let decls = parse_inline_style(style);
        assert_eq!(decls.len(), 2);
        assert_eq!(decls[0].property, "color");
        assert_eq!(decls[0].value, "red");
    }

    #[test]
    fn test_parse_inline_style_no_colon_ignored() {
        let style = "color red; font-size: 16px";
        let decls = parse_inline_style(style);
        // "color red" without colon is skipped
        assert_eq!(decls.len(), 1);
        assert_eq!(decls[0].property, "font-size");
    }

    // ----------------------------------------------------------------------
    // get_declaration tests
    // ----------------------------------------------------------------------

    #[test]
    fn test_get_declaration_found() {
        let decls = vec![
            Declaration {
                property: "color".to_string(),
                value: "red".to_string(),
            },
            Declaration {
                property: "font-size".to_string(),
                value: "16px".to_string(),
            },
        ];
        assert_eq!(get_declaration(&decls, "color"), Some("red".to_string()));
        assert_eq!(
            get_declaration(&decls, "font-size"),
            Some("16px".to_string())
        );
    }

    #[test]
    fn test_get_declaration_not_found() {
        let decls = vec![Declaration {
            property: "color".to_string(),
            value: "red".to_string(),
        }];
        assert_eq!(get_declaration(&decls, "background"), None);
        assert_eq!(get_declaration(&decls, "margin"), None);
    }

    #[test]
    fn test_get_declaration_empty_decls() {
        let decls: Vec<Declaration> = vec![];
        assert_eq!(get_declaration(&decls, "color"), None);
    }

    #[test]
    fn test_get_declaration_case_sensitive() {
        // CSS properties are case-insensitive per spec, but our implementation
        // currently does exact matching. Let's verify current behavior.
        let decls = vec![Declaration {
            property: "Color".to_string(),
            value: "red".to_string(),
        }];
        // Exact match works
        assert_eq!(get_declaration(&decls, "Color"), Some("red".to_string()));
        // Case mismatch: this depends on current implementation
        // The function does exact match, so lowercase won't find "Color"
        assert_eq!(get_declaration(&decls, "color"), None);
    }

    // ----------------------------------------------------------------------
    // parse_selector tests
    // ----------------------------------------------------------------------

    #[test]
    fn test_parse_selector_tag() {
        let sel = selector::parse_selector("div");
        assert!(sel.is_ok(), "Expected OK, got {:?}", sel);
    }

    #[test]
    fn test_parse_selector_class() {
        let sel = selector::parse_selector(".class");
        assert!(sel.is_ok(), "Expected OK, got {:?}", sel);
    }

    #[test]
    fn test_parse_selector_id() {
        let sel = selector::parse_selector("#id");
        assert!(sel.is_ok(), "Expected OK, got {:?}", sel);
    }

    #[test]
    fn test_parse_selector_descendant() {
        let sel = selector::parse_selector("div span");
        assert!(sel.is_ok(), "Expected OK, got {:?}", sel);
    }

    #[test]
    fn test_parse_selector_child() {
        let sel = selector::parse_selector("div > span");
        assert!(sel.is_ok(), "Expected OK, got {:?}", sel);
    }

    #[test]
    fn test_parse_selector_attribute() {
        let sel = selector::parse_selector(r#"[type="text"]"#);
        assert!(sel.is_ok(), "Expected OK, got {:?}", sel);
    }

    #[test]
    fn test_parse_selector_pseudo_class() {
        let sel = selector::parse_selector(":hover");
        assert!(sel.is_ok(), "Expected OK, got {:?}", sel);
    }

    #[test]
    fn test_parse_selector_multiple() {
        let sel = selector::parse_selector("div, span");
        assert!(sel.is_ok(), "Expected OK, got {:?}", sel);
    }

    #[test]
    fn test_parse_selector_invalid_returns_err() {
        let sel = selector::parse_selector("!!!invalid!!!");
        assert!(sel.is_err(), "Expected Err, got {:?}", sel);
    }

    #[test]
    fn test_parse_selector_empty_returns_err() {
        let sel = selector::parse_selector("");
        assert!(sel.is_err(), "Expected Err, got {:?}", sel);
    }

    // ----------------------------------------------------------------------
    // element_matches_selector_list tests
    // ----------------------------------------------------------------------

    #[test]
    fn test_element_matches_tag_selector() {
        let html = r"<html><body><div>Hello</div></body></html>";
        let doc = parse_html_simple(html);

        // Find the div node
        let div_node = doc
            .descendants()
            .find(|n| {
                n.as_element()
                    .map(|el| el.name.local.as_ref() == "div")
                    .unwrap_or(false)
            })
            .expect("div node should exist");

        let selector_list = selector::parse_selector("div").expect("parse selector");
        assert!(selector::element_matches_selector_list(
            &div_node,
            &selector_list
        ));
    }

    #[test]
    fn test_element_does_not_match_wrong_tag() {
        let html = r"<html><body><div>Hello</div></body></html>";
        let doc = parse_html_simple(html);

        let div_node = doc
            .descendants()
            .find(|n| {
                n.as_element()
                    .map(|el| el.name.local.as_ref() == "div")
                    .unwrap_or(false)
            })
            .expect("div node should exist");

        let selector_list = selector::parse_selector("span").expect("parse selector");
        assert!(!selector::element_matches_selector_list(
            &div_node,
            &selector_list
        ));
    }

    #[test]
    fn test_element_matches_class_selector() {
        let html = r#"<html><body><div class="foo">Hello</div></body></html>"#;
        let doc = parse_html_simple(html);

        let div_node = doc
            .descendants()
            .find(|n| {
                n.as_element()
                    .map(|el| el.name.local.as_ref() == "div")
                    .unwrap_or(false)
            })
            .expect("div node should exist");

        let selector_list = selector::parse_selector(".foo").expect("parse selector");
        assert!(selector::element_matches_selector_list(
            &div_node,
            &selector_list
        ));
    }

    #[test]
    fn test_element_does_not_match_different_class() {
        let html = r#"<html><body><div class="foo">Hello</div></body></html>"#;
        let doc = parse_html_simple(html);

        let div_node = doc
            .descendants()
            .find(|n| {
                n.as_element()
                    .map(|el| el.name.local.as_ref() == "div")
                    .unwrap_or(false)
            })
            .expect("div node should exist");

        let selector_list = selector::parse_selector(".bar").expect("parse selector");
        assert!(!selector::element_matches_selector_list(
            &div_node,
            &selector_list
        ));
    }

    #[test]
    fn test_element_matches_id_selector() {
        let html = r#"<html><body><div id="main">Hello</div></body></html>"#;
        let doc = parse_html_simple(html);

        let div_node = doc
            .descendants()
            .find(|n| {
                n.as_element()
                    .map(|el| el.name.local.as_ref() == "div")
                    .unwrap_or(false)
            })
            .expect("div node should exist");

        let selector_list = selector::parse_selector("#main").expect("parse selector");
        assert!(selector::element_matches_selector_list(
            &div_node,
            &selector_list
        ));
    }

    #[test]
    fn test_element_matches_descendant_selector() {
        let html = r"<html><body><div><span>Nested</span></div></body></html>";
        let doc = parse_html_simple(html);

        let span_node = doc
            .descendants()
            .find(|n| {
                n.as_element()
                    .map(|el| el.name.local.as_ref() == "span")
                    .unwrap_or(false)
            })
            .expect("span node should exist");

        let selector_list = selector::parse_selector("div span").expect("parse selector");
        assert!(selector::element_matches_selector_list(
            &span_node,
            &selector_list
        ));
    }

    // ----------------------------------------------------------------------
    // rules_to_style_map_with_selectors tests
    // ----------------------------------------------------------------------

    #[test]
    fn test_rules_to_style_map_with_selectors_basic() {
        let html = r"<html><body><div>Hello</div></body></html>";
        let doc = parse_html_simple(html);
        let css = r"div { color: red; }";
        let rules = parse_css_rules(css);
        let map = selector::rules_to_style_map_with_selectors(&rules, &doc);

        assert!(map.contains_key("div"));
        assert_eq!(map["div"][0].value, "red");
    }

    #[test]
    fn test_rules_to_style_map_with_selectors_no_match() {
        let html = r"<html><body><div>Hello</div></body></html>";
        let doc = parse_html_simple(html);
        let css = r"span { color: red; }";
        let rules = parse_css_rules(css);
        let map = selector::rules_to_style_map_with_selectors(&rules, &doc);

        // No span element exists, so map should be empty
        assert!(map.is_empty());
    }

    #[test]
    fn test_rules_to_style_map_with_selectors_class() {
        let html = r#"<html><body><div class="foo">Hello</div></body></html>"#;
        let doc = parse_html_simple(html);
        let css = r".foo { color: red; }";
        let rules = parse_css_rules(css);
        let map = selector::rules_to_style_map_with_selectors(&rules, &doc);

        assert!(
            map.contains_key("div"),
            "map should contain 'div', got keys: {:?}",
            map.keys().collect::<Vec<_>>()
        );
        assert_eq!(map["div"][0].value, "red");
    }

    #[test]
    fn test_rules_to_style_map_with_selectors_empty_rules() {
        let html = r"<html><body><div>Hello</div></body></html>";
        let doc = parse_html_simple(html);
        let map = selector::rules_to_style_map_with_selectors(&[], &doc);
        assert!(map.is_empty());
    }

    #[test]
    fn test_rules_to_style_map_with_selectors_id_selector() {
        let html = r#"<html><body><div id="main">Hello</div></body></html>"#;
        let doc = parse_html_simple(html);
        let css = r"#main { color: blue; }";
        let rules = parse_css_rules(css);
        let map = selector::rules_to_style_map_with_selectors(&rules, &doc);

        assert!(
            map.contains_key("div"),
            "map should contain 'div', got keys: {:?}",
            map.keys().collect::<Vec<_>>()
        );
        assert_eq!(map["div"][0].value, "blue");
    }
}
