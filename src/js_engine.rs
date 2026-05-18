//! JS 引擎 —— 多后端支持（deno_core(V8) / Boa）
//!
//! 通过 feature 切换后端：
//! - `boa`（默认）：基于 Boa Engine（纯 Rust，无 V8 依赖）
//! - `v8`：基于 deno_core（V8）
//!
//! # 公开 API
//!
//! 无论选择哪个后端，外部代码都使用统一的 `JsEngine` 类型，
//! 通过 `new()` / `initialize()` / `evaluate()` / `is_ready()` 等
//! 方法操作，不依赖后端细节。

use log::info;

// ═══════════════════════════════════════════════════════════════
// Boa 后端（默认，纯 Rust）
// ═══════════════════════════════════════════════════════════════

#[cfg(feature = "boa")]
mod backend {
    use std::cell::RefCell;
    use std::rc::Rc;

    use boa_engine::native_function::NativeFunction;
    use boa_engine::property::Attribute;
    use boa_engine::JsString;
    use boa_engine::{Context, JsArgs, JsError, JsResult, JsValue, Source};
    use log::{info, warn};

    use crate::network::NetworkClient;

    pub struct BoaJsEngine {
        context: Option<Context>,
        url: String,
        network: Rc<RefCell<NetworkClient>>,
    }

    impl BoaJsEngine {
        pub fn new() -> Self {
            info!("初始化 JS 引擎 (Boa Engine)");
            Self {
                context: None,
                url: "about:blank".to_string(),
                network: Rc::new(RefCell::new(NetworkClient::new())),
            }
        }

        pub fn initialize(&mut self, url: &str) -> Result<(), String> {
            if self.context.is_some() {
                return Ok(());
            }
            info!("启动 Boa 运行时, URL: {}", url);
            self.url = url.to_string();

            let mut context = Context::default();

            // 注入基础的 console 和 polyfill 对象
            let polyfill_js = r#"
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
                    title: '', URL: '',
                    createElement: (tag) => ({ tagName: tag, style: {}, setAttribute: () => {}, getAttribute: () => null, appendChild: () => {}, textContent: '' }),
                    createTextNode: (text) => ({ nodeType: 3, textContent: text, data: text }),
                    getElementById: () => null, querySelector: () => null, querySelectorAll: () => [],
                    body: { appendChild: () => {}, style: {} },
                    head: { appendChild: () => {} },
                    documentElement: { style: {} },
                    addEventListener: () => {}, removeEventListener: () => {}, dispatchEvent: () => true,
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
                globalThis.JSON = JSON; globalThis.Math = Math;
                globalThis.parseInt = parseInt; globalThis.parseFloat = parseFloat;
                globalThis.isNaN = isNaN; globalThis.isFinite = isFinite;
                globalThis.encodeURI = encodeURI; globalThis.decodeURI = decodeURI;
                globalThis.encodeURIComponent = encodeURIComponent; globalThis.decodeURIComponent = decodeURIComponent;
                globalThis.Array = Array; globalThis.Object = Object; globalThis.String = String;
                globalThis.Number = Number; globalThis.Boolean = Boolean; globalThis.Function = Function;
                globalThis.Date = Date; globalThis.RegExp = RegExp;
                globalThis.Error = Error; globalThis.TypeError = TypeError; globalThis.ReferenceError = ReferenceError;
                globalThis.SyntaxError = SyntaxError;
                globalThis.Promise = Promise; globalThis.Map = Map; globalThis.Set = Set; globalThis.Symbol = Symbol;
            "#;

            match context.eval(Source::from_bytes(&polyfill_js)) {
                Ok(_) => info!("Boa 运行时 polyfill 注入完成"),
                Err(e) => warn!("Boa polyfill 注入警告: {}", e),
            }

            // 注册 fetch 原生函数
            self.register_fetch(&mut context);

            self.context = Some(context);
            info!("JS 引擎已就绪 (Boa)");
            Ok(())
        }

        /// 注册全局 fetch() 原生函数
        fn register_fetch(&self, context: &mut Context) {
            // 使用 from_fn_ptr 创建 fetch 函数
            // 函数内部用 tokio block_on 执行 HTTP 请求，然后返回 Promise
            fn fetch_impl(
                _this: &JsValue,
                args: &[JsValue],
                context: &mut Context,
            ) -> JsResult<JsValue> {
                let url_val = args.get_or_undefined(0).clone();
                let opts_val = args.get_or_undefined(1).clone();

                // 获取 URL 字符串
                let url_str = url_val
                    .to_string(context)
                    .map_err(|e| JsError::from_opaque(JsString::from(e.to_string()).into()))?
                    .to_std_string()
                    .map_err(|e| JsError::from_opaque(JsString::from(e.to_string()).into()))?;

                // 获取 method
                let _method = if opts_val.is_undefined() {
                    "GET".to_string()
                } else {
                    let obj = opts_val.as_object().ok_or_else(|| {
                        JsError::from_opaque(
                            JsString::from("fetch: options must be an object").into(),
                        )
                    })?;
                    let method_val = obj
                        .get(JsString::from("method"), context)
                        .map_err(|e| JsError::from_opaque(JsString::from(e.to_string()).into()))?;
                    if method_val.is_undefined() {
                        "GET".to_string()
                    } else {
                        method_val
                            .to_string(context)
                            .map_err(|e| {
                                JsError::from_opaque(JsString::from(e.to_string()).into())
                            })?
                            .to_std_string()
                            .map_err(|e| {
                                JsError::from_opaque(JsString::from(e.to_string()).into())
                            })?
                            .to_uppercase()
                    }
                };

                // 使用 tokio runtime 执行异步 HTTP 请求（同步阻塞）
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .map_err(|e| JsError::from_opaque(JsString::from(e.to_string()).into()))?;

                let (status, status_text, headers_vec, body_text) = rt
                    .block_on(async {
                        let client = reqwest::Client::builder()
                            .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36")
                            .timeout(std::time::Duration::from_secs(15))
                            .danger_accept_invalid_certs(false)
                            .gzip(true)
                            .brotli(true)
                            .build()
                            .map_err(|e| JsError::from_opaque(JsString::from(e.to_string()).into()))?;

                        let resp = client.get(&url_str).send().await
                            .map_err(|e| JsError::from_opaque(JsString::from(e.to_string()).into()))?;

                        let status = resp.status().as_u16();
                        let status_text = resp.status().canonical_reason().unwrap_or("").to_string();
                        let headers_vec: Vec<(String, String)> = resp
                            .headers()
                            .iter()
                            .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or("").to_string()))
                            .collect();

                        let body_bytes = resp.bytes().await
                            .map_err(|e| JsError::from_opaque(JsString::from(e.to_string()).into()))?;
                        let body_text = String::from_utf8_lossy(&body_bytes).to_string();

                        Ok::<_, JsError>((status, status_text, headers_vec, body_text))
                    })?;

                // 构造 Response JS 对象
                let body_json = serde_json::to_string(&body_text).unwrap_or_else(|_| "\"\"".into());
                let headers_map: std::collections::HashMap<String, String> =
                    headers_vec.into_iter().collect();
                let headers_json =
                    serde_json::to_string(&headers_map).unwrap_or_else(|_| "{}".into());
                let ok_str = if (200..300).contains(&status) {
                    "true"
                } else {
                    "false"
                };
                let status_text_escaped = status_text.replace('"', "\\\"");
                let url_safe = url_str.replace('"', "\\\"").replace('\n', "");

                // 创建 Promise：立即 resolve 为构造好的 Response 对象
                let response_js = format!(
                    "new Promise(resolve => resolve((function() {{
                        const body = {body_json};
                        const headers = {headers_json};
                        const hdr = {{
                            get: (name) => headers[name.toLowerCase()] || null,
                            has: (name) => name.toLowerCase() in headers,
                            forEach: (cb) => Object.entries(headers).forEach(([k,v]) => cb(v,k)),
                        }};
                        return {{
                            ok: {ok},
                            status: {status},
                            statusText: \"{status_text}\",
                            headers: hdr,
                            url: \"{url_safe}\",
                            type: \"basic\",
                            redirected: false,
                            body: null,
                            bodyUsed: false,
                            text: () => Promise.resolve(body),
                            json: () => Promise.resolve(JSON.parse(body)),
                            blob: () => Promise.resolve(new Blob([body])),
                            arrayBuffer: () => Promise.resolve(new TextEncoder().encode(body).buffer),
                            clone: function() {{ return Object.assign({{}}, this); }},
                        }};
                    }})()))",
                    body_json = body_json,
                    headers_json = headers_json,
                    ok = ok_str,
                    status = status,
                    status_text = status_text_escaped,
                    url_safe = url_safe,
                );

                let result = context
                    .eval(Source::from_bytes(response_js.as_bytes()))
                    .map_err(|e| {
                        JsError::from_opaque(
                            JsString::from(format!("fetch response eval error: {}", e)).into(),
                        )
                    })?;

                Ok(result)
            }

            let fetch_fn = NativeFunction::from_fn_ptr(fetch_impl);
            let fetch_func = fetch_fn.to_js_function(context.realm());
            let _ = context.register_global_property(
                JsString::from("fetch"),
                fetch_func,
                Attribute::WRITABLE | Attribute::CONFIGURABLE,
            );

            info!("fetch() 函数已注册，返回 Promise");
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

        #[allow(unused_variables)]
        pub fn dispatch_event(
            &mut self,
            _node_id: u32,
            _event_type: &str,
        ) -> Result<String, String> {
            Ok("ok".to_string())
        }
    }
}

// ═══════════════════════════════════════════════════════════════
// 统一对外类型
// ═══════════════════════════════════════════════════════════════

pub struct JsEngine {
    #[cfg(feature = "boa")]
    inner: backend::BoaJsEngine,
    #[cfg(feature = "v8")]
    inner: ObscuraJsEngine,
    #[cfg(not(any(feature = "boa", feature = "v8")))]
    _dummy: (),
}

#[cfg(feature = "v8")]
mod js_backend {
    use deno_core::{JsRuntime, RuntimeOptions};
    use log::info;

    pub struct ObscuraJsEngine {
        runtime: Option<JsRuntime>,
    }

    impl ObscuraJsEngine {
        pub fn new() -> Self {
            info!("初始化 JS 引擎 (deno_core / V8)");
            Self { runtime: None }
        }

        pub fn initialize(&mut self, url: &str) -> Result<(), String> {
            if self.runtime.is_some() {
                return Ok(());
            }
            info!("启动 V8 运行时, URL: {}", url);

            let mut runtime = JsRuntime::new(RuntimeOptions {
                ..Default::default()
            });

            // 注入 polyfill
            let polyfill_js = r#"
                if (typeof globalThis.console === 'undefined') {
                    globalThis.console = {
                        log: (...args) => {},
                        warn: (...args) => {},
                        error: (...args) => {},
                        info: (...args) => {},
                        debug: () => {},
                        trace: () => {},
                    };
                }
                if (typeof globalThis.setTimeout === 'undefined') {
                    globalThis.setTimeout = (fn, ms) => { if (typeof fn === 'function') fn(); };
                    globalThis.setInterval = (fn, ms) => { if (typeof fn === 'function') fn(); };
                    globalThis.clearTimeout = () => {};
                    globalThis.clearInterval = () => {};
                    globalThis.requestAnimationFrame = (fn) => { if (typeof fn === 'function') fn(0); };
                    globalThis.cancelAnimationFrame = () => {};
                    globalThis.queueMicrotask = (fn) => { if (typeof fn === 'function') Promise.resolve().then(fn); };
                }
                if (typeof globalThis.document === 'undefined') {
                    globalThis.document = {
                        title: '', URL: '',
                        createElement: (tag) => ({ tagName: tag, style: {}, setAttribute: () => {}, getAttribute: () => null, appendChild: () => {}, textContent: '' }),
                        createTextNode: (text) => ({ nodeType: 3, textContent: text, data: text }),
                        getElementById: () => null, querySelector: () => null, querySelectorAll: () => [],
                        body: { appendChild: () => {}, style: {} },
                        head: { appendChild: () => {} },
                        documentElement: { style: {} },
                        addEventListener: () => {}, removeEventListener: () => {}, dispatchEvent: () => true,
                    };
                }
                globalThis.window = globalThis;
                globalThis.self = globalThis;
                if (typeof globalThis.location === 'undefined') {
                    globalThis.location = { href: '', protocol: 'https:', host: '', hostname: '', pathname: '/', search: '', hash: '', origin: '' };
                }
                if (typeof globalThis.navigator === 'undefined') {
                    globalThis.navigator = { userAgent: 'RustBrowser/1.0', platform: 'Win32', language: 'zh-CN' };
                }
                globalThis.HTMLElement = function() {};
                if (typeof globalThis.Event === 'undefined') {
                    globalThis.Event = class Event { constructor(type) { this.type = type; this.defaultPrevented = false; } preventDefault() { this.defaultPrevented = true; } };
                    globalThis.CustomEvent = class CustomEvent extends Event { constructor(type, detail) { super(type); this.detail = detail?.detail; } };
                    globalThis.MouseEvent = class MouseEvent extends Event { constructor(type, init) { super(type); this.clientX = init?.clientX || 0; this.clientY = init?.clientY || 0; } };
                    globalThis.KeyboardEvent = class KeyboardEvent extends Event { constructor(type, init) { super(type); this.key = init?.key || ''; this.code = init?.code || ''; } };
                }
            "#;

            runtime
                .execute_script("<polyfill>", polyfill_js.to_string())
                .map_err(|e| format!("Polyfill 注入失败: {}", e))?;

            info!("V8 运行时 polyfill 注入完成");
            self.runtime = Some(runtime);
            info!("JS 引擎已就绪 (V8 / deno_core)");
            Ok(())
        }

        pub fn evaluate(&mut self, code: &str) -> Result<String, String> {
            match self.runtime.as_mut() {
                Some(rt) => {
                    let result = rt
                        .execute_script("<eval>", code.to_string())
                        .map_err(|e| format!("JS error: {}", e))?;
                    let scope = &mut rt.handle_scope();
                    let local = deno_core::v8::Local::new(scope, result);
                    if local.is_string() {
                        let s = local.to_string(scope).unwrap();
                        let rust_str = s.to_rust_string_lossy(scope);
                        Ok(rust_str)
                    } else if local.is_number() {
                        let num = local.number_value(scope).unwrap_or(0.0);
                        Ok(num.to_string())
                    } else if local.is_boolean() {
                        let b = local.boolean_value(scope);
                        Ok(b.to_string())
                    } else if local.is_undefined() || local.is_null() {
                        Ok("undefined".to_string())
                    } else if local.is_object() {
                        let json_str = deno_core::v8::json::stringify(scope, local)
                            .map(|s| s.to_rust_string_lossy(scope))
                            .unwrap_or_else(|| {
                                local
                                    .to_string(scope)
                                    .map(|s| s.to_rust_string_lossy(scope))
                                    .unwrap_or_default()
                            });
                        Ok(json_str)
                    } else {
                        Ok(local
                            .to_string(scope)
                            .map(|s| s.to_rust_string_lossy(scope))
                            .unwrap_or_default())
                    }
                }
                None => Err("JS 引擎未初始化".to_string()),
            }
        }

        pub fn is_ready(&self) -> bool {
            self.runtime.is_some()
        }

        pub fn set_url(&self, _url: &str) {
            // deno_core 原生 JsRuntime 不需要手动设置 URL
            // 当前 polyfill 中的 location 已经是只读的
        }

        pub fn dispatch_event(&mut self, node_id: u32, event_type: &str) -> Result<String, String> {
            let js = format!(
                "(() => {{ try {{ document.getElementById('{}')?.dispatchEvent(new Event('{}')); return 'ok'; }} catch(e) {{ return `err:${{e.message}}`; }} }})()",
                node_id, event_type
            );
            self.evaluate(&js)
        }
    }
}

#[cfg(feature = "v8")]
use js_backend::ObscuraJsEngine;

impl JsEngine {
    pub fn new() -> Self {
        #[cfg(feature = "boa")]
        {
            info!("初始化 JS 引擎 (Boa 后端)");
            return Self {
                inner: backend::BoaJsEngine::new(),
            };
        }
        #[cfg(feature = "v8")]
        {
            info!("初始化 JS 引擎 (deno_core 后端)");
            return Self {
                inner: ObscuraJsEngine::new(),
            };
        }
        #[cfg(not(any(feature = "boa", feature = "v8")))]
        {
            info!("JS 引擎: 未启用 (启用 boa 或 v8 feature)");
            return Self { _dummy: () };
        }
    }

    pub fn initialize(&mut self, url: &str) -> Result<(), String> {
        #[cfg(feature = "boa")]
        {
            return self.inner.initialize(url);
        }
        #[cfg(feature = "v8")]
        {
            return self.inner.initialize(url);
        }
        #[cfg(not(any(feature = "boa", feature = "v8")))]
        {
            let _ = url;
            return Ok(());
        }
    }

    pub fn evaluate(&mut self, code: &str) -> Result<String, String> {
        #[cfg(feature = "boa")]
        {
            return self.inner.evaluate(code);
        }
        #[cfg(feature = "v8")]
        {
            return self.inner.evaluate(code);
        }
        #[cfg(not(any(feature = "boa", feature = "v8")))]
        {
            let _ = code;
            return Err("JS 引擎未启用".to_string());
        }
    }

    pub fn is_ready(&self) -> bool {
        #[cfg(feature = "boa")]
        {
            return self.inner.is_ready();
        }
        #[cfg(feature = "v8")]
        {
            return self.inner.is_ready();
        }
        #[cfg(not(any(feature = "boa", feature = "v8")))]
        {
            return false;
        }
    }

    pub fn set_url(&mut self, url: &str) {
        #[cfg(feature = "boa")]
        {
            self.inner.set_url(url);
        }
        #[cfg(feature = "v8")]
        {
            self.inner.set_url(url);
        }
        #[cfg(not(any(feature = "boa", feature = "v8")))]
        {
            let _ = url;
        }
    }

    pub fn dispatch_event(&mut self, node_id: u32, event_type: &str) -> Result<String, String> {
        #[cfg(feature = "boa")]
        {
            return self.inner.dispatch_event(node_id, event_type);
        }
        #[cfg(feature = "v8")]
        {
            return self.inner.dispatch_event(node_id, event_type);
        }
        #[cfg(not(any(feature = "boa", feature = "v8")))]
        {
            let _ = (node_id, event_type);
            return Err("JS 引擎未启用".to_string());
        }
    }
}

impl Default for JsEngine {
    fn default() -> Self {
        Self::new()
    }
}
