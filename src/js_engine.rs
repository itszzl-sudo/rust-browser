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
use std::sync::Mutex;

/// 全局 JS 控制台日志缓冲区
/// 由 nativeConsoleLog 原生函数写入，由 GUI 线程定期读取
pub static CONSOLE_LOG_BUFFER: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// 将 JS 控制台消息添加到全局日志缓冲区
pub fn push_console_log(level: &str, message: &str) {
    let msg = format!("[console.{}] {}", level, message);
    if let Ok(mut buf) = CONSOLE_LOG_BUFFER.lock() {
        buf.push(msg);
        if buf.len() > 1000 {
            buf.drain(0..500);
        }
    }
    // 同时通过 log crate 输出
    match level {
        "error" => log::error!("[JS] {}", message),
        "warn" => log::warn!("[JS] {}", message),
        "info" => log::info!("[JS] {}", message),
        "debug" => log::debug!("[JS] {}", message),
        "trace" => log::trace!("[JS] {}", message),
        _ => log::info!("[JS] {}", message),
    }
}

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
        #[allow(dead_code)]
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
            // console 方法通过 nativeConsoleLog 原生函数转发到 Rust 日志系统
            let polyfill_js = r#"
                globalThis.console = {
                    log: (...args) => { nativeConsoleLog('log', args.map(a => String(a)).join(' ')); },
                    warn: (...args) => { nativeConsoleLog('warn', args.map(a => String(a)).join(' ')); },
                    error: (...args) => { nativeConsoleLog('error', args.map(a => String(a)).join(' ')); },
                    info: (...args) => { nativeConsoleLog('info', args.map(a => String(a)).join(' ')); },
                    debug: (...args) => { nativeConsoleLog('debug', args.map(a => String(a)).join(' ')); },
                    trace: (...args) => { nativeConsoleLog('trace', args.map(a => String(a)).join(' ')); },
                };
                globalThis.setTimeout = (fn, ms) => { if (typeof fn === 'function') fn(); };
                globalThis.setInterval = (fn, ms) => { if (typeof fn === 'function') fn(); };
                globalThis.clearTimeout = () => {};
                globalThis.clearInterval = () => {};
                globalThis.requestAnimationFrame = (fn) => { if (typeof fn === 'function') fn(0); };
                globalThis.cancelAnimationFrame = () => {};
                globalThis.queueMicrotask = (fn) => { if (typeof fn === 'function') Promise.resolve().then(fn); };

                // 简化的 document 对象（createElement 支持 'canvas'、'select'、'option' 标签）
                globalThis.document = {
                    title: '', URL: '',
                    createElement: (tag) => {
                        var el = { tagName: tag, style: {}, setAttribute: function(k,v) { this[k] = v; }, getAttribute: function(k) { return this[k] || null; }, appendChild: function(c) { return c; }, textContent: '', children: [], addEventListener: function() {}, removeEventListener: function() {} };
                        if (tag.toLowerCase() === 'select') {
                            el._options = [];
                            el._selectedIndex = -1;
                            el.options = [];
                            el.value = '';
                            el.selectedIndex = -1;
                            el.add = function(opt) {
                                el._options.push(opt);
                                el.options.push(opt);
                                opt._parentSelect = el;
                                if (el._options.length === 1) {
                                    el.selectedIndex = 0;
                                    el.value = opt.value || opt.textContent;
                                }
                            };
                            el.appendChild = function(child) {
                                el.children.push(child);
                                if (child.tagName && child.tagName.toLowerCase() === 'option') {
                                    el.add(child);
                                }
                                return child;
                            };
                        }
                        if (tag.toLowerCase() === 'option') {
                            el.selected = false;
                            el.value = '';
                            el._parentSelect = null;
                            el.setAttribute = function(k,v) { this[k] = v; if (k === 'selected') { this.selected = true; if (this._parentSelect) { this._parentSelect.value = this.value || this.textContent; this._parentSelect.selectedIndex = Array.from(this._parentSelect.options).indexOf(this); } } };
                            el.addEventListener = function() {};
                            el.removeEventListener = function() {};
                        }
                        if (tag.toLowerCase() === 'canvas') {
                            el.width = 300;
                            el.height = 150;
                            el._canvasData = null;  // lazy init
                            el.getContext = function(type) {
                                if (type !== '2d') return null;
                                if (!el._canvasCtx) {
                                    el._canvasCtx = new CanvasRenderingContext2D(el);
                                }
                                return el._canvasCtx;
                            };
                            el.toDataURL = function() { return 'data:image/png;base64,'; };
                        }
                        return el;
                    },
                    createTextNode: (text) => ({ nodeType: 3, textContent: text, data: text }),
                    getElementById: () => null, querySelector: () => null, querySelectorAll: () => [],
                    body: { appendChild: function(c) { return c; }, style: {} },
                    head: { appendChild: function(c) { return c; } },
                    documentElement: { style: {} },
                    addEventListener: () => {}, removeEventListener: () => {}, dispatchEvent: () => true,
                };

                // —— Canvas 2D 上下文实现 ——
                // 维护像素缓冲区，支持基本的 2D 绘图 API
                globalThis.CanvasRenderingContext2D = function(canvas) {
                    this._canvas = canvas;
                    this._w = canvas.width || 300;
                    this._h = canvas.height || 150;
                    // 初始化像素缓冲区（RGBA），默认全白透明
                    var size = this._w * this._h * 4;
                    this._pixels = new Uint8Array(size);
                    for (var i = 0; i < size; i += 4) {
                        this._pixels[i] = 0;     // R
                        this._pixels[i+1] = 0;   // G
                        this._pixels[i+2] = 0;   // B
                        this._pixels[i+3] = 0;   // A (transparent)
                    }
                    // 绘制状态
                    this.fillStyle = '#000000';
                    this.strokeStyle = '#000000';
                    this.lineWidth = 1;
                    this.font = '10px sans-serif';
                    this.textAlign = 'start';
                    this.textBaseline = 'alphabetic';
                    this.globalAlpha = 1.0;
                };

                CanvasRenderingContext2D.prototype = {
                    // 辅助：解析 #rrggbb 颜色为 [r,g,b]
                    _parseColor: function(color) {
                        if (typeof color !== 'string') return [0,0,0];
                        var c = color.trim();
                        if (c.startsWith('#')) {
                            var hex = c.slice(1);
                            if (hex.length === 3) {
                                hex = hex[0]+hex[0]+hex[1]+hex[1]+hex[2]+hex[2];
                            }
                            if (hex.length === 6) {
                                return [parseInt(hex.substr(0,2),16), parseInt(hex.substr(2,2),16), parseInt(hex.substr(4,2),16)];
                            }
                        }
                        // 常见命名颜色简写
                        var named = { red:[255,0,0], green:[0,128,0], blue:[0,0,255], white:[255,255,255],
                            black:[0,0,0], gray:[128,128,128], grey:[128,128,128], yellow:[255,255,0],
                            orange:[255,165,0], purple:[128,0,128], pink:[255,192,203], cyan:[0,255,255],
                            magenta:[255,0,255], transparent:[0,0,0], silver:[192,192,192] };
                        if (named[c.toLowerCase()]) return named[c.toLowerCase()];
                        return [0,0,0];
                    },
                    _setPixel: function(x, y, r, g, b, a) {
                        var idx = (Math.floor(y) * this._w + Math.floor(x)) * 4;
                        if (idx < 0 || idx + 3 >= this._pixels.length) return;
                        this._pixels[idx] = r;
                        this._pixels[idx+1] = g;
                        this._pixels[idx+2] = b;
                        this._pixels[idx+3] = a;
                    },
                    _fillRectPixels: function(x, y, w, h, r, g, b, a) {
                        var ix = Math.max(0, Math.floor(x));
                        var iy = Math.max(0, Math.floor(y));
                        var iw = Math.min(Math.floor(x + w) - ix, this._w - ix);
                        var ih = Math.min(Math.floor(y + h) - iy, this._h - iy);
                        for (var py = iy; py < iy + ih; py++) {
                            for (var px = ix; px < ix + iw; px++) {
                                this._setPixel(px, py, r, g, b, a);
                            }
                        }
                    },
                    clearRect: function(x, y, w, h) {
                        this._fillRectPixels(x, y, w, h, 0, 0, 0, 0);
                    },
                    fillRect: function(x, y, w, h) {
                        var c = this._parseColor(this.fillStyle);
                        var a = Math.round(this.globalAlpha * 255);
                        this._fillRectPixels(x, y, w, h, c[0], c[1], c[2], a);
                    },
                    strokeRect: function(x, y, w, h) {
                        var c = this._parseColor(this.strokeStyle);
                        var a = Math.round(this.globalAlpha * 255);
                        var lw = Math.max(1, Math.round(this.lineWidth));
                        // 上边
                        this._fillRectPixels(x, y, w, lw, c[0], c[1], c[2], a);
                        // 下边
                        this._fillRectPixels(x, y + h - lw, w, lw, c[0], c[1], c[2], a);
                        // 左边
                        this._fillRectPixels(x, y + lw, lw, h - lw*2, c[0], c[1], c[2], a);
                        // 右边
                        this._fillRectPixels(x + w - lw, y + lw, lw, h - lw*2, c[0], c[1], c[2], a);
                    },
                    // fillText 简版 - 实际无法在 JS 中做像素字体渲染，静默略过
                    fillText: function(text, x, y, maxWidth) { /* 像素字体需要原生支持，此处为空操作 */ },
                    strokeText: function(text, x, y, maxWidth) { /* 同上 */ },
                    measureText: function(text) {
                        return { width: text.length * 6 };  // 粗略估计
                    },
                    beginPath: function() { this._path = []; },
                    moveTo: function(x, y) { if (!this._path) this._path = []; this._path.push({type:'move', x:x, y:y}); },
                    lineTo: function(x, y) { if (!this._path) this._path = []; this._path.push({type:'line', x:x, y:y}); },
                    closePath: function() { if (this._path && this._path.length > 1) this._path.push({type:'close'}); },
                    stroke: function() {
                        // 简版：用 Bresenham 画线
                        if (!this._path || this._path.length < 2) return;
                        var c = this._parseColor(this.strokeStyle);
                        var a = Math.round(this.globalAlpha * 255);
                        var cx = 0, cy = 0;
                        for (var i = 0; i < this._path.length; i++) {
                            var p = this._path[i];
                            if (p.type === 'move') { cx = p.x; cy = p.y; }
                            else if (p.type === 'line') {
                                this._drawLine(cx, cy, p.x, p.y, c[0], c[1], c[2], a);
                                cx = p.x; cy = p.y;
                            }
                        }
                    },
                    fill: function() {
                        // 填充路径封闭区域（简版：仅填充三角形/矩形）
                        if (!this._path || this._path.length < 2) return;
                        var c = this._parseColor(this.fillStyle);
                        var a = Math.round(this.globalAlpha * 255);
                        var pts = this._path.filter(function(p) { return p.type !== 'close'; });
                        if (pts.length >= 3) {
                            // 扫描线填充多边形（简化版）
                            var minX = this._w, maxX = 0, minY = this._h, maxY = 0;
                            for (var i = 0; i < pts.length; i++) {
                                if (pts[i].x < minX) minX = pts[i].x;
                                if (pts[i].x > maxX) maxX = pts[i].x;
                                if (pts[i].y < minY) minY = pts[i].y;
                                if (pts[i].y > maxY) maxY = pts[i].y;
                            }
                            minX = Math.max(0, Math.floor(minX));
                            maxX = Math.min(this._w-1, Math.ceil(maxX));
                            minY = Math.max(0, Math.floor(minY));
                            maxY = Math.min(this._h-1, Math.ceil(maxY));
                            for (var py = minY; py <= maxY; py++) {
                                var inside = false;
                                var prev = pts[pts.length-1];
                                for (var j = 0; j < pts.length; j++) {
                                    var cur = pts[j];
                                    if ((cur.y > py) !== (prev.y > py) &&
                                        px < (prev.x - cur.x) * (py - cur.y) / (prev.y - cur.y) + cur.x) {
                                        inside = !inside;
                                    }
                                    prev = cur;
                                }
                                if (inside) {
                                    for (var px = minX; px <= maxX; px++) {
                                        this._setPixel(px, py, c[0], c[1], c[2], a);
                                    }
                                }
                            }
                        }
                    },
                    _drawLine: function(x0, y0, x1, y1, r, g, b, a) {
                        var dx = Math.abs(x1 - x0), dy = Math.abs(y1 - y0);
                        var sx = x0 < x1 ? 1 : -1, sy = y0 < y1 ? 1 : -1;
                        var err = dx - dy;
                        var cx = Math.round(x0), cy = Math.round(y0);
                        var ex = Math.round(x1), ey = Math.round(y1);
                        while (true) {
                            this._setPixel(cx, cy, r, g, b, a);
                            if (cx === ex && cy === ey) break;
                            var e2 = 2 * err;
                            if (e2 > -dy) { err -= dy; cx += sx; }
                            if (e2 < dx) { err += dx; cy += sy; }
                        }
                    },
                    // ImageData
                    createImageData: function(w, h) {
                        var data = new Uint8Array(w * h * 4);
                        return { width: w, height: h, data: data };
                    },
                    getImageData: function(x, y, w, h) {
                        var data = new Uint8Array(w * h * 4);
                        for (var py = 0; py < h; py++) {
                            for (var px = 0; px < w; px++) {
                                var srcIdx = ((Math.floor(y)+py) * this._w + (Math.floor(x)+px)) * 4;
                                var dstIdx = (py * w + px) * 4;
                                if (srcIdx >= 0 && srcIdx+3 < this._pixels.length) {
                                    data[dstIdx] = this._pixels[srcIdx];
                                    data[dstIdx+1] = this._pixels[srcIdx+1];
                                    data[dstIdx+2] = this._pixels[srcIdx+2];
                                    data[dstIdx+3] = this._pixels[srcIdx+3];
                                }
                            }
                        }
                        return { width: w, height: h, data: data };
                    },
                    putImageData: function(imgData, x, y) {
                        for (var py = 0; py < imgData.height; py++) {
                            for (var px = 0; px < imgData.width; px++) {
                                var srcIdx = (py * imgData.width + px) * 4;
                                if (srcIdx+3 >= imgData.data.length) continue;
                                this._setPixel(Math.floor(x)+px, Math.floor(y)+py,
                                    imgData.data[srcIdx], imgData.data[srcIdx+1],
                                    imgData.data[srcIdx+2], imgData.data[srcIdx+3]);
                            }
                        }
                    },
                    save: function() { /* 状态栈略过 */ },
                    restore: function() { /* 状态栈略过 */ },
                    scale: function(x, y) { /* 变换略过 */ },
                    rotate: function(angle) { /* 变换略过 */ },
                    translate: function(x, y) { /* 变换略过 */ },
                    setTransform: function(a,b,c,d,e,f) { /* 变换略过 */ },
                    // canvas 渲染到主渲染器：通过原生函数通知 Rust 端
                    _syncToNative: function() {
                        var pixels = this._pixels;
                        var w = this._w;
                        var h = this._h;
                        if (typeof nativeCanvasRender === 'function') {
                            nativeCanvasRender(w, h, Array.from(pixels));
                        }
                    },
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

                // —— XMLHttpRequest（XHR）实现 ——
                globalThis.XMLHttpRequest = function XMLHttpRequest() {
                    this.readyState = 0; // UNSENT
                    this.status = 0;
                    this.statusText = '';
                    this.responseText = '';
                    this.responseXML = null;
                    this.responseType = '';
                    this.response = null;
                    this.timeout = 0;
                    this.withCredentials = false;
                    this._url = '';
                    this._method = 'GET';
                    this._headers = {};
                    this._listeners = {};
                    this._aborted = false;
                };

                Object.assign(globalThis.XMLHttpRequest.prototype, {
                    UNSENT: 0,
                    OPENED: 1,
                    HEADERS_RECEIVED: 2,
                    LOADING: 3,
                    DONE: 4,

                    open: function(method, url, async) {
                        this._method = method.toUpperCase();
                        this._url = url;
                        this.readyState = 1; // OPENED
                        this._dispatchEvent('readystatechange');
                    },

                    setRequestHeader: function(name, value) {
                        this._headers[name] = value;
                    },

                    send: function(body) {
                        var self = this;
                        self.readyState = 2; // HEADERS_RECEIVED
                        self._dispatchEvent('readystatechange');
                        self.readyState = 3; // LOADING
                        self._dispatchEvent('readystatechange');

                        // 调用原生函数执行真正 HTTP 请求
                        var result = nativeXhrRequest(self._method, self._url,
                            JSON.stringify(self._headers), body || '', self.timeout);

                        if (self._aborted) return;

                        if (result) {
                            try {
                                var parsed = JSON.parse(result);
                                self.status = parsed.status;
                                self.statusText = parsed.statusText;
                                self.responseText = parsed.responseText;
                                self.response = self.responseText;
                            } catch(e) {
                                self.status = 0;
                                self.statusText = 'Error';
                            }
                        } else {
                            self.status = 0;
                            self.statusText = 'Network Error';
                        }

                        self.readyState = 4; // DONE
                        self._dispatchEvent('readystatechange');
                        self._dispatchEvent('load');
                    },

                    abort: function() {
                        this._aborted = true;
                        this.readyState = 0;
                        this._dispatchEvent('abort');
                    },

                    addEventListener: function(type, listener) {
                        if (!this._listeners[type]) this._listeners[type] = [];
                        this._listeners[type].push(listener);
                    },

                    removeEventListener: function(type, listener) {
                        if (!this._listeners[type]) return;
                        var idx = this._listeners[type].indexOf(listener);
                        if (idx >= 0) this._listeners[type].splice(idx, 1);
                    },

                    getResponseHeader: function(name) {
                        return null; // 简版
                    },

                    getAllResponseHeaders: function() {
                        return '';
                    },

                    _dispatchEvent: function(type) {
                        var evt = new Event(type);
                        evt.target = this;
                        if (typeof this['on' + type] === 'function') {
                            this['on' + type](evt);
                        }
                        if (this._listeners[type]) {
                            for (var i = 0; i < this._listeners[type].length; i++) {
                                this._listeners[type][i](evt);
                            }
                        }
                    }
                });
            "#;

            match context.eval(Source::from_bytes(&polyfill_js)) {
                Ok(_) => info!("Boa 运行时 polyfill 注入完成"),
                Err(e) => warn!("Boa polyfill 注入警告: {}", e),
            }

            // 注册 fetch 原生函数
            self.register_fetch(&mut context);

            // 注册 console 原生函数（将 JS 日志转发到 Rust）
            self.register_console(&mut context);

            // 注册 XHR 原生函数
            self.register_xhr(&mut context);

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

        /// 注册 console 原生函数 — 将 JS console 消息转发到 Rust 日志系统
        fn register_console(&self, context: &mut Context) {
            fn native_console_log_impl(
                _this: &JsValue,
                args: &[JsValue],
                context: &mut Context,
            ) -> JsResult<JsValue> {
                // 参数：level (string), message (string)
                let level = args
                    .get_or_undefined(0)
                    .to_string(context)
                    .ok()
                    .and_then(|s| s.to_std_string().ok())
                    .unwrap_or_else(|| "log".to_string());

                let message = args
                    .get_or_undefined(1)
                    .to_string(context)
                    .ok()
                    .and_then(|s| s.to_std_string().ok())
                    .unwrap_or_else(|| String::new());

                // 通过全局函数转发
                crate::js_engine::push_console_log(&level, &message);

                Ok(JsValue::undefined())
            }

            let console_fn = NativeFunction::from_fn_ptr(native_console_log_impl);
            let console_func = console_fn.to_js_function(context.realm());
            let _ = context.register_global_property(
                JsString::from("nativeConsoleLog"),
                console_func,
                Attribute::WRITABLE | Attribute::CONFIGURABLE,
            );

            info!("nativeConsoleLog() 函数已注册");
        }

        /// 注册 XMLHttpRequest 的 nativeXhrRequest 原生函数
        fn register_xhr(&self, context: &mut Context) {
            fn native_xhr_request_impl(
                _this: &JsValue,
                args: &[JsValue],
                context: &mut Context,
            ) -> JsResult<JsValue> {
                // 参数: (method, url, headers_json, body, timeout)
                let method = args
                    .get_or_undefined(0)
                    .to_string(context)
                    .map_err(|e| JsError::from_opaque(JsString::from(e.to_string()).into()))?
                    .to_std_string()
                    .unwrap_or_default();

                let url_str = args
                    .get_or_undefined(1)
                    .to_string(context)
                    .map_err(|e| JsError::from_opaque(JsString::from(e.to_string()).into()))?
                    .to_std_string()
                    .unwrap_or_default();

                let headers_json = args
                    .get_or_undefined(2)
                    .to_string(context)
                    .map_err(|e| JsError::from_opaque(JsString::from(e.to_string()).into()))?
                    .to_std_string()
                    .unwrap_or_else(|_| "{}".to_string());

                let body_str = args
                    .get_or_undefined(3)
                    .to_string(context)
                    .map_err(|e| JsError::from_opaque(JsString::from(e.to_string()).into()))?
                    .to_std_string()
                    .unwrap_or_default();

                let timeout_ms = args
                    .get_or_undefined(4)
                    .to_number(context)
                    .unwrap_or(0.0)
                    .max(0.0) as u64;

                if url_str.is_empty() {
                    return Ok(JsValue::null());
                }

                // 解析自定义请求头
                let custom_headers: std::collections::HashMap<String, String> =
                    serde_json::from_str(&headers_json).unwrap_or_default();

                // 使用 tokio runtime 执行 HTTP 请求
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .map_err(|e| JsError::from_opaque(JsString::from(e.to_string()).into()))?;

                let result = rt.block_on(async {
                    let client_builder = reqwest::Client::builder()
                        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36")
                        .danger_accept_invalid_certs(false);

                    let client = if timeout_ms > 0 {
                        client_builder
                            .timeout(std::time::Duration::from_millis(timeout_ms))
                            .build()
                    } else {
                        client_builder
                            .timeout(std::time::Duration::from_secs(15))
                            .build()
                    }
                    .map_err(|e| JsError::from_opaque(JsString::from(e.to_string()).into()))?;

                    let req = match method.as_str() {
                        "POST" => {
                            let mut r = client.post(&url_str);
                            for (k, v) in &custom_headers {
                                r = r.header(k.as_str(), v.as_str());
                            }
                            if !body_str.is_empty() {
                                r = r.body(body_str.clone());
                            }
                            r
                        }
                        "PUT" => {
                            let mut r = client.put(&url_str);
                            for (k, v) in &custom_headers {
                                r = r.header(k.as_str(), v.as_str());
                            }
                            if !body_str.is_empty() {
                                r = r.body(body_str.clone());
                            }
                            r
                        }
                        "DELETE" => {
                            let mut r = client.delete(&url_str);
                            for (k, v) in &custom_headers {
                                r = r.header(k.as_str(), v.as_str());
                            }
                            r
                        }
                        "HEAD" => {
                            let mut r = client.head(&url_str);
                            for (k, v) in &custom_headers {
                                r = r.header(k.as_str(), v.as_str());
                            }
                            r
                        }
                        _ => {
                            // GET 或其他
                            let mut r = client.get(&url_str);
                            for (k, v) in &custom_headers {
                                r = r.header(k.as_str(), v.as_str());
                            }
                            r
                        }
                    };

                    let resp = req.send().await.map_err(|e| {
                        JsError::from_opaque(JsString::from(format!("XHR network error: {}", e)).into())
                    })?;

                    let status = resp.status().as_u16();
                    let status_text = resp.status().canonical_reason().unwrap_or("").to_string();
                    let body_bytes = resp.bytes().await.map_err(|e| {
                        JsError::from_opaque(JsString::from(format!("XHR body error: {}", e)).into())
                    })?;
                    let body_text = String::from_utf8_lossy(&body_bytes).to_string();

                    // 返回 JSON 序列化的结果对象
                    let result_obj = serde_json::json!({
                        "status": status,
                        "statusText": status_text,
                        "responseText": body_text,
                    });

                    Ok::<_, JsError>(result_obj.to_string())
                });

                match result {
                    Ok(json_str) => Ok(JsValue::new(JsString::from(json_str))),
                    Err(e) => {
                        warn!("XHR request failed: {}", e);
                        Ok(JsValue::null())
                    }
                }
            }

            let native_fn = NativeFunction::from_fn_ptr(native_xhr_request_impl);
            let js_func = native_fn.to_js_function(context.realm());
            let _ = context.register_global_property(
                JsString::from("nativeXhrRequest"),
                js_func,
                Attribute::WRITABLE | Attribute::CONFIGURABLE,
            );

            info!("nativeXhrRequest 原生函数已注册");
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

        /// 定时器 tick：当前为简版，总是返回 0
        pub fn tick_timers(&mut self) -> usize {
            0
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

    /// 定时器 tick：每帧调用，返回触发的回调数量
    #[cfg(any(feature = "boa", feature = "v8"))]
    pub fn tick_timers(&mut self) -> usize {
        self.inner.tick_timers()
    }
}

impl Default for JsEngine {
    fn default() -> Self {
        Self::new()
    }
}
