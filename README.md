# Rust Browser

Rust 编写的轻量级浏览器引擎，聚焦于 Web → Native 转换、高性能渲染和跨平台架构。

## 核心特性

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
| **305 测试通过** | 全模块单元测试覆盖 |

## 快速开始

```bash
# 默认模式（GUI + Boa JS）
cargo run

# Headless 模式（服务器端渲染/自动化测试）
cargo run --example bridge_dom_test --no-default-features --features headless

# 截图模式
cargo run -- "https://www.baidu.com" --output screenshot.png

# 指定 JS 引擎
cargo run --no-default-features --features browser,v8
```

## 项目结构

```
rust-browser/
├── Cargo.toml
├── README.md
├── ARCHITECTURE.md           # 完整架构文档
├── src/
│   ├── main.rs               # GUI 入口（eframe 工具条 + winit 主窗口）
│   ├── lib.rs                # 库入口
│   ├── bridge.rs             # WebNativeBridge trait 定义
│   ├── bridge_impl.rs        # Bridge 默认实现
│   ├── gui_window.rs         # winit + softbuffer 跨平台窗口管理
│   ├── compositor.rs         # UI 层与 Web 层合成
│   ├── js_dom_bridge.rs      # JS DOM 双向绑定桥接器
│   ├── js_timer_queue.rs     # 真实 JS 定时器调度
│   ├── js_task_scheduler.rs  # JS 任务分级调度
│   ├── web_storage.rs        # localStorage/sessionStorage/cookie
│   ├── csp.rs                # Content-Security-Policy 引擎
│   ├── cors.rs               # CORS 跨域检查
│   ├── http_cache.rs         # HTTP 缓存层
│   ├── page_state.rs         # 页面冻结机制
│   ├── resource_scheduler.rs # 资源优先级调度
│   ├── dom_wrapper.rs        # kuchiki DOM 包装器
│   ├── js_engine.rs          # JS 引擎（Boa/V8 双后端）
│   ├── network.rs            # HTTP 客户端（reqwest）
│   ├── browser/              # 浏览器核心
│   │   ├── mod.rs            # Browser 主类
│   │   ├── engine.rs         # 浏览器引擎
│   │   ├── page.rs           # 页面管理
│   │   ├── tabs.rs           # 标签页管理
│   │   └── ui.rs             # Chrome 风格 UI 渲染
│   ├── browser_process/      # 多进程 IPC
│   │   ├── host.rs           # BrowserProcessHost
│   │   └── interfaces.rs     # Mojo IPC 接口定义
│   ├── css/                  # CSS 值类型
│   ├── css_engine/           # CSS 引擎
│   ├── html/                 # HTML 解析
│   ├── mojo/                 # Mojo IPC 管道
│   ├── task_queue/           # 任务队列（Work-Stealing 线程池）
│   └── renderer/             # 渲染引擎
│       ├── renderer.rs       # 主渲染器
│       ├── taffy_layout.rs   # taffy 布局引擎
│       ├── async_pipeline.rs # 异步渲染管线
│       ├── layout_cache.rs   # 布局缓存
│       ├── image_pipeline.rs # 图片管道
│       ├── painter.rs        # tiny-skia 绘制
│       ├── text.rs           # cosmic-text 排版
│       ├── border.rs         # border/box-shadow
│       └── context.rs        # 渲染上下文
└── examples/
    ├── bridge_dom_test.rs    # Bridge 渲染 + 事件测试
    ├── baidu_test.rs         # 百度渲染测试
    └── web_test.rs           # 网页能力测试
```
