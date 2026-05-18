//! JS 引擎 —— 多后端支持（obscura-js / Boa）
//!
//! 通过 feature 切换后端：
//! - `boa`（默认）：基于 Boa Engine（纯 Rust，无 V8 依赖）
//! - `js`：基于 obscura-js（deno_core/V8）
//!
//! # 公开 API
//!
//! 无论选择哪个后端，外部代码都使用统一的 `JsEngine` 类型，
//! 通过 `new()` / `initialize()` / `evaluate()` / `is_ready()` 等
//! 方法操作，不依赖后端细节。

use log::{info, warn};

// ═══════════════════════════════════════════════════════════════
// Boa 后端（默认，纯 Rust）
// ═══════════════════════════════════════════════════════════════

#[cfg(feature = "boa")]
mod backend {
    use boa_engine::{Context, Source};
    use log::info;

    pub struct BoaJsEngine {
        context: Option<Context>,
        url: String,
    }

    impl BoaJsEngine {
        pub fn new() -> Self {
            info!("初始化 JS 引擎 (Boa Engine)");
            Self {
                context: None,
                url: "about:blank".to_string(),
            }
        }

        pub fn initialize(&mut self, url: &str) -> Result<(), String> {
            if self.context.is_some() {
                return Ok(());
            }
            info!("启动 Boa 运行时, URL: {}", url);
            self.url = url.to_string();

            let mut context = Context::default();

            // 注入基础的 console 对象
            let console_js = r#"
                globalThis.console = {
                    log: (...args) => {},
                    warn: (...args) => {},
                    error: (...args) => {},
                    info: (...args) => {},
                    debug: () => {},
                    trace: () => {},
                };
                globalThis.setTimeout = (fn, ms) => { if (typeof fn === 'function') fn(); };
                globalThis.setInterval = (fn, ms) => { if (typeof fn === 'function') fn(); };
                globalThis.clearTimeout = () => {};
                globalThis.clearInterval = () => {};
                globalThis.requestAnimationFrame = (fn) => { if (typeof fn === 'function') fn(0); };
                globalThis.cancelAnimationFrame = () => {};
                globalThis.queueMicrotask = (fn) => { if (typeof fn === 'function') Promise.resolve().then(fn); };

                // 简化的 document 对象
                globalThis.document = {
                    title: '',
                    URL: '',
                    createElement: (tag) => ({ tagName: tag, style: {}, setAttribute: () => {}, getAttribute: () => null, appendChild: () => {}, textContent: '' }),
                    createTextNode: (text) => ({ nodeType: 3, textContent: text, data: text }),
                    getElementById: () => null,
                    querySelector: () => null,
                    querySelectorAll: () => [],
                    body: { appendChild: () => {}, style: {} },
                    head: { appendChild: () => {} },
                    documentElement: { style: {} },
                    addEventListener: () => {},
                    removeEventListener: () => {},
                    dispatchEvent: () => true,
                };
                globalThis.window = globalThis;
                globalThis.self = globalThis;
                globalThis.location = { href: '', protocol: 'https:', host: '', hostname: '', pathname: '/', search: '', hash: '', origin: '' };
                globalThis.navigator = { userAgent: 'RustBrowser/1.0', platform: 'Win32', language: 'zh-CN' };
                globalThis.HTMLElement = function() {};
                globalThis.Event = class Event { constructor(type) { this.type = type; this.defaultPrevented = false; } preventDefault() { this.defaultPrevented = true; } };
                globalThis.CustomEvent = class CustomEvent extends Event { constructor(type, detail) { super(type); this.detail = detail?.detail; } };
                globalThis.MouseEvent = class MouseEvent extends Event { constructor(type, init) { super(type); this.clientX = init?.clientX || 0; this.clientY = init?.clientY || 0; } };
                globalThis.KeyboardEvent = class KeyboardEvent extends Event { constructor(type, init) { super(type); this.key = init?.key || ''; this.code = init?.code || ''; } };
                globalThis.JSON = JSON;
                globalThis.Math = Math;
                globalThis.parseInt = parseInt;
                globalThis.parseFloat = parseFloat;
                globalThis.isNaN = isNaN;
                globalThis.isFinite = isFinite;
                globalThis.encodeURI = encodeURI;
                globalThis.decodeURI = decodeURI;
                globalThis.encodeURIComponent = encodeURIComponent;
                globalThis.decodeURIComponent = decodeURIComponent;
                globalThis.Array = Array;
                globalThis.Object = Object;
                globalThis.String = String;
                globalThis.Number = Number;
                globalThis.Boolean = Boolean;
                globalThis.Function = Function;
                globalThis.Date = Date;
                globalThis.RegExp = RegExp;
                globalThis.Error = Error;
                globalThis.TypeError = TypeError;
                globalThis.ReferenceError = ReferenceError;
                globalThis.SyntaxError = SyntaxError;
                globalThis.Promise = Promise;
                globalThis.Map = Map;
                globalThis.Set = Set;
                globalThis.Symbol = Symbol;
            "#;

            match context.eval(Source::from_bytes(&console_js)) {
                Ok(_) => info!("Boa 运行时初始化完成"),
                Err(e) => log::warn!("Boa 运行时初始化警告: {}", e),
            }

            self.context = Some(context);
            info!("JS 引擎已就绪 (Boa)");
            Ok(())
        }

        pub fn evaluate(&mut self, code: &str) -> Result<String, String> {
            match self.context.as_mut() {
                Some(ctx) => {
                    let result = ctx
                        .eval(Source::from_bytes(code.as_bytes()))
                        .map_err(|e| format!("Boa JS Error: {}", e))?;
                    let js_string = result
                        .to_string(&mut *ctx)
                        .map_err(|e| format!("Boa toString Error: {}", e))?;
                    Ok(js_string
                        .to_std_string()
                        .map_err(|e| format!("Boa std string Error: {}", e))?)
                }
                None => Err("JS 引擎未初始化".to_string()),
            }
        }

        pub fn is_ready(&self) -> bool {
            self.context.is_some()
        }

        pub fn set_url(&mut self, url: &str) {
            self.url = url.to_string();
        }

        pub fn dispatch_event(
            &mut self,
            _node_id: u32,
            _event_type: &str,
        ) -> Result<String, String> {
            // Boa 中没有 DOM，dispatchEvent 简化为 no-op
            Ok("ok".to_string())
        }
    }
}

// ═══════════════════════════════════════════════════════════════
// 统一对外类型
// ═══════════════════════════════════════════════════════════════

/// JS 引擎包装器
///
/// 根据编译 feature 选择后端：
/// - `boa` feature → Boa Engine（纯 Rust，默认）
/// - `js` feature → obscura-js（V8/deno_core）
///
/// # 示例
///
/// ```ignore
/// let mut engine = JsEngine::new();
/// engine.initialize("https://example.com").unwrap();
/// let result = engine.evaluate("1 + 2").unwrap();
/// assert_eq!(result, "3");
/// ```
pub struct JsEngine {
    #[cfg(feature = "boa")]
    inner: backend::BoaJsEngine,
    #[cfg(feature = "js")]
    inner: ObscuraJsEngine,
    #[cfg(not(any(feature = "boa", feature = "js")))]
    _dummy: (),
}

#[cfg(feature = "js")]
mod js_backend {
    use log::{info, warn};

    pub struct ObscuraJsEngine {
        runtime: Option<obscura_js::runtime::ObscuraJsRuntime>,
    }

    impl ObscuraJsEngine {
        pub fn new() -> Self {
            info!("初始化 JS 引擎 (obscura-js + deno_core)");
            Self { runtime: None }
        }

        pub fn initialize(&mut self, url: &str) -> Result<(), String> {
            if self.runtime.is_some() {
                return Ok(());
            }
            info!("启动 V8 运行时, URL: {}", url);
            let rt = obscura_js::runtime::ObscuraJsRuntime::with_base_url(url);
            self.runtime = Some(rt);
            info!("JS 引擎已就绪 (V8)");
            Ok(())
        }

        pub fn evaluate(&mut self, code: &str) -> Result<String, String> {
            match self.runtime.as_mut() {
                Some(rt) => {
                    let result = rt.evaluate(code).map_err(|e| format!("{:?}", e))?;
                    Ok(result.to_string())
                }
                None => Err("JS 引擎未初始化".to_string()),
            }
        }

        pub fn is_ready(&self) -> bool {
            self.runtime.is_some()
        }

        pub fn set_url(&self, url: &str) {
            if let Some(ref rt) = self.runtime {
                rt.set_url(url);
            }
        }

        pub fn dispatch_event(&mut self, node_id: u32, event_type: &str) -> Result<String, String> {
            let js = format!(
                "(() => {{ try {{ document.getElementById('{}')?.dispatchEvent(new Event('{}')); return 'ok'; }} catch(e) {{ return `err:${{e.message}}`; }} }})()",
                node_id, event_type
            );
            self.evaluate(&js)
        }

        pub fn set_dom(&self, dom: obscura_dom::tree::DomTree) {
            if let Some(ref rt) = self.runtime {
                rt.set_dom(dom);
            }
        }
    }
}

#[cfg(feature = "js")]
use js_backend::ObscuraJsEngine;

impl JsEngine {
    /// 创建新的 JS 引擎
    pub fn new() -> Self {
        #[cfg(feature = "boa")]
        {
            info!("初始化 JS 引擎 (Boa 后端)");
            Self {
                inner: backend::BoaJsEngine::new(),
            }
        }
        #[cfg(feature = "js")]
        {
            info!("初始化 JS 引擎 (obscura-js 后端)");
            Self {
                inner: ObscuraJsEngine::new(),
            }
        }
        #[cfg(not(any(feature = "boa", feature = "js")))]
        {
            info!("JS 引擎: 未启用 (启用 boa 或 js feature)");
            Self { _dummy: () }
        }
    }

    /// 初始化运行时
    pub fn initialize(&mut self, url: &str) -> Result<(), String> {
        #[cfg(feature = "boa")]
        {
            self.inner.initialize(url)
        }
        #[cfg(feature = "js")]
        {
            self.inner.initialize(url)
        }
        #[cfg(not(any(feature = "boa", feature = "js")))]
        {
            let _ = url;
            Ok(())
        }
    }

    /// 执行 JavaScript 代码
    pub fn evaluate(&mut self, code: &str) -> Result<String, String> {
        #[cfg(feature = "boa")]
        {
            self.inner.evaluate(code)
        }
        #[cfg(feature = "js")]
        {
            self.inner.evaluate(code)
        }
        #[cfg(not(any(feature = "boa", feature = "js")))]
        {
            let _ = code;
            Err("JS 引擎未启用".to_string())
        }
    }

    /// 引擎是否就绪
    pub fn is_ready(&self) -> bool {
        #[cfg(feature = "boa")]
        {
            self.inner.is_ready()
        }
        #[cfg(feature = "js")]
        {
            self.inner.is_ready()
        }
        #[cfg(not(any(feature = "boa", feature = "js")))]
        {
            false
        }
    }

    /// 设置当前 URL
    pub fn set_url(&mut self, url: &str) {
        #[cfg(feature = "boa")]
        {
            self.inner.set_url(url);
        }
        #[cfg(feature = "js")]
        {
            self.inner.set_url(url);
        }
        #[cfg(not(any(feature = "boa", feature = "js")))]
        {
            let _ = url;
        }
    }

    /// 触发 JS 事件
    pub fn dispatch_event(&mut self, node_id: u32, event_type: &str) -> Result<String, String> {
        #[cfg(feature = "boa")]
        {
            self.inner.dispatch_event(node_id, event_type)
        }
        #[cfg(feature = "js")]
        {
            self.inner.dispatch_event(node_id, event_type)
        }
        #[cfg(not(any(feature = "boa", feature = "js")))]
        {
            let _ = (node_id, event_type);
            Err("JS 引擎未启用".to_string())
        }
    }

    /// 设置 DOM 树（仅 obscura-js 后端支持）
    #[cfg(feature = "js")]
    pub fn set_dom(&self, dom: obscura_dom::tree::DomTree) {
        self.inner.set_dom(dom);
    }
}

impl Default for JsEngine {
    fn default() -> Self {
        Self::new()
    }
}
