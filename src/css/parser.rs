//! CSS 解析器 - 解析 CSS 样式表
//!
//! 将 CSS 文本解析为样式规则

use super::stylesheet::{Rule, Selector, SelectorKind, Stylesheet, Property};
use super::values::{Color, Length, LengthUnit};
use log::{debug, trace, warn};

/// CSS 解析器
pub struct CssParser {
    /// 输入字符串
    input: String,
    /// 当前位置
    pos: usize,
}

impl CssParser {
    /// 创建新的解析器
    pub fn new(input: &str) -> Self {
        Self {
            input: input.to_string(),
            pos: 0,
        }
    }

    /// 解析 CSS
    pub fn parse(&mut self) -> Result<Stylesheet, String> {
        let mut stylesheet = Stylesheet::default();

        // 移除注释
        self.remove_comments();
        trace!("解析 CSS: {} 字符", self.input.len());

        while !self.is_at_end() {
            // 跳过空白
            self.skip_whitespace();

            if self.is_at_end() {
                break;
            }

            // 解析规则
            match self.parse_rule() {
                Ok(rule) => {
                    debug!("解析规则: {:?}", rule.selector);
                    stylesheet.add_rule(rule);
                }
                Err(e) => {
                    warn!("解析规则失败: {}", e);
                    self.skip_to_next_rule();
                }
            }
        }

        Ok(stylesheet)
    }

    /// 移除注释
    fn remove_comments(&mut self) {
        let mut result = String::new();
        let mut in_comment = false;
        let chars: Vec<char> = self.input.chars().collect();

        for i in 0..chars.len() {
            if i + 1 < chars.len() && chars[i] == '/' && chars[i + 1] == '*' {
                in_comment = true;
                continue;
            }
            if in_comment && i + 1 < chars.len() && chars[i] == '*' && chars[i + 1] == '/' {
                in_comment = false;
                continue;
            }
            if !in_comment {
                result.push(chars[i]);
            }
        }

        self.input = result;
    }

    /// 解析规则
    fn parse_rule(&mut self) -> Result<Rule, String> {
        // 解析选择器
        let selector = self.parse_selector()?;

        // 跳过空白
        self.skip_whitespace();

        // 检查并跳过左花括号
        if !self.check('{') {
            return Err("期望 '{'".to_string());
        }
        self.advance();

        // 跳过空白
        self.skip_whitespace();

        // 解析声明
        let declarations = self.parse_declarations()?;

        // 检查并跳过右花括号
        if !self.check('}') {
            return Err("期望 '}'".to_string());
        }
        self.advance();

        Ok(Rule {
            selector,
            declarations,
        })
    }

    /// 解析选择器
    fn parse_selector(&mut self) -> Result<Selector, String> {
        self.skip_whitespace();

        let start = self.pos;
        let mut selector = None;

        while !self.is_at_end() && !self.check('{') {
            let ch = self.current();

            match ch {
                '.' => {
                    // 类选择器
                    self.advance();
                    let class_name = self.parse_identifier()?;
                    selector = Some(Selector::class(&class_name));
                }
                '#' => {
                    // ID 选择器
                    self.advance();
                    let id_name = self.parse_identifier()?;
                    selector = Some(Selector::id(&id_name));
                }
                '*' => {
                    // 通配符选择器
                    self.advance();
                    selector = Some(Selector::universal());
                }
                ' ' | '\t' | '\n' => {
                    // 空格分隔，可能是后代选择器
                    if selector.is_some() {
                        self.skip_whitespace();
                        if !self.check('{') {
                            let child = self.parse_selector()?;
                            selector = Some(Selector {
                                kind: SelectorKind::Descendant(
                                    Box::new(selector.take().unwrap()),
                                    Box::new(child),
                                ),
                                specificity: (0, 0, 0),
                            });
                        } else {
                            break;
                        }
                    } else {
                        self.skip_whitespace();
                    }
                }
                c if c.is_ascii_alphabetic() => {
                    // 标签选择器
                    let tag_name = self.parse_identifier()?;
                    if selector.is_none() {
                        selector = Some(Selector::tag(&tag_name));
                    }
                }
                _ => {
                    self.advance();
                }
            }
        }

        selector.ok_or_else(|| "未找到选择器".to_string())
    }

    /// 解析声明块
    fn parse_declarations(&mut self) -> Result<Vec<(String, Property)>, String> {
        let mut declarations = Vec::new();

        loop {
            self.skip_whitespace();

            if self.check('}') || self.is_at_end() {
                break;
            }

            // 解析属性名
            let prop_name = self.parse_identifier()?;
            trace!("解析属性: {}", prop_name);

            // 跳过空白
            self.skip_whitespace();

            // 检查冒号
            if !self.check(':') {
                warn!("期望 ':', 跳过声明");
                self.skip_to_semicolon();
                continue;
            }
            self.advance();

            // 跳过空白
            self.skip_whitespace();

            // 解析属性值
            let value = self.parse_property_value()?;

            declarations.push((prop_name, value));

            // 跳过空白和分号
            self.skip_whitespace();
            if self.check(';') {
                self.advance();
            }
            self.skip_whitespace();
        }

        Ok(declarations)
    }

    /// 解析属性值
    fn parse_property_value(&mut self) -> Result<Property, String> {
        let start = self.pos;

        // 检查颜色值 (#xxx 或 #xxxxxx)
        if self.check('#') {
            self.advance();
            let hex = self.read_while(|c| c.is_ascii_hexdigit());
            if !hex.is_empty() {
                return Ok(Property::Color(Color::from_hex(&hex)));
            }
        }

        // 检查数值
        if self.current().is_ascii_digit() || self.current() == '.' {
            let num_str = self.parse_number()?;
            let unit = self.parse_unit()?;
            return Ok(Property::Length(Length {
                value: num_str.parse().unwrap_or(0.0),
                unit,
            }));
        }

        // 检查关键字
        let keyword = self.parse_identifier()?;
        if !keyword.is_empty() {
            return Ok(Property::Keyword(keyword));
        }

        Ok(Property::Keyword(self.input[start..self.pos].trim().to_string()))
    }

    /// 解析标识符
    fn parse_identifier(&mut self) -> Result<String, String> {
        let start = self.pos;

        // 跳过初始空白
        self.skip_whitespace();

        while !self.is_at_end() {
            let c = self.current();
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                self.advance();
            } else {
                break;
            }
        }

        Ok(self.input[start..self.pos].trim().to_string())
    }

    /// 解析数字
    fn parse_number(&mut self) -> Result<String, String> {
        let start = self.pos;
        let mut has_dot = false;

        while !self.is_at_end() {
            let c = self.current();
            if c.is_ascii_digit() {
                self.advance();
            } else if c == '.' && !has_dot {
                has_dot = true;
                self.advance();
            } else {
                break;
            }
        }

        Ok(self.input[start..self.pos].to_string())
    }

    /// 解析单位
    fn parse_unit(&mut self) -> Result<LengthUnit, String> {
        let unit = self.input[self.pos..]
            .chars()
            .take_while(|c| c.is_ascii_alphabetic())
            .collect::<String>()
            .to_lowercase();

        if !unit.is_empty() {
            self.pos += unit.len();
        }

        match unit.as_str() {
            "px" => Ok(LengthUnit::Px),
            "em" => Ok(LengthUnit::Em),
            "rem" => Ok(LengthUnit::Rem),
            "%" => Ok(LengthUnit::Percent),
            "vw" => Ok(LengthUnit::Vw),
            "vh" => Ok(LengthUnit::Vh),
            "pt" => Ok(LengthUnit::Pt),
            "cm" => Ok(LengthUnit::Cm),
            "mm" => Ok(LengthUnit::Mm),
            "in" => Ok(LengthUnit::In),
            _ => Ok(LengthUnit::Px), // 默认像素
        }
    }

    /// 跳过空白
    fn skip_whitespace(&mut self) {
        while !self.is_at_end() && self.current().is_whitespace() {
            self.advance();
        }
    }

    /// 跳过到下一个规则
    fn skip_to_next_rule(&mut self) {
        while !self.is_at_end() {
            if self.check('}') {
                self.advance();
                break;
            }
            self.advance();
        }
    }

    /// 跳过到分号
    fn skip_to_semicolon(&mut self) {
        while !self.is_at_end() && !self.check(';') && !self.check('}') {
            self.advance();
        }
        if self.check(';') {
            self.advance();
        }
    }

    /// 读取满足条件的字符
    fn read_while<F>(&mut self, f: F) -> String
    where
        F: Fn(char) -> bool,
    {
        let start = self.pos;
        while !self.is_at_end() && f(self.current()) {
            self.advance();
        }
        self.input[start..self.pos].to_string()
    }

    /// 检查当前字符
    fn check(&self, expected: char) -> bool {
        !self.is_at_end() && self.current() == expected
    }

    /// 当前字符
    fn current(&self) -> char {
        self.input.chars().nth(self.pos).unwrap_or('\0')
    }

    /// 前进
    fn advance(&mut self) {
        if !self.is_at_end() {
            self.pos += 1;
        }
    }

    /// 是否到达末尾
    fn is_at_end(&self) -> bool {
        self.pos >= self.input.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_rule() {
        let css = "div { color: red; }";
        let mut parser = CssParser::new(css);
        let result = parser.parse();
        assert!(result.is_ok());
    }

    #[test]
    fn test_parse_multiple_rules() {
        let css = "div { color: red; } .class { font-size: 14px; }";
        let mut parser = CssParser::new(css);
        let result = parser.parse();
        assert!(result.is_ok());
        let stylesheet = result.unwrap();
        assert_eq!(stylesheet.rules().len(), 2);
    }
}
