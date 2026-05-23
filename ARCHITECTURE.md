# Rust Browser 架构文档

> 最后更新：2026-06-01

本文档面向项目参与者，记录架构决策、模块职责、数据流和关键技术细节。

---

## 一、整体架构

```
┌────────────────────────────────────────────────────────────────────┐
│                         Rust Browser                               │
│                                                                    │
│  ┌──────────────┐    ┌────────────────────┐    ┌───────────────┐  │
│  │  UI 层        │    │  浏览器核心         │    │  渲染引擎      │  │
│  │  (winit +     │    │  (BrowserProcess + │    │  (Renderer    │  │
│  │   eframe)     │◄──►│   RendererProcess) │◄──►│   Process)    │  │
│  │               │    │                    │    │               │  │
│  │  eframe 工具条│    │  TabManager        │    │  kuchiki DOM  │  │
│  │  winit 主窗口 │    │  NetworkClient     │    │  taffy 布局   │  │
│  │  softbuffer   │    │  JS 引擎           │    │  tiny-skia    │  │
│  └──────────────┘    └────────────────────┘    └───────────────┘  │
│                                                                    │
│  ┌──────────────────────────────────────────────────────────────┐  │
│  │  基础设施层                                                   │  │
│  │  Mojo IPC · TaskQueue · HTTP Cache · CSP · CORS · WebStorage │  │
│  └──────────────────────────────────────────────────────────────┘  │
└────────────────────────────────────────────────────────────────────┘
```

## 二、模块架构

```
src/
├── main.rs              # 入口：eframe 工具条 + winit 主窗口联动
├── lib.rs               # 模块注册和公共导出
│
├── gui_window.rs        # 跨平台窗口管理：winit 事件循环 + softbuffer 像素推送
│
├── browser/             # 浏览器核心
│   ├── mod.rs           # Browser 主类（标签页、导航、截图）
│   ├── engine.rs        # BrowserEngine（导航、JS、文档管理、favicon 提取）
│   ├── page.rs          # 页面管理
│   ├── tabs.rs          # 标签页管理（历史、前进/后退、favicon）
│   └── ui.rs            # Chrome 风格 UI 绘制（tiny-skia 渲染）
│
├── browser_process/     # 多进程 IPC
│   ├── host.rs          # BrowserProcessHost + run_renderer_process 消息循环
│   └── interfaces.rs    # Mojo IPC 接口（Navigation/InputEvent/RenderResult）
│
├── renderer/            # 渲染引擎
│   ├── renderer.rs      # 主渲染器（TaffyRenderer: DOM→布局→绘制）
│   ├── taffy_layout.rs  # taffy 布局引擎（Block/Flex/Grid + float + z-index）
│   ├── painter.rs       # tiny-skia 2D 绘制原语
│   ├── text.rs          # cosmic-text 文本排版
│   ├── border.rs        # border/box-shadow 绘制
│   ├── async_pipeline.rs# 异步渲染管线（首屏阻塞 + 后台 CSS 解析）
│   ├── layout_cache.rs  # 布局缓存（指纹哈希 + 增量复用）
│   └── image_pipeline.rs# 图片管道（元数据预读 + 占位布局）
│
├── css_engine/          # CSS 引擎
│   ├── mod.rs           # CSS 规则解析 + 媒体查询 + 样式映射
│   └── selector.rs      # selectors crate 适配（完整 CSS 选择器）
│
├── js_engine.rs         # JS 引擎双后端（Boa 纯 Rust / V8 deno_core）
├── js_dom_bridge.rs     # JS DOM 双向绑定（33 个原生函数 + 脏节点追踪）
├── js_timer_queue.rs    # 真实异步定时器（setTimeout/setInterval/requestAnimationFrame）
├── js_task_scheduler.rs # JS 任务分级调度（交互/布局/渲染辅助/网络/后台）
│
├── bridge.rs            # WebNativeBridge trait（纯接口定义，无依赖）
├── bridge_impl.rs       # Bridge 默认实现（串联 DOM/CSS/JS/布局/渲染/事件）
├── dom_wrapper.rs       # kuchiki DOM 包装器（ID 索引 + CSS 选择器查询）
│
├── network.rs           # HTTP 客户端（reqwest + Cookie RFC 6265）
├── http_cache.rs        # HTTP 缓存（Cache-Control/ETag/304 条件请求）
├── csp.rs               # CSP 引擎（Content-Security-Policy）
├── cors.rs              # CORS 跨域检查
├── web_storage.rs       # localStorage/sessionStorage + document.cookie 桥接
│
├── compositor.rs        # 合成层（UI RGBA + Web RGBA 合并）
├── page_state.rs        # 页面冻结（4 级：Active→Light→Medium→Deep）
├── resource_scheduler.rs# 资源调度（4 级优先级 + LRU 缓存）
│
├── task_queue/          # 任务调度系统
│   ├── scheduler.rs     # 全局调度器（前台 75% + 后台 25% 线程池）
│   ├── task.rs          # Task 类型 + 优先级 + TaskHandle 取消令牌
│   ├── thread_pool.rs   # Work-Stealing 线程池（crossbeam-deque）
│   └── mod.rs           # 模块入口
│
├── mojo/                # Mojo IPC
│   ├── pipe.rs          # 双向消息管道
│   ├── message.rs       # 消息格式
│   └── interface.rs     # InterfaceBinding/InterfaceProxy
│
├── css/                 # CSS 值类型
├── html/                # HTML 解析辅助
└── examples/            # 示例（bridge_dom_test, baidu_test, web_test）
```

## 三、窗口架构

```
┌──────────────────────────────────────────────────┐
│  eframe 浮动工具栏窗口 (工具条)                    │ ← 独立窗口
│  ┌──────┬────────────────────────┬──────────────┐│    高度 80px
│  │ 标签  │ 后退 < > ↻ [URL 栏] Go│ ☆ ▼BK ▼LOG  ││    无边框
│  │ 栏    │ 书签栏 (可选)          │              ││    始终显示
│  └──────┴────────────────────────┴──────────────┘│    always-on-top
├──────────────────────────────────────────────────┤
│  winit 主窗口 (页面内容)                           │ ← 独立窗口
│                                                   │    紧贴工具条下方
│  ┌──────────────────────────────────────────┐     │    宽度同步
│  │  tiny-skia Pixmap → softbuffer Buffer    │     │
│  │  → buffer.present() → 屏幕               │     │
│  └──────────────────────────────────────────┘     │
│  ┌──────────────────────────────────────────┐     │
│  │  半透明状态条 (覆盖层底部, 26px)          │     │
│  └──────────────────────────────────────────┘     │
├──────────────────────────────────────────────────┤
│  状态栏窗口 (屏幕底部, 任务栏上方, 28px)           │ ← 独立窗口
│  ✓ https://example.com                           │    半透明
└──────────────────────────────────────────────────┘
```

**窗口联动**：
- 主窗口 `WM_MOVE` → 工具条窗口同步位置（紧贴上方）
- 主窗口 `WM_SIZE` → 工具条窗口同步宽度
- 关闭主窗口 → 退出整个应用

**渲染方式**：winit 0.30 `ApplicationHandler` trait + softbuffer 0.4 `Surface::buffer_mut()`
- 渲染器生成 RGBA 像素 → IPC 传递 → `WindowManager::update_pixels()` → winit 事件循环 `RedrawRequested` → `softbuffer Buffer` → 屏幕

## 四、渲染管线

```
HTML (字符串)
    │
    ▼
kuchiki::parse_html() → DOM 树 (NodeRef)
    │
    ├── extract_style_tags()  → CSS 文本
    ├── parse_css_rules()     → CSS 规则列表
    └── rules_to_style_map()  → StyleMap (selectors 匹配)
    │
    ▼
TaffyLayoutEngine::compute(dom)
    ├── build_from_noderef()   → 遍历 DOM 创建 taffy 节点
    ├── determine_style()      → 映射 CSS 属性到 taffy Style
    │   ├── display / width / height / margin / padding
    │   ├── flexbox 全套属性
    │   ├── position / inset / z-index
    │   └── border / background / font / text 等渲染属性
    ├── determine_background() / color() / font_size() / ...
    ├── taffy::compute_layout() → 绝对坐标
    └── apply_float_layout()   → float 元素后处理偏移
    │
    ▼
TaffyRenderer::render_dom()
    ├── 渲染背景（body background-color）
    ├── 按 z-index 排序子节点
    ├── 深度遍历渲染树
    │   ├── render_element_box() → 元素装饰（hr/blockquote/button/li/h1-h6）
    │   ├── render_box_shadow()  → box-shadow 高斯模糊
    │   ├── render_background_image() → cover/contain/sprite
    │   ├── render_border()     → solid/dashed/dotted
    │   ├── render_img_element()→ 网络/base64/本地图片
    │   ├── render_text()       → cosmic-text 自动换行
    │   └── overflow:hidden 裁剪
    └── painter.to_png() / pixmap.data()
    │
    ▼
RGBA 像素 → IPC (Mojo RenderResult) → Browser Process
    │
    ▼
winit 主窗口 RedrawRequested
    → softbuffer::Buffer::buffer_mut()
    → buffer.as_mut().copy_from_slice(&rgba_bgra)
    → buffer.present()
    → 屏幕
```

## 五、数据流（页面加载）

```
用户输入 URL / 点击链接 / 书签
    │
    ▼
BrowserProcessHost::navigate(url)
    │  IPC (Navigation 管道)
    ▼
run_renderer_process (渲染器线程)
    │
    ├── 发送 loading 帧 → IPC → GUI 显示"加载中..."
    │
    ├── load_document(url)
    │   ├── NetworkClient::fetch_html_blocking()  → reqwest HTTP
    │   │   ├── Cookie 附加（RFC 6265）
    │   │   ├── HTTP Cache 检查（Cache-Control/ETag）
    │   │   ├── CSP 检查（Content-Security-Policy）
    │   │   └── CORS 跨域验证
    │   ├── kuchiki::parse_html() → DOM 树
    │   ├── DomWrapper::build_node_index() → 数字 ID 索引
    │   └── renderer.set_document(doc)
    │
    ├── extract_style_tags() → CSS 文本
    ├── parse_css_rules() + rules_to_style_map() → StyleMap
    │
    ├── TaffyLayoutEngine::compute(dom)
    │   ├── 布局缓存检查（LayoutCache::try_use_cache）
    │   ├── 递归构建 taffy 节点
    │   ├── 样式计算（CSS 属性映射）
    │   └── taffy::compute_layout() + float/z-index 处理
    │
    ├── render_document()
    │   ├── 设置画布背景
    │   ├── render_tree_with_taffy() 深度遍历
    │   │   ├── 背景/边框/box-shadow
    │   │   ├── 图片（缓存/网络/base64）
    │   │   ├── 文本（cosmic-text）
    │   │   ├── overflow hidden 裁剪
    │   │   └── z-index 排序
    │   └── pixmap 编码 → RGBA 像素
    │
    └── IPC (RenderResult 管道) → RGBA 数据
    │
    ▼
BrowserApp::refresh_from_renderer()
    ├── 保存 RGBA 像素到 rgba_pixels
    └── WindowManager::update_pixels(w, h, rgba)
    │
    ▼
winit 事件循环 → RedrawRequested
    → softbuffer Buffer → 屏幕
```

## 六、事件流（用户交互）

```
用户点击
    │
    ▼
egui 工具栏事件
    ├── 标签页切换 → host.switch_to_tab()
    ├── 后退/前进 → navigate(url)
    ├── URL 输入 → Enter → navigate(url)
    ├── 刷新 → navigate(current_url)
    ├── 书签 → navigate(url)
    └── 关闭标签页 → host.close_renderer()
    │
    ▼
BrowserProcessHost::navigate(url)
    │  IPC (Navigation 管道)
    ▼
run_renderer_process 消息循环
    ├── NavigationMessage → 导航新页面
    ├── MouseClick → hit_test(x,y) → DOM 冒泡 → 链接导航
    ├── MouseMove → hover 状态更新 → 重绘
    ├── KeyPress → 输入框 value 修改 → 重绘
    └── Scroll → TODO: 滚动支持
    │
    ▼
渲染 → IPC → 屏幕
```

## 七、JS 引擎架构

```
JS 代码
    │
    ▼
BoaJsEngine / ObscuraJsEngine
    ├── polyfill JS 注入（Event, console, location, navigator）
    ├── 33 个原生 DOM 函数注册
    │   ├── getElementById / querySelector / querySelectorAll
    │   ├── innerHTML / textContent / className
    │   ├── appendChild / removeChild / insertBefore / replaceChild
    │   ├── setAttribute / getAttribute / removeAttribute
    │   ├── createElement / createTextNode
    │   └── document.cookie / localStorage / sessionStorage
    ├── 真实 fetch() 实现（Boa 后端，reqwest）
    ├── 真实 XMLHttpRequest 实现
    ├── 真实 setTimeout/setInterval/requestAnimationFrame
    └── JsDomBridge 脏节点追踪
    │
    ▼
GLOBAL_TIMER_QUEUE (lazy_static)
    └── tick_timers() → 每帧检查到期定时器 → 执行 JS 回调
    │
    ▼
JsDomBridge
    ├── DomMutationType 记录
    ├── 脏节点 HashSet
    └── has_pending_changes → 触发增量重布局 + 重绘
```

## 八、多进程 IPC 架构

```
Browser Process (主线程/UI)          Renderer Process #1 (线程)     Renderer Process #2 (线程)
┌──────────────────────┐           ┌──────────────────────┐      ┌──────────────────────┐
│ BrowserProcessHost   │           │ run_renderer_process │      │ run_renderer_process │
│                      │           │                      │      │                      │
│  Navigation Proxy ───┼───Mojo───►│  Navigation Binding  │      │  Navigation Binding  │
│                      │           │                      │      │                      │
│  InputEvent Proxy ───┼───Mojo───►│  InputEvent Binding  │      │  InputEvent Binding  │
│                      │           │                      │      │                      │
│  RenderResult Bind ◄─┼───Mojo───┤  RenderResult Proxy  │      │  RenderResult Proxy  │
│                      │           │                      │      │                      │
│  TabManager          │           │  Renderer            │      │  Renderer            │
│  └─ Tab #1 (active)──┼──────────►│  ├─ kuchiki DOM      │      │  ├─ kuchiki DOM      │
│  └─ Tab #2           │           │  ├─ TaffyLayout      │      │  ├─ TaffyLayout      │
│  └─ Tab #3           │           │  ├─ tiny-skia Pixmap │      │  ├─ tiny-skia Pixmap │
└──────────────────────┘           └──────────────────────┘      └──────────────────────┘

Mojo IPC 管道（每个标签页 3 条）：
┌────────────────────────────────────────────────────────────┐
│ Navigation  (Browser → Renderer) : Navigate / Resize       │
│ InputEvent  (Browser → Renderer) : MouseClick/Move/KeyPress│
│ RenderResult(Renderer → Browser) : RGBA pixels + title     │
└────────────────────────────────────────────────────────────┘
```

## 九、跨平台渲染路径

```
tiny-skia Pixmap
    │
    ├── Pixmap::data() → &[u8] (RGBA)
    │
    └── softbuffer 0.4 API
        ├── Context::new(window)           # 创建上下文
        ├── Surface::new(&context, window) # 创建表面
        ├── surface.buffer_mut() → Buffer  # 获取缓冲区
        ├── buffer.as_mut()[..len]         # 写入 RGBA→BGRA 像素
        └── buffer.present()              # 推送到屏幕

Windows:  DirectX surface blit
Linux:    dri/Mesa buffer swap
macOS:    Metal texture upload
Web:      CanvasRenderingContext2D
```

## 十、特性矩阵

| feature | 默认 | 说明 |
|---------|------|------|
| `browser` | ✅ | 浏览器模式（GUI + 完整功能） |
| `embed` | | 嵌入模式（无 GUI，WebNativeBridge API） |
| `boa` | ✅ | Boa JS 引擎（纯 Rust） |
| `v8` | | V8 JS 引擎（deno_core） |
| `gui` | ✅ | GUI 窗口（eframe + winit + softbuffer，由 browser 隐含） |
| `headless` | | 无 GUI 模式（embed 别名） |

## 十一、测试

```bash
# 单元测试（305 个）
cargo test --lib --no-default-features --features headless

# Bridge 接口测试（23 项）
cargo test --lib bridge::tests --no-default-features --features headless

# 渲染测试
cargo run --example bridge_dom_test --no-default-features --features headless

# 百度渲染测试
cargo run --example baidu_test

# 网页加载测试
cargo run --example web_test
```

## 十二、关键依赖

| 库 | 版本 | 用途 |
|---|---|---|
| taffy | 0.10（本地） | CSS 布局引擎 |
| tiny-skia | 0.12（本地） | 2D 渲染 |
| cosmic-text | 0.19（本地） | 文本排版 |
| kuchiki | 0.12（本地） | HTML 解析 |
| selectors/cssparser | 0.27/0.35 | CSS 选择器/语法 |
| winit | 0.30 | 跨平台窗口管理 |
| softbuffer | 0.4 | 软件像素推送 |
| eframe/egui | 0.34 | 工具条 GUI |
| reqwest | 0.12 | HTTP 客户端 |
| boa_engine | 0.21 | JS 引擎（纯 Rust） |
| image | 0.25 | 图片解码 |
| httpdate | 1.0 | HTTP 日期解析 |
| crossbeam-deque | 0.8 | Work-Stealing 线程池 |
| url | 2.5 | URL 解析 |

## 十三、已知限制

| 限制 | 说明 |
|------|------|
| taffy 0.10 版本上限 | 布局引擎功能受限于此版本 |
| float 布局 | 通过后处理偏移模拟，非原生 taffy 支持 |
| table 布局 | 仅有 Block display 默认值，无表格布局引擎 |
| `<iframe>` / `<canvas>` | 未实现 |
| CSS transition/animation | 未实现 |
| V8 后端 fetch | polyfill 占位，无真实网络请求绑定 |
| console.log | 空函数（Boa 后端） |
| 中文 IME | Windows IME 组合字符未处理 |
| 多进程 | 当前为线程模拟，非真实进程隔离 |
