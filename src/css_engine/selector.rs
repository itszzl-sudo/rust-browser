//! CSS 选择器匹配引擎 —— 使用 selectors crate 实现完整的 CSS 选择器支持
//!
//! 实现了 `selectors::SelectorImpl` 和 `selectors::Element` trait，
//! 适配 kuchiki 的 `NodeRef` 作为 DOM 元素。

use cssparser::ToCss;
use kuchiki::NodeRef;
use precomputed_hash::PrecomputedHash;
use selectors::attr::{AttrSelectorOperation, CaseSensitivity, NamespaceConstraint};
use selectors::context::{MatchingContext, QuirksMode, SelectorCaches};
use selectors::matching::{matches_selector_list, ElementSelectorFlags};
use selectors::parser::{
    NonTSPseudoClass, Parser as SelectorParser, PseudoElement, SelectorImpl, SelectorList,
};
use selectors::Element as SelectorElement;
use selectors::OpaqueElement;
use std::borrow::Borrow;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::rc::Rc;

use super::CssRule;

// ---------------------------------------------------------------------------
// 新类型包装 —— 满足 SelectorImpl 的 trait 约束
// ---------------------------------------------------------------------------

/// 包装 `String`，实现 `ToCss`、`PrecomputedHash` 等 trait
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct CssString(String);

impl CssString {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'a> From<&'a str> for CssString {
    fn from(s: &'a str) -> Self {
        CssString(s.to_string())
    }
}

impl From<String> for CssString {
    fn from(s: String) -> Self {
        CssString(s)
    }
}

impl AsRef<str> for CssString {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl Borrow<str> for CssString {
    fn borrow(&self) -> &str {
        &self.0
    }
}

impl ToCss for CssString {
    fn to_css<W: fmt::Write>(&self, dest: &mut W) -> fmt::Result {
        dest.write_str(&self.0)
    }
}

/// 预计算哈希：使用标准哈希
fn compute_hash(s: &str) -> u32 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    s.hash(&mut hasher);
    hasher.finish() as u32
}

// PrecomputedHash 是 selectors crate 对 precomputed-hash crate 依赖的重新导出
// 但我们直接实现它。我们需要检查它是否从 selectors 重新导出。
// 实际上在 selectors 0.27 中，PrecomputedHash 来自 precomputed-hash crate。
// 但我们可以直接在 CssString 上实现这个 trait。

impl PrecomputedHash for CssString {
    fn precomputed_hash(&self) -> u32 {
        compute_hash(&self.0)
    }
}

// 默认为 NamespaceUrl 实现 Default
impl Default for CssString {
    fn default() -> Self {
        CssString(String::new())
    }
}

// ---------------------------------------------------------------------------
// SelectorImpl 实现
// ---------------------------------------------------------------------------

/// selectors crate 的 `SelectorImpl`，用于 kuchiki 节点
#[derive(Clone, Debug)]
pub struct KuchikiSelectorImpl;

impl SelectorImpl for KuchikiSelectorImpl {
    type ExtraMatchingData<'a> = ();
    type AttrValue = CssString;
    type Identifier = CssString;
    type LocalName = CssString;
    type NamespaceUrl = CssString;
    type NamespacePrefix = CssString;
    type BorrowedNamespaceUrl = str;
    type BorrowedLocalName = str;
    type NonTSPseudoClass = KuchikiNonTSPseudoClass;
    type PseudoElement = KuchikiPseudoElement;
}

// ---------------------------------------------------------------------------
// 伪类 / 伪元素
// ---------------------------------------------------------------------------

/// 非树结构伪类（本项目暂时不支持）
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum KuchikiNonTSPseudoClass {
    /// `:hover`
    Hover,
    /// `:active`
    Active,
    /// `:focus`
    Focus,
    /// 其他未知伪类
    Unknown(String),
}

impl NonTSPseudoClass for KuchikiNonTSPseudoClass {
    type Impl = KuchikiSelectorImpl;

    fn is_active_or_hover(&self) -> bool {
        matches!(
            self,
            KuchikiNonTSPseudoClass::Hover | KuchikiNonTSPseudoClass::Active
        )
    }

    fn is_user_action_state(&self) -> bool {
        matches!(
            self,
            KuchikiNonTSPseudoClass::Hover
                | KuchikiNonTSPseudoClass::Active
                | KuchikiNonTSPseudoClass::Focus
        )
    }
}

impl ToCss for KuchikiNonTSPseudoClass {
    fn to_css<W: fmt::Write>(&self, dest: &mut W) -> fmt::Result {
        match self {
            KuchikiNonTSPseudoClass::Hover => dest.write_str(":hover"),
            KuchikiNonTSPseudoClass::Active => dest.write_str(":active"),
            KuchikiNonTSPseudoClass::Focus => dest.write_str(":focus"),
            KuchikiNonTSPseudoClass::Unknown(s) => write!(dest, ":{s}"),
        }
    }
}

/// 伪元素（本项目暂时不支持）
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum KuchikiPseudoElement {
    /// `::before`
    Before,
    /// `::after`
    After,
    /// 其他未知伪元素
    Unknown(String),
}

impl PseudoElement for KuchikiPseudoElement {
    type Impl = KuchikiSelectorImpl;
}

impl ToCss for KuchikiPseudoElement {
    fn to_css<W: fmt::Write>(&self, dest: &mut W) -> fmt::Result {
        match self {
            KuchikiPseudoElement::Before => dest.write_str("::before"),
            KuchikiPseudoElement::After => dest.write_str("::after"),
            KuchikiPseudoElement::Unknown(s) => write!(dest, "::{s}"),
        }
    }
}

// ---------------------------------------------------------------------------
// SelectorParser 实现（用于从 CSS 文本解析选择器）
// ---------------------------------------------------------------------------

/// `selectors::Parser` 实现，用于解析选择器字符串
pub struct KuchikiSelectorParser;

impl<'i> SelectorParser<'i> for KuchikiSelectorParser {
    type Impl = KuchikiSelectorImpl;
    type Error = selectors::parser::SelectorParseErrorKind<'i>;

    fn parse_slotted(&self) -> bool {
        false
    }

    fn parse_part(&self) -> bool {
        false
    }

    fn parse_nth_child_of(&self) -> bool {
        false
    }

    fn parse_is_and_where(&self) -> bool {
        true
    }

    fn parse_has(&self) -> bool {
        false
    }

    fn parse_parent_selector(&self) -> bool {
        false
    }

    fn parse_host(&self) -> bool {
        false
    }

    fn default_namespace(&self) -> Option<<Self::Impl as SelectorImpl>::NamespaceUrl> {
        None
    }

    fn namespace_for_prefix(
        &self,
        _prefix: &<Self::Impl as SelectorImpl>::NamespacePrefix,
    ) -> Option<<Self::Impl as SelectorImpl>::NamespaceUrl> {
        None
    }

    fn parse_non_ts_pseudo_class(
        &self,
        _location: cssparser::SourceLocation,
        name: cssparser::CowRcStr<'i>,
    ) -> Result<KuchikiNonTSPseudoClass, selectors::parser::SelectorParseError<'i>> {
        match name.as_ref() {
            "hover" => Ok(KuchikiNonTSPseudoClass::Hover),
            "active" => Ok(KuchikiNonTSPseudoClass::Active),
            "focus" => Ok(KuchikiNonTSPseudoClass::Focus),
            other => Ok(KuchikiNonTSPseudoClass::Unknown(other.to_string())),
        }
    }

    fn parse_pseudo_element(
        &self,
        _location: cssparser::SourceLocation,
        name: cssparser::CowRcStr<'i>,
    ) -> Result<KuchikiPseudoElement, selectors::parser::SelectorParseError<'i>> {
        match name.as_ref() {
            "before" => Ok(KuchikiPseudoElement::Before),
            "after" => Ok(KuchikiPseudoElement::After),
            other => Ok(KuchikiPseudoElement::Unknown(other.to_string())),
        }
    }
}

// ---------------------------------------------------------------------------
// selectors::Element 实现 —— 包装 kuchiki 元素节点
// ---------------------------------------------------------------------------

/// 包装 kuchiki 元素节点的选择器匹配元素
///
/// 内部持有 `NodeRef`（引用计数的智能指针），所以元素可以独立于原始节点存在。
#[derive(Clone, Debug)]
pub struct KuchikiElement {
    /// kuchiki 元素节点的克隆
    node: NodeRef,
}

impl KuchikiElement {
    /// 从 `&NodeRef` 创建一个 `KuchikiElement`。
    /// 克隆底层节点（增加引用计数）。
    /// 调用者需确保该节点是一个元素节点。
    pub fn new(node: &NodeRef) -> Self {
        KuchikiElement { node: node.clone() }
    }

    /// 获取底层 `NodeRef`
    pub fn node(&self) -> &NodeRef {
        &self.node
    }
}

impl SelectorElement for KuchikiElement {
    type Impl = KuchikiSelectorImpl;

    fn opaque(&self) -> OpaqueElement {
        // 使用 NodeRef 的内部指针作为唯一标识
        let ptr = Rc::as_ptr(&self.node.0) as *const ();
        OpaqueElement::from_non_null_ptr(std::ptr::NonNull::new(ptr as *mut ()).unwrap())
    }

    fn parent_element(&self) -> Option<Self> {
        self.node.parent().and_then(|parent| {
            if parent.as_element().is_some() {
                Some(KuchikiElement { node: parent })
            } else {
                None
            }
        })
    }

    fn parent_node_is_shadow_root(&self) -> bool {
        false
    }

    fn containing_shadow_host(&self) -> Option<Self> {
        None
    }

    fn is_pseudo_element(&self) -> bool {
        false
    }

    fn prev_sibling_element(&self) -> Option<Self> {
        let mut prev = self.node.previous_sibling();
        while let Some(ref sibling) = prev {
            if sibling.as_element().is_some() {
                return Some(KuchikiElement {
                    node: sibling.clone(),
                });
            }
            prev = sibling.previous_sibling();
        }
        None
    }

    fn next_sibling_element(&self) -> Option<Self> {
        let mut next = self.node.next_sibling();
        while let Some(ref sibling) = next {
            if sibling.as_element().is_some() {
                return Some(KuchikiElement {
                    node: sibling.clone(),
                });
            }
            next = sibling.next_sibling();
        }
        None
    }

    fn first_element_child(&self) -> Option<Self> {
        for child in self.node.children() {
            if child.as_element().is_some() {
                return Some(KuchikiElement { node: child });
            }
        }
        None
    }

    fn is_html_element_in_html_document(&self) -> bool {
        // kuchiki 默认解析文本类 HTML，所以假设是 html 文档
        true
    }

    fn has_local_name(&self, local_name: &<Self::Impl as SelectorImpl>::BorrowedLocalName) -> bool {
        self.node
            .as_element()
            .map_or(false, |el| el.name.local.as_ref() == local_name)
    }

    fn has_namespace(&self, ns: &<Self::Impl as SelectorImpl>::BorrowedNamespaceUrl) -> bool {
        self.node
            .as_element()
            .map_or(false, |el| el.name.ns.as_ref() == ns)
    }

    fn is_same_type(&self, other: &Self) -> bool {
        let self_el = self.node.as_element();
        let other_el = other.node.as_element();
        match (self_el, other_el) {
            (Some(a), Some(b)) => a.name.local == b.name.local && a.name.ns == b.name.ns,
            _ => false,
        }
    }

    fn attr_matches(
        &self,
        ns: &NamespaceConstraint<&<Self::Impl as SelectorImpl>::NamespaceUrl>,
        local_name: &<Self::Impl as SelectorImpl>::LocalName,
        operation: &AttrSelectorOperation<&<Self::Impl as SelectorImpl>::AttrValue>,
    ) -> bool {
        use selectors::attr::AttrSelectorOperator;
        use selectors::attr::NamespaceConstraint;

        let element = match self.node.as_element() {
            Some(el) => el,
            None => return false,
        };

        // 检查命名空间约束
        let ns_match = match ns {
            NamespaceConstraint::Any => true,
            NamespaceConstraint::Specific(ns_str) => element.name.ns.as_ref() == ns_str.as_ref(),
        };
        if !ns_match {
            return false;
        }

        // 获取属性值
        let attrs = element.attributes.borrow();
        let attr_value = attrs.get(local_name.as_str());

        match operation {
            AttrSelectorOperation::Exists => attr_value.is_some(),
            AttrSelectorOperation::WithValue {
                operator,
                case_sensitivity,
                value: expected_value,
            } => {
                let actual = match attr_value {
                    Some(v) => v,
                    None => return false,
                };

                let cs = *case_sensitivity;
                match operator {
                    AttrSelectorOperator::Equal => CaseSensitivity::eq(
                        cs,
                        actual.as_bytes(),
                        expected_value.as_ref().as_bytes(),
                    ),
                    AttrSelectorOperator::Includes => actual.split_whitespace().any(|part| {
                        CaseSensitivity::eq(cs, part.as_bytes(), expected_value.as_ref().as_bytes())
                    }),
                    AttrSelectorOperator::DashMatch => {
                        let actual = actual.as_bytes();
                        let expected = expected_value.as_ref().as_bytes();
                        CaseSensitivity::eq(cs, actual, expected)
                            || (actual.len() > expected.len()
                                && actual[expected.len()] == b'-'
                                && CaseSensitivity::eq(cs, &actual[..expected.len()], expected))
                    }
                    AttrSelectorOperator::Prefix => {
                        let actual = actual.as_bytes();
                        let expected = expected_value.as_ref().as_bytes();
                        !expected.is_empty()
                            && actual.len() >= expected.len()
                            && CaseSensitivity::eq(cs, &actual[..expected.len()], expected)
                    }
                    AttrSelectorOperator::Suffix => {
                        let actual = actual.as_bytes();
                        let expected = expected_value.as_ref().as_bytes();
                        !expected.is_empty()
                            && actual.len() >= expected.len()
                            && CaseSensitivity::eq(
                                cs,
                                &actual[actual.len() - expected.len()..],
                                expected,
                            )
                    }
                    AttrSelectorOperator::Substring => CaseSensitivity::contains(
                        cs,
                        actual.as_ref(),
                        expected_value.as_ref().as_ref(),
                    ),
                }
            }
        }
    }

    fn match_non_ts_pseudo_class(
        &self,
        _pc: &<Self::Impl as SelectorImpl>::NonTSPseudoClass,
        _context: &mut MatchingContext<'_, Self::Impl>,
    ) -> bool {
        // 本项目暂时不实现交互式伪类匹配
        false
    }

    fn match_pseudo_element(
        &self,
        _pe: &<Self::Impl as SelectorImpl>::PseudoElement,
        _context: &mut MatchingContext<'_, Self::Impl>,
    ) -> bool {
        false
    }

    fn apply_selector_flags(&self, _flags: ElementSelectorFlags) {
        // 本项目不涉及选择器标志
    }

    fn is_link(&self) -> bool {
        // 匹配 <a> 标签
        self.node
            .as_element()
            .map_or(false, |el| el.name.local.as_ref() == "a")
    }

    fn is_html_slot_element(&self) -> bool {
        false
    }

    fn has_id(
        &self,
        id: &<Self::Impl as SelectorImpl>::Identifier,
        case_sensitivity: CaseSensitivity,
    ) -> bool {
        self.node.as_element().map_or(false, |el| {
            let attrs = el.attributes.borrow();
            attrs.get("id").map_or(false, |actual_id| {
                CaseSensitivity::eq(
                    case_sensitivity,
                    actual_id.as_bytes(),
                    id.as_str().as_bytes(),
                )
            })
        })
    }

    fn has_class(
        &self,
        name: &<Self::Impl as SelectorImpl>::Identifier,
        case_sensitivity: CaseSensitivity,
    ) -> bool {
        self.node.as_element().map_or(false, |el| {
            let attrs = el.attributes.borrow();
            attrs.get("class").map_or(false, |class_str| {
                class_str.split_whitespace().any(|part| {
                    CaseSensitivity::eq(case_sensitivity, part.as_bytes(), name.as_str().as_bytes())
                })
            })
        })
    }

    fn has_custom_state(&self, _name: &<Self::Impl as SelectorImpl>::Identifier) -> bool {
        false
    }

    fn imported_part(
        &self,
        _name: &<Self::Impl as SelectorImpl>::Identifier,
    ) -> Option<<Self::Impl as SelectorImpl>::Identifier> {
        None
    }

    fn is_part(&self, _name: &<Self::Impl as SelectorImpl>::Identifier) -> bool {
        false
    }

    fn is_empty(&self) -> bool {
        // `:empty` 伪类：没有子元素且没有非零长度文本节点
        for child in self.node.children() {
            if child.as_element().is_some() {
                return false;
            }
            if let Some(text) = child.as_text() {
                if !text.borrow().trim().is_empty() {
                    return false;
                }
            }
        }
        true
    }

    fn is_root(&self) -> bool {
        // `:root` 伪类：父节点是文档节点
        self.node
            .parent()
            .map_or(false, |parent| parent.as_document().is_some())
    }

    fn add_element_unique_hashes(&self, _filter: &mut selectors::bloom::BloomFilter) -> bool {
        // 本项目暂不实现 Bloom filter 加速
        false
    }
}

// ---------------------------------------------------------------------------
// 公开 API
// ---------------------------------------------------------------------------

/// 解析 CSS 选择器字符串为 `SelectorList`
pub fn parse_selector(selector_str: &str) -> Result<SelectorList<KuchikiSelectorImpl>, String> {
    use cssparser::Parser as CssParser;

    let mut input = cssparser::ParserInput::new(selector_str);
    let mut parser = CssParser::new(&mut input);
    SelectorList::parse(
        &KuchikiSelectorParser,
        &mut parser,
        selectors::parser::ParseRelative::No,
    )
    .map_err(|e| format!("选择器解析失败: {:?}", e))
}

/// 判断一个 kuchiki 元素节点是否匹配给定的选择器列表
pub fn element_matches_selector_list(
    node: &NodeRef,
    selector_list: &SelectorList<KuchikiSelectorImpl>,
) -> bool {
    let mut selector_caches = SelectorCaches::default();
    let mut context = MatchingContext::new(
        selectors::context::MatchingMode::Normal,
        None,
        &mut selector_caches,
        QuirksMode::NoQuirks,
        selectors::context::NeedsSelectorFlags::No,
        selectors::context::MatchingForInvalidation::No,
    );

    let element = KuchikiElement::new(node);
    matches_selector_list(selector_list, &element, &mut context)
}

/// 对文档中所有元素，根据 CSS 规则进行选择器匹配，返回按标签名分组的声明映射
///
/// `rules`: 已解析的 CSS 规则
/// `doc_ref`: kuchiki 文档节点（从此遍历所有后代元素）
pub fn rules_to_style_map_with_selectors(rules: &[CssRule], doc_ref: &NodeRef) -> super::StyleMap {
    use std::collections::HashMap;
    let mut map: super::StyleMap = HashMap::new();

    // 对每个 CSS 规则解析其选择器（跳过解析失败的）
    let parsed_rules: Vec<(SelectorList<KuchikiSelectorImpl>, &[super::Declaration])> = rules
        .iter()
        .filter_map(|rule| {
            parse_selector(&rule.selector)
                .ok()
                .map(|sel_list| (sel_list, rule.declarations.as_slice()))
        })
        .collect();

    if parsed_rules.is_empty() {
        return map;
    }

    // 遍历文档中的所有元素节点
    for node in doc_ref.descendants() {
        if node.as_element().is_none() {
            continue;
        }

        let tag_name = node
            .as_element()
            .map(|el| el.name.local.to_string())
            .unwrap_or_default();

        // 对每个规则检查是否匹配
        for (selector_list, decls) in &parsed_rules {
            if element_matches_selector_list(&node, selector_list) {
                map.entry(tag_name.clone())
                    .or_default()
                    .extend(decls.iter().cloned());
            }
        }
    }

    map
}
