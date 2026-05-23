# Rust Browser 项目 Code Wiki

> 文档版本：1.0  
> 生成日期：2026-05-22  
> 项目版本：0.1.0

---

## 一、项目概述

### 1.1 项目简介

**Rust Browser** 是一个使用 Rust 语言编写的轻量级浏览器渲染引擎，聚焦于 Web → Native 转换、高性能渲染和跨平台架构。该项目采用 Chrome 浏览器的多进程架构设计，通过模块化设计实现了 DOM 解析、CSS 布局、像素渲染、JS 执行等核心功能。

### 1.2 核心特性

| 特性 | 说明 |
|------|------|
| **渲染引擎** | kuchiki DOM → taffy 布局 → tiny-skia 像素渲染 → cosmic-text 文本排版 |
| **CSS 引擎** | 基于 selectors crate（Mozilla Servo），支持完整 CSS 选择器 |
| **JS 引擎** | Boa（纯 Rust，默认）/ V8（deno_core）双后端，真实 DOM 绑定 |
| **多进程架构** | Browser Process + Renderer Process，Mojo IPC 通信 |
| **跨平台 GUI** | winit + softbuffer 窗口管理（非 GDI），egui 浮动工具条 |
| **HTTP 缓存** | RFC 7234 缓存层，Cache-Control/ETag/304 条件请求 |
| **安全** | CSP Content-Security-Policy / CORS 跨域检查 |
| **JS DOM 绑定** | 33 个原生 DOM 函数，脏节点追踪增量重绘 |
| **JS 定时器** | 真实异步 setTimeout/setInterval/requestAnimationFrame |
| **Web Storage** | localStorage / sessionStorage / document.cookie JS 桥接 |
| **Web → Native 桥接** | WebNativeBridge trait，统一 DOM/CSS/JS/事件 API |
| **资源调度** | 4 级优先级 + 页面冻结机制 |
| **Headless 模式** | 无 GUI，纯 CLI 渲染，CI/CD 截图 |
| **测试覆盖** | 305 个测试用例覆盖全模块 |

### 1.3 项目结构

```
rust-browser/
├── Cargo.toml              # 项目配置和依赖管理
├── README.md               # 项目说明
├── ARCHITECTURE.md         # 架构设计文档
├── SESSION_SUMMARY.md     # 会话摘要
├── test.html              # 测试 HTML 文件
│
├── src/                    # 源代码目录
│   ├── main.rs            # 程序入口（GUI + CLI）
│   ├── lib.rs             # 库入口，模块导出
│   │
│   ├── browser/           # 浏览器核心模块
│   │   ├── mod.rs         # Browser 主类
│   │   ├── engine.rs      # BrowserEngine 核心引擎
│   │   ├── page.rs        # 页面管理
│   │   ├── tabs.rs        # 标签页管理
│   │   └── ui.rs          # Chrome 风格 UI
│   │
│   ├── browser_process/   # 多进程 IPC 模块
│   │   ├── host.rs        # BrowserProcessHost 进程管理
│   │   └── interfaces.rs  # Mojo IPC 接口定义
│   │
│   ├── renderer/          # 渲染引擎模块
│   │   ├── renderer.rs    # 主渲染器（TaffyRenderer）
│   │   ├── taffy_layout.rs# taffy 布局引擎
│   │   ├── painter.rs     # tiny-skia 绘制
│   │   ├── text.rs        # cosmic-text 文本排版
│   │   ├── border.rs      # border/box-shadow 绘制
│   │   ├── async_pipeline.rs # 异步渲染管线
│   │   ├── layout_cache.rs  # 布局缓存
│   │   ├── image_pipeline.rs # 图片管道
│   │   ├── image_cache.rs   # 图片缓存
│   │   ├── context.rs     # 渲染上下文
│   │   ├── cursor.rs      # 光标渲染
│   │   └── mod.rs         # 模块入口
│   │
│   ├── css/               # CSS 值类型
│   │   ├── mod.rs         # CSS 模块入口
│   │   └── values.rs      # CSS 值类型定义
│   │
│   ├── css_engine/        # CSS 引擎模块
│   │   ├── mod.rs         # CSS 规则解析
│   │   └── selector.rs    # CSS 选择器
│   │
│   ├── html/              # HTML 解析模块
│   │   ├── mod.rs         # HTML 模块入口
│   │   └── parser.rs      # HTML 解析器
│   │
│   ├── task_queue/        # 任务调度系统
│   │   ├── mod.rs         # 模块入口
│   │   ├── scheduler.rs   # 全局调度器
│   │   ├── task.rs        # 任务类型
│   │   └── thread_pool.rs # Work-Stealing 线程池
│   │
│   ├── mojo/              # Mojo IPC 通信
│   │   ├── mod.rs         # Mojo 模块入口
│   │   ├── pipe.rs        # 消息管道
│   │   ├── message.rs     # 消息格式
│   │   ├── interface.rs   # 接口绑定
│   │   └── process_ipc.rs # 进程间通信
│   │
│   ├── bridge.rs          # WebNativeBridge trait 定义
│   ├── bridge_impl.rs     # Bridge 默认实现
│   ├── dom_wrapper.rs     # kuchiki DOM 包装器
│   ├── js_engine.rs       # JS 引擎（Boa/V8 双后端）
│   ├── js_dom_bridge.rs   # JS DOM 双向绑定
│   ├── js_timer_queue.rs  # JS 定时器队列
│   ├── js_task_scheduler.rs # JS 任务调度
│   ├── network.rs         # HTTP 客户端（reqwest）
│   ├── http_cache.rs      # HTTP 缓存层
│   ├── csp.rs             # Content-Security-Policy 引擎
│   ├── cors.rs            # CORS 跨域检查
│   ├── web_storage.rs     # localStorage/sessionStorage
│   ├── compositor.rs      # 合成层
│   ├── page_state.rs      # 页面冻结机制
│   ├── resource_scheduler.rs # 资源调度
│   └── gui_window.rs      # winit 窗口管理
│
└── examples/              # 示例程序
    ├── bridge_dom_test.rs # Bridge 接口测试
    ├── baidu_test.rs      # 百度渲染测试
    ├── web_test.rs        # 网页能力测试
    └── example.html       # 测试 HTML
```

---

## 二、核心模块详解

### 2.1 入口模块 (lib.rs)

**文件路径**：`src/lib.rs`

**模块职责**：作为库的公共入口，导出所有公共类型和模块。

**关键导出**：

```rust
// 核心类型导出
pub use browser::{Browser, BrowserEngine, BrowserError, Document, DEFAULT_HOME_URL};
pub use browser::{Tab, TabManager};

// 渲染相关
pub use renderer::Renderer;
pub use compositor::CompositorFrame;
pub use css::values::Color;

// DOM 相关
pub use dom_wrapper::DomWrapper;

// 任务调度
pub use js_task_scheduler::JsTaskScheduler;

// 网络相关
pub use network::NetworkClient;

// 页面状态
pub use page_state::{FreezeLevel, PageFreezeState, PageFreezer};

// 资源调度
pub use resource_scheduler::{ResourcePriority, ResourceRequest, ResourceScheduler, ResourceType};
```

**公开模块列表**：

| 模块 | 说明 |
|------|------|
| `browser` | 浏览器核心 |
| `browser_process` | 多进程 IPC |
| `cors` | CORS 跨域检查 |
| `csp` | CSP 安全策略 |
| `css` | CSS 值类型 |
| `css_engine` | CSS 引擎 |
| `dom_wrapper` | DOM 包装器 |
| `http_cache` | HTTP 缓存 |
| `js_dom_bridge` | JS DOM 绑定 |
| `js_timer_queue` | JS 定时器 |
| `js_engine` | JS 引擎（条件编译） |
| `mojo` | Mojo IPC |
| `network` | 网络请求 |
| `renderer` | 渲染引擎 |
| `task_queue` | 任务队列 |
| `web_storage` | Web 存储 |

---

### 2.2 浏览器核心模块 (browser)

#### 2.2.1 Browser 主类

**文件路径**：`src/browser/mod.rs`

**结构定义**：

```rust
pub struct Browser {
    engines: Vec<BrowserEngine>,   // 每个标签页对应一个引擎
    tab_manager: TabManager,         // 标签页管理器
    width: u32,                     // 视口宽度
    height: u32,                    // 视口高度
}
```

**关键方法**：

| 方法 | 说明 |
|------|------|
| `new()` | 创建浏览器实例 |
| `load_default()` | 加载默认首页 |
| `load_url()` | 加载指定 URL |
| `navigate()` | 导航到 URL |
| `new_tab()` | 创建新标签页 |
| `close_tab()` | 关闭标签页 |
| `switch_to_tab()` | 切换标签页 |
| `go_back()` | 后退 |
| `go_forward()` | 前进 |
| `reload()` | 刷新 |
| `evaluate()` | 执行 JS 代码 |
| `screenshot()` | 截图保存 |
| `render_full()` | 渲染全页面 |

**常量**：

```rust
pub const DEFAULT_HOME_URL: &str = "https://www.baidu.com";  // 默认首页
const DEFAULT_UI_HEIGHT: u32 = 40;                           // UI 工具栏高度
```

#### 2.2.2 BrowserEngine 引擎类

**文件路径**：`src/browser/engine.rs`

**错误类型**：

```rust
pub enum BrowserError {
    InitError(String),        // 初始化失败
    NavigationError(String),  // 导航失败
    RenderError(String),      // 渲染失败
    PageNotLoaded,            // 页面未加载
    NetworkError(String),     // 网络请求失败
    JsError(String),          // JS 执行失败
}
```

**Document 结构**：

```rust
#[derive(Clone)]
pub struct Document {
    pub dom: DomWrapper,           // DOM 树
    pub title: Option<String>,      // 页面标题
    pub url: String,                // 页面 URL
}
```

**引擎结构**：

```rust
pub struct BrowserEngine {
    renderer: Renderer,             // 渲染器
    document: Option<Document>,     // 当前文档
    width: u32,                    // 视口宽度
    height: u32,                   // 视口高度
    title: Option<String>,         // 页面标题
    current_url: Option<String>,   // 当前 URL
    current_favicon: Option<String>,// Favicon
    network_client: NetworkClient,   // 网络客户端
    js_engine: JsEngine,           // JS 引擎
}
```

**关键方法**：

| 方法 | 说明 |
|------|------|
| `new(width, height)` | 创建引擎 |
| `navigate()` | 导航（同步） |
| `navigate_async()` | 导航（异步） |
| `load_html()` | 加载 HTML 内容 |
| `load_local_file()` | 加载本地文件 |
| `run_page_scripts()` | 执行页面脚本 |
| `extract_favicon()` | 提取 Favicon |
| `render_to_image()` | 渲染为图片 |
| `execute_js()` | 执行 JS 代码 |

---

### 2.3 渲染引擎模块 (renderer)

#### 2.3.1 主渲染器 Renderer

**文件路径**：`src/renderer/renderer.rs`

**结构定义**：

```rust
pub struct Renderer {
    context: RenderContext,              // 渲染上下文
    painter: Painter,                     // 绘制器
    text_renderer: TextRenderer,          // 文本渲染器
    document: Option<Document>,           // 当前文档
    title: Option<String>,                // 页面标题
    last_taffy: Option<TaffyLayoutEngine>,// 上次布局结果
    page_png: Option<Vec<u8>>,           // 页面 PNG 缓存
    is_loading: bool,                     // 加载状态
    focused_node: Option<usize>,          // 焦点节点
    cursor: CursorRenderer,               // 光标渲染器
    scroll_offset_y: f32,                 // 滚动偏移
    content_height: f32,                  // 内容高度
    js_dom_bridge: Option<JsDomBridge>,  // JS DOM 桥接
    js_engine: Option<JsEngine>,          // JS 引擎
}
```

**关键方法**：

| 方法 | 说明 |
|------|------|
| `new(width, height)` | 创建渲染器 |
| `set_viewport()` | 设置视口尺寸 |
| `set_document()` | 设置文档 |
| `render()` | 渲染到 PNG |
| `render_to_rgba()` | 渲染到 RGBA 像素 |
| `render_to_png()` | 渲染到 PNG 字节 |
| `render_loading_page()` | 渲染加载画面 |
| `render_blank_page()` | 渲染空白页面 |
| `capture_viewport()` | 捕获视口 |
| `save()` | 保存为文件 |
| `set_scroll_offset()` | 设置滚动偏移 |
| `focus_by_click()` | 点击设置焦点 |
| `hit_test_link()` | 点击测试链接 |
| `js_engine_tick_timers()` | JS 定时器 tick |

**内部渲染器 TaffyRenderer**：

```rust
struct TaffyRenderer<'a> {
    painter: &'a mut Painter,
    taffy: &'a TaffyLayoutEngine,
    dom: &'a DomWrapper,
    node_index_cache: HashMap<u64, usize>,
    scroll_offset_y: f32,
}
```

#### 2.3.2 Taffy 布局引擎

**文件路径**：`src/renderer/taffy_layout.rs`

**核心结构**：

```rust
pub struct TaffyLayoutNode {
    pub node: NodeId,              // taffy 节点 ID
    pub dom_node: usize,          // DOM 节点索引
    pub tag_name: String,          // 标签名
    pub x: f32,                   // X 坐标
    pub y: f32,                   // Y 坐标
    pub width: f32,               // 宽度
    pub height: f32,              // 高度
    pub background: Option<Color>,// 背景色
    pub color: Option<Color>,     // 文字颜色
    pub font_size: f32,          // 字体大小
    pub font_color: Option<Color>,// 字体颜色
    pub position_type: PositionType, // CSS position
    pub float_type: FloatType,    // CSS float
    pub border_radius: f32,       // 圆角
    pub box_shadow: Option<String>, // 阴影
    pub overflow_x: String,        // overflow-x
    pub overflow_y: String,        // overflow-y
    pub z_index: i32,            // z-index
    pub opacity: f32,            // 透明度
}
```

**布局引擎**：

```rust
pub struct TaffyLayoutEngine {
    taffy: TaffyTree,                    // taffy 树
    layout_nodes: Vec<TaffyLayoutNode>,  // 布局节点列表
    dom_to_layout: HashMap<usize, usize>,// DOM→布局映射
    root: Option<NodeId>,               // 根节点
    viewport: Size<AvailableSpace>,      // 视口
    style_map: StyleMap,                 // 样式映射
    hovered_node: Option<usize>,         // hover 节点
    focused_node: Option<usize>,         // 焦点节点
}
```

**关键方法**：

| 方法 | 说明 |
|------|------|
| `new(w, h)` | 创建布局引擎 |
| `compute(dom)` | 计算布局 |
| `set_viewport()` | 设置视口 |
| `get_layout()` | 获取节点布局 |
| `hit_test()` | 点击测试 |
| `find_by_tag()` | 按标签查找 |
| `document_height()` | 获取文档高度 |

#### 2.3.3 绘制器 Painter

**文件路径**：`src/renderer/painter.rs`

**职责**：使用 tiny-skia 进行 2D 绘制。

**关键方法**：

| 方法 | 说明 |
|------|------|
| `new(width, height)` | 创建绘制器 |
| `set_viewport()` | 设置视口 |
| `set_background()` | 设置背景色 |
| `set_clip()` | 设置裁剪区域 |
| `clear_clip()` | 清除裁剪 |
| `draw_rect()` | 绘制矩形 |
| `draw_rounded_rect()` | 绘制圆角矩形 |
| `draw_rect_border()` | 绘制矩形边框 |
| `draw_rounded_border()` | 绘制圆角边框 |
| `to_png()` | 输出 PNG |
| `save_png()` | 保存 PNG 文件 |

#### 2.3.4 文本渲染器 TextRenderer

**文件路径**：`src/renderer/text.rs`

**职责**：使用 cosmic-text 进行文本排版和渲染。

**关键方法**：

| 方法 | 说明 |
|------|------|
| `new()` | 创建文本渲染器 |
| `render_text()` | 渲染文本 |
| `measure_text()` | 测量文本尺寸 |

---

### 2.4 DOM 模块 (dom_wrapper)

**文件路径**：`src/dom_wrapper.rs`

**结构定义**：

```rust
pub struct DomWrapper {
    document: NodeRef,                       // kuchiki 文档节点
    url: Option<Url>,                       // 文档 URL
    node_list: Vec<NodeRef>,               // 节点列表
    node_id_map: HashMap<usize, u64>,      // 指针→ID 映射
    id_to_index: HashMap<u64, usize>,      // ID→索引映射
}
```

**关键方法**：

| 方法 | 说明 |
|------|------|
| `from_html()` | 从 HTML 解析 |
| `get_node()` | 获取节点 |
| `children()` | 获取子节点 |
| `tag_name()` | 获取标签名 |
| `attribute()` | 获取属性 |
| `set_attribute()` | 设置属性 |
| `text_content()` | 获取文本内容 |
| `select()` | CSS 选择器查询 |
| `select_first()` | 查询首个匹配 |
| `body()` | 获取 body 元素 |
| `title()` | 获取页面标题 |
| `parent()` | 获取父节点 |
| `traverse_elements()` | 遍历元素 |
| `node_id()` | 获取节点 ID |
| `index_of_node()` | 节点→索引转换 |

**节点 ID 机制**：
- 使用全局单调递增的 64 位 ID
- 类似 Blink 的 WeakMap 句柄表
- 永不回收，保证 ID 稳定性

---

### 2.5 网络模块 (network)

**文件路径**：`src/network.rs`

**NetworkClient 结构**：

```rust
pub struct NetworkClient {
    timeout_secs: u64,              // 超时秒数
    enable_cookies: bool,          // Cookie 支持
    custom_ua: Option<String>,     // 自定义 UA
    cookie_jar: SharedCookieJar,   // Cookie 存储
}
```

**错误类型**：

```rust
pub enum NetworkError {
    InvalidUrl(String),             // URL 解析失败
    RequestFailed(String),          // 请求失败
    ReadBodyFailed(String),         // 读取响应失败
    HttpStatus(u16),                // HTTP 状态码错误
    ClientCreationFailed(String),   // 客户端创建失败
}
```

**响应结构**：

```rust
pub struct HttpResponse {
    pub status: u16,               // 状态码
    pub headers: HashMap<String, String>, // 响应头
    pub body: Vec<u8>,             // 响应体
    pub final_url: String,         // 最终 URL（重定向后）
}
```

**Cookie 结构**：

```rust
pub struct CookieEntry {
    pub name: String,              // Cookie 名称
    pub value: String,             // Cookie 值
    pub domain: String,            // 域名
    pub path: String,              // 路径
    pub secure: bool,              // 安全标志
    pub http_only: bool,           // HttpOnly 标志
    pub expires: Option<Instant>,  // 过期时间
}
```

**关键方法**：

| 方法 | 说明 |
|------|------|
| `new()` | 创建网络客户端 |
| `with_timeout()` | 设置超时 |
| `with_user_agent()` | 设置 UA |
| `with_cookies_disabled()` | 禁用 Cookie |
| `fetch()` | 异步 GET 请求 |
| `fetch_navigation()` | 异步导航请求 |
| `fetch_html()` | 获取 HTML |
| `fetch_html_blocking()` | 同步获取 HTML |
| `get()` | 同步 GET 请求 |
| `post()` | 同步 POST 请求 |
| `navigate_blocking()` | 同步导航请求 |

**全局静态量**：

```rust
lazy_static! {
    pub static ref GLOBAL_COOKIE_JAR: SharedCookieJar;  // 全局 Cookie 存储
}
```

---

### 2.6 JS 引擎模块 (js_engine)

**文件路径**：`src/js_engine.rs`

**支持的后端**：
- **Boa Engine**（默认）：纯 Rust 实现，无需 V8
- **deno_core/V8**：需要 V8 依赖

**控制台日志**：

```rust
pub static CONSOLE_LOG_BUFFER: Mutex<Vec<String>>;
```

**Boa 后端结构**：

```rust
pub struct BoaJsEngine {
    context: Option<Context>,       // Boa 上下文
    url: String,                   // 当前 URL
    network: Rc<RefCell<NetworkClient>>, // 网络客户端
}
```

**关键方法**：

| 方法 | 说明 |
|------|------|
| `new()` | 创建 JS 引擎 |
| `initialize()` | 初始化运行时 |
| `evaluate()` | 执行 JS 代码 |
| `is_ready()` | 检查就绪状态 |
| `set_url()` | 设置当前 URL |
| `tick_timers()` | 定时器 tick |

**Polyfill 支持**：
- `console` 对象
- `setTimeout` / `setInterval`
- `requestAnimationFrame`
- `document` 对象
- `Canvas 2D` 上下文

---

### 2.7 任务队列模块 (task_queue)

**文件路径**：`src/task_queue/mod.rs`

**任务优先级**：

```rust
pub enum TaskPriority {
    BestEffort,    // 后台任务
    UserVisible,   // 用户可见任务
    UserBlocking,  // 用户阻塞任务
}
```

**任务结构**：

```rust
pub struct Task {
    priority: TaskPriority,
    task_fn: Box<dyn FnOnce() + Send>,
    handle: TaskHandle,
}
```

**线程池**：

```rust
pub struct ThreadPool {
    injector: Injector<Task>,
    workers: Vec<Worker<Task>>,
}
```

**调度器**：

```rust
pub struct TaskScheduler {
    foreground_pool: ThreadPool,
    background_pool: ThreadPool,
}
```

**关键方法**：

| 方法 | 说明 |
|------|------|
| `spawn()` | 提交任务 |
| `run_main_tasks()` | 运行主线程任务 |
| `default_thread_pool()` | 获取默认线程池 |

---

### 2.8 Mojo IPC 模块 (mojo)

**文件路径**：`src/mojo/`

**接口绑定**：

```rust
pub struct InterfaceBinding<T: MojoMessage> {
    tx: Sender<PipelineMessage>,
    _phantom: PhantomData<T>,
}

pub struct InterfaceProxy<T: MojoMessage> {
    rx: Receiver<PipelineMessage>,
    _phantom: PhantomData<T>,
}
```

**消息管道**：

```rust
pub struct MessagePipe {
    ends: (MessagePipeEndpoint, MessagePipeEndpoint),
}
```

**关键方法**：

| 方法 | 说明 |
|------|------|
| `send_message()` | 发送消息 |
| `receive_message()` | 接收消息 |
| `create_pair()` | 创建管道对 |

---

### 2.9 浏览器进程模块 (browser_process)

**文件路径**：`src/browser_process/host.rs`

**渲染器通道**：

```rust
pub struct RendererChannel {
    pub id: u64,                            // 渲染器 ID
    pub navigation_remote: InterfaceProxy,  // 导航接口
    pub input_remote: InterfaceProxy,       // 输入接口
    pub result_binding: InterfaceBinding,  // 结果绑定
    pub url: String,                        // 当前 URL
    pub title: Option<String>,             // 页面标题
    pub width: u32,                        // 视口宽度
    pub height: u32,                        // 视口高度
    pub child_process: Option<Child>,       // 子进程
}
```

**浏览器进程宿主**：

```rust
pub struct BrowserProcessHost {
    renderers: HashMap<u64, RendererChannel>, // 渲染器映射
    active_tab_id: Option<u64>,               // 活跃标签页
    use_real_process: bool,                  // 是否使用真实进程
}
```

**关键方法**：

| 方法 | 说明 |
|------|------|
| `new()` | 创建宿主 |
| `spawn_renderer()` | 创建渲染器 |
| `navigate()` | 导航 |
| `send_input()` | 发送输入 |
| `try_receive_result()` | 接收渲染结果 |
| `try_receive_rgba_result()` | 接收 RGBA 结果 |
| `switch_to_tab()` | 切换标签页 |
| `close_renderer()` | 关闭渲染器 |
| `active_renderer()` | 获取活跃渲染器 |

---

### 2.10 WebNativeBridge 模块 (bridge)

**文件路径**：`src/bridge.rs`

**核心 Trait**：

```rust
pub trait WebNativeBridge {
    fn new(width: u32, height: u32) -> Self;
    
    // DOM 操作
    fn set_html(&mut self, html: &str);
    fn query(&self, selector: &str) -> Option<usize>;
    fn query_all(&self, selector: &str) -> Vec<usize>;
    fn tag_name(&self, node_id: usize) -> Option<String>;
    fn get_attr(&self, node_id: usize, name: &str) -> Option<String>;
    fn set_attr(&mut self, node_id: usize, name: &str, value: &str);
    fn text(&self, node_id: usize) -> Option<String>;
    
    // 布局操作
    fn get_rect(&self, selector: &str) -> Option<LayoutRect>;
    fn all_rects(&self) -> Vec<LayoutNode>;
    fn hit_test(&self, x: f32, y: f32) -> Option<LayoutNode>;
    
    // CSS 操作
    fn set_css(&mut self, css_text: &str);
    fn set_style(&mut self, selector: &str, property: &str, value: &str);
    fn clear_css(&mut self);
    
    // JS 执行
    fn eval_js(&mut self, code: &str) -> String;
    
    // 渲染
    fn render(&mut self) -> Vec<u8>;
    
    // 事件绑定
    fn on_click(&mut self, selector: &str, handler: EventHandler);
    fn on_form_submit(&mut self, selector: &str, handler: FormHandler);
    fn on_window_open(&mut self, handler: WindowOpenHandler);
    fn handle_click(&mut self, x: f32, y: f32) -> bool;
    
    // 视口
    fn set_viewport(&mut self, width: u32, height: u32);
    fn viewport(&self) -> (u32, u32);
    
    // 网络
    fn navigate(&mut self, url: &str) -> Result<(), String>;
    fn http_get(&mut self, url: &str) -> Result<HttpResponse, String>;
    fn http_post(&mut self, url: &str, body: &[u8], ct: &str) -> Result<HttpResponse, String>;
    
    // 文件操作
    fn download_file(&mut self, url: &str, path: &str) -> Result<u64, String>;
    fn write_file(&mut self, path: &str, data: &[u8]) -> Result<(), String>;
    fn read_file(&mut self, path: &str) -> Result<Vec<u8>, String>;
}
```

**数据类型**：

```rust
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

pub struct LayoutRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

pub struct LayoutNode {
    pub dom_node: usize,
    pub tag_name: String,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub background: Option<Color>,
}
```

---

## 三、CSS 引擎模块

### 3.1 CSS 值类型 (css/values.rs)

**颜色类型**：

```rust
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    pub fn from_hex(hex: &str) -> Self;
    pub fn from_rgb(r: u8, g: u8, b: u8) -> Self;
    pub fn from_rgba(r: u8, g: u8, b: u8, a: u8) -> Self;
    pub fn to_rgba(&self) -> [u8; 4];
    pub fn to_hex(&self) -> String;
    pub const fn WHITE() -> Self;
    pub const fn BLACK() -> Self;
}
```

### 3.2 CSS 解析引擎 (css_engine/mod.rs)

**CSS 规则**：

```rust
pub struct CssRule {
    pub selector: String,
    pub declarations: Vec<Declaration>,
}

pub struct Declaration {
    pub property: String,
    pub value: String,
}
```

**StyleMap**：

```rust
pub struct StyleMap(HashMap<SelectorHash, Vec<Declaration>>);
```

**关键方法**：

| 方法 | 说明 |
|------|------|
| `parse_css_rules()` | 解析 CSS 规则 |
| `parse_inline_style()` | 解析内联样式 |
| `rules_to_style_map()` | 转换为 StyleMap |
| `get_declaration()` | 获取声明 |
| `parse_length()` | 解析长度值 |
| `parse_color()` | 解析颜色值 |

---

## 四、依赖关系

### 4.1 核心依赖

| 依赖 | 版本 | 说明 |
|------|------|------|
| **taffy** | 0.10（本地） | CSS 布局引擎 |
| **tiny-skia** | 0.12（本地） | 2D 渲染 |
| **cosmic-text** | 0.19（本地） | 文本排版 |
| **kuchiki** | 0.12（本地） | HTML 解析 |
| **selectors** | 0.27 | CSS 选择器 |
| **cssparser** | 0.35 | CSS 语法解析 |
| **reqwest** | 0.12 | HTTP 客户端 |
| **boa_engine** | 0.21 | JS 引擎（纯 Rust） |
| **deno_core** | 0.350 | V8 JS 引擎（可选） |

### 4.2 GUI 依赖

| 依赖 | 版本 | 说明 |
|------|------|------|
| **eframe** | 0.34 | GUI 框架 |
| **egui** | 0.34 | UI 组件库 |
| **winit** | 0.30 | 窗口管理 |
| **softbuffer** | 0.4 | 像素缓冲区 |
| **raw-window-handle** | 0.6 | 原生窗口句柄 |

### 4.3 工具依赖

| 依赖 | 版本 | 说明 |
|------|------|------|
| **tokio** | 1.x | 异步运行时 |
| **crossbeam-deque** | 0.8 | Work-Stealing 队列 |
| **image** | 0.25 | 图片编解码 |
| **url** | 2.5 | URL 解析 |
| **lazy_static** | 1.4 | 静态延迟初始化 |
| **log** | 0.4 | 日志框架 |
| **thiserror** | 2.0 | 错误处理 |
| **anyhow** | 1.0 | 错误传播 |

### 4.4 特性矩阵

| Feature | 默认 | 说明 |
|---------|------|------|
| `browser` | ✅ | 浏览器模式（GUI + 完整功能） |
| `embed` | | 嵌入模式（无 GUI，WebNativeBridge API） |
| `boa` | ✅ | Boa JS 引擎（纯 Rust） |
| `v8` | | V8 JS 引擎（deno_core） |
| `gui` | ✅ | GUI 窗口（eframe + winit + softbuffer） |
| `headless` | | 无 GUI 模式（embed 别名） |

---

## 五、运行方式

### 5.1 构建项目

```bash
# 默认模式（GUI + Boa JS）
cargo build

# Release 模式
cargo build --release
```

### 5.2 运行浏览器

```bash
# 默认模式（GUI + Boa JS）
cargo run

# 打开指定 URL
cargo run -- "https://www.example.com"

# 指定窗口尺寸
cargo run -- "https://www.example.com" --width 1920 --height 1080
```

### 5.3 Headless 模式

```bash
# 无 GUI 模式
cargo run --no-default-features --features headless

# 带 JS 引擎的 Headless 模式
cargo run --no-default-features --features headless,boa
```

### 5.4 截图模式

```bash
# 截图保存为 PNG
cargo run -- "https://www.baidu.com" --output screenshot.png

# 指定尺寸
cargo run -- "https://www.example.com" --output screenshot.png --width 1280 --height 720
```

### 5.5 运行示例

```bash
# Bridge 接口测试
cargo run --example bridge_dom_test --no-default-features --features headless

# 百度渲染测试
cargo run --example baidu_test

# 网页能力测试
cargo run --example web_test
```

### 5.6 运行测试

```bash
# 所有单元测试（305 个）
cargo test --lib --no-default-features --features headless

# Bridge 接口测试（23 项）
cargo test --lib bridge::tests --no-default-features --features headless

# 特定模块测试
cargo test --lib dom_wrapper
cargo test --lib network
```

---

## 六、程序入口 (main.rs)

**文件路径**：`src/main.rs`

### 6.1 命令行参数

```rust
#[derive(Parser, Debug)]
struct Args {
    url: String,              // 初始 URL（默认：百度）
    output: Option<PathBuf>, // 截图输出路径
    width: u32,              // 视口宽度（默认：1280）
    height: u32,             // 视口高度（默认：720）
    debug: bool,             // 调试模式
}
```

### 6.2 GUI 模式流程

1. 初始化日志系统
2. 解析命令行参数
3. 创建 `BrowserProcessHost`
4. 创建 `BrowserApp`（eframe App）
5. 启动 eframe 窗口
6. 创建渲染器进程
7. 建立 Mojo IPC 通信
8. 进入事件循环

### 6.3 Headless 模式流程

1. 初始化日志系统
2. 解析命令行参数
3. 创建 `BrowserProcessHost`
4. 创建渲染器进程
5. 等待渲染结果
6. 输出截图或完成

---

## 七、关键设计模式

### 7.1 多进程架构

```
┌─────────────────────────────────────────────────────────────┐
│                    Browser Process                            │
│  ┌─────────────────┐    ┌─────────────────┐                 │
│  │ BrowserApp      │    │ BrowserProcess  │                 │
│  │ (eframe UI)     │◄──►│ Host            │                 │
│  └─────────────────┘    └────────┬────────┘                 │
│                                  │                            │
│                    ┌─────────────┼─────────────┐             │
│                    │             │             │             │
│               ┌────▼────┐  ┌────▼────┐  ┌────▼────┐        │
│               │ Nav IPC │  │Input IPC│  │Result IPC│        │
│               └────┬────┘  └────┬────┘  └────┬────┘        │
│                    │             │             │              │
│  Renderer #1 ◄────┴─────────────┴─────────────┴────►        │
│  (thread)                                                │
│  ┌──────────────────────────────────────────┐              │
│  │ Renderer                                │              │
│  │ ├─ NetworkClient                       │              │
│  │ ├─ DomWrapper (kuchiki)                │              │
│  │ ├─ TaffyLayoutEngine                   │              │
│  │ ├─ TaffyRenderer → tiny-skia Pixmap   │              │
│  │ └─ JsEngine (Boa/V8)                   │              │
│  └──────────────────────────────────────────┘              │
└─────────────────────────────────────────────────────────────┘
```

### 7.2 渲染管线

```
HTML 字符串
    │
    ▼
kuchiki::parse_html() → DOM 树 (NodeRef)
    │
    ├── extract_style_tags() → CSS 文本
    ├── parse_css_rules() → CSS 规则
    └── rules_to_style_map() → StyleMap
    │
    ▼
TaffyLayoutEngine::compute()
    │
    ├── build_from_noderef() → 构建 taffy 节点
    ├── determine_style() → CSS 属性映射
    ├── taffy::compute_layout() → 布局计算
    └── apply_float_layout() → float 处理
    │
    ▼
TaffyRenderer::render_dom()
    │
    ├── render_background() → 背景
    ├── render_border() → 边框
    ├── render_box_shadow() → 阴影
    ├── render_background_image() → 背景图
    ├── render_img_element() → 图片
    ├── render_text() → 文本
    └── overflow:hidden 裁剪
    │
    ▼
tiny-skia Pixmap → RGBA 像素
    │
    ▼
Mojo IPC → Browser Process
    │
    ▼
winit + softbuffer → 屏幕显示
```

### 7.3 模块依赖图

```
┌────────────────────────────────────────────────────────┐
│                     main.rs / lib.rs                     │
└──────────────────────────┬─────────────────────────────┘
                           │
        ┌──────────────────┼──────────────────┐
        ▼                  ▼                  ▼
┌───────────────┐  ┌───────────────┐  ┌───────────────┐
│    browser     │  │   renderer    │  │   network     │
└───────┬───────┘  └───────┬───────┘  └───────────────┘
        │                  │
        │           ┌───────┴───────┐
        │           ▼               ▼
        │    ┌───────────┐  ┌───────────┐
        │    │  taffy_   │  │  painter  │
        │    │  layout   │  │  (tiny-   │
        │    └───────────┘  │   skia)   │
        │                   └───────────┘
        │
        ▼
┌───────────────┐  ┌───────────────┐  ┌───────────────┐
│   dom_wrapper │  │   css_engine  │  │   js_engine   │
│   (kuchiki)   │  └───────────────┘  └───────┬───────┘
└───────────────┘                              │
        │                              ┌───────┴───────┐
        │                              ▼               ▼
        │                      ┌───────────┐  ┌───────────┐
        │                      │   js_dom_ │  │js_timer_  │
        │                      │   bridge  │  │  queue    │
        │                      └───────────┘  └───────────┘
        │
        ▼
┌───────────────┐  ┌───────────────┐  ┌───────────────┐
│  task_queue    │  │    mojo       │  │   browser_    │
│                │  │                │  │   process     │
└───────────────┘  └───────────────┘  └───────────────┘
```

---

## 八、已知限制

| 限制 | 说明 |
|------|------|
| taffy 0.10 版本 | 布局引擎功能受限于此版本 |
| float 布局 | 通过后处理偏移模拟，非原生 taffy 支持 |
| table 布局 | 仅有 Block display 默认值，无表格布局引擎 |
| `<iframe>` | 已实现但有限制 |
| `<canvas>` | 未实现 |
| CSS transition/animation | 未实现 |
| V8 后端 fetch | polyfill 占位，无真实网络请求绑定 |
| console.log | 空函数（Boa 后端） |
| 中文 IME | Windows IME 组合字符未处理 |
| 多进程 | 当前为线程模拟，非真实进程隔离 |

---

## 九、API 使用示例

### 9.1 基本使用

```rust
use rust_browser::Browser;

let mut browser = Browser::new().unwrap();
browser.navigate("https://example.com").unwrap();
browser.screenshot("output.png").unwrap();
```

### 9.2 WebNativeBridge 使用

```rust
use rust_browser::bridge::{WebNativeBridge, DefaultWebNativeBridge};

// 创建桥接器
let mut bridge = DefaultWebNativeBridge::new(1280, 720);

// 设置 HTML
bridge.set_html(r#"<div id="app"><button id="btn">Click</button></div>"#);

// 绑定事件
bridge.on_click("#btn", Box::new(|x, y| {
    println!("按钮被点击: ({}, {})", x, y);
}));

// 渲染
let png = bridge.render();

// 修改样式
bridge.set_style("#btn", "background-color", "red");
let png2 = bridge.render();
```

### 9.3 JS 执行

```rust
let result = browser.evaluate("document.title");
println!("页面标题: {}", result);
```

---

## 十、参考资料

- **ARCHITECTURE.md** - 完整架构设计文档
- **README.md** - 项目说明和快速开始
- **Cargo.toml** - 依赖配置
- **examples/** - 示例程序

---

> 本文档由 Code Wiki 生成器自动创建
> 如有更新需求，请参考源代码中的最新实现
