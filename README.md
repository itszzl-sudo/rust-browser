# Rust Browser - 基于 Taffy + tiny-skia 的浏览器引擎

一个轻量级的浏览器渲染引擎，使用 Rust 语言开发。

## 核心特性

- **Taffy 布局引擎** - CSS Flexbox/Grid/Block/Inline/Position 布局计算
- **完整 CSS 选择器** - 基于 selectors crate（Mozilla Servo），支持标签/类/ID/后代/属性/伪类
- **tiny-skia 渲染** - 高性能 2D 图形绘制，支持 border、圆角、box-shadow
- **cosmic-text 排版** - 支持 font-size、color、font-family，中文文本渲染
- **网络图片加载** - 基于 reqwest + image crate 的异步图片下载与缓存
- **CSS sprite 裁剪** - 支持 background-position 子图裁剪
- **SVG 渲染** - 基于 resvg 的完整 SVG 支持
- **JS 引擎** - 基于 obscura-js（deno_core/V8），完整 DOM API（document.querySelector 等）
- **Web → Native 桥接** - WebNativeBridge，统一 DOM/CSS/JS/布局/渲染/事件 API
- **点击测试** - hit_test + 事件 IPC，支持鼠标点击到元素的命中检测
- **多进程架构** - Browser Process + Renderer Process 分离，Mojo IPC 通信
- **Chrome 风格 UI** - 完整的标签页、地址栏、书签栏
- **多标签页** - 创建、切换、关闭标签页
- **HTTP 网络请求** - 加载网页内容
- **截图功能** - 生成 PNG 图像

## 项目结构

```
rust-browser/
├── Cargo.toml                # 项目配置
├── README.md                 # 项目说明
├── src/
│   ├── main.rs               # 命令行入口 / Chrome 风格 GUI
│   ├── lib.rs                # 库入口
│   ├── bridge.rs             # WebNativeBridge（Web → Native 桥接层）
│   ├── browser/              # 浏览器核心
│   │   ├── mod.rs            # Browser 主类
│   │   ├── engine.rs         # 浏览器引擎（导航/JS执行/DOM管理）
│   │   ├── page.rs           # 页面管理
│   │   ├── tabs.rs           # 标签页管理
│   │   └── ui.rs             # Chrome 风格 UI 绘制
│   ├── browser_process/      # 浏览器进程（多进程）
│   │   ├── host.rs           # BrowserProcessHost
│   │   └── interfaces.rs     # Mojo IPC 接口定义
│   ├── css/                  # CSS 值类型
│   │   ├── mod.rs
│   │   ├── parser.rs
│   │   ├── stylesheet.rs
│   │   └── values.rs
│   ├── css_engine/           # CSS 引擎（selectors crate 适配）
│   │   ├── mod.rs            # CSS 解析 + 声明类型
│   │   └── selector.rs       # 完整 CSS 选择器匹配
│   ├── dom/                  # 旧版 DOM（过渡）
│   ├── dom_obscura/          # obscura-dom 适配层
│   ├── dom_wrapper.rs        # kuchiki DOM 包装器（主用）
│   ├── html/                 # HTML 解析
│   ├── js_engine.rs          # JS 引擎（obscura-js/V8）
│   ├── network.rs            # HTTP 网络客户端
│   ├── renderer/             # 渲染引擎
│   │   ├── mod.rs
│   │   ├── renderer.rs       # 主渲染器（TaffyRenderer）
│   │   ├── taffy_layout.rs   # Taffy 布局引擎
│   │   ├── border.rs         # border/box-shadow 绘制
│   │   ├── image_cache.rs    # 网络图片加载/缓存
│   │   ├── svg.rs            # resvg SVG 渲染
│   │   ├── painter.rs        # tiny-skia 绘制
│   │   ├── layout.rs         # 旧版布局（过渡）
│   │   ├── context.rs        # 渲染上下文
│   │   └── text.rs           # 文本处理
│   ├── renderer_process/     # 渲染器进程
│   ├── storage/              # 存储模块
│   │   ├── cookie.rs         # Cookie 存储
│   │   ├── history.rs        # 浏览历史
│   │   └── local_storage.rs  # localStorage
│   └── task_queue/           # 任务队列
└── examples/
    ├── example.html           # 示例 HTML
    ├── baidu_test.rs          # 渲染测试
    └── web_test.rs            # 网页测试
```

## 渲染管线

```
HTML → kuchiki DOM ─→ css_engine（selectors 0.27 完整选择器）
                          ↓
                    TaffyLayoutEngine → taffy（Block/Inline/Flex/Grid/Position）
                          ↓
                    TaffyLayoutNode[]  ← hit_test(x,y)
                          ↓
                    TaffyRenderer
    ├─ cosmic-text（font-size/color/family）
    ├─ border/box-shadow（tiny-skia）
    ├─ ImageCache（reqwest + image）
    ├─ resvg（SVG）
    └─ text-decoration
                          ↓
              tiny-skia Pixmap → PNG
```

## WebNativeBridge API

供 web-to-native 工具产出的 Rust 代码直接调用：

```rust
use rust_browser::bridge::WebNativeBridge;

let mut bridge = WebNativeBridge::new(1280, 720);

// ① 写入 Vite 产物 DOM
bridge.set_html(r#"<div id="app"><button id="btn">Click</button></div>"#);

// ② 写入 CSS
bridge.set_css("#btn { background: blue; border-radius: 8px; }");
bridge.set_style("#btn", "color", "white");

// ③ 执行 JS
bridge.eval_js("console.log('hello')");

// ④ 绑定事件到 Rust 回调
bridge.on_click("#btn", |x, y| {
    println!("按钮点击: {}, {}", x, y);
});

// ⑤ 渲染 → PNG
let png: Vec<u8> = bridge.render();

// ⑥ Rust 侧修改 DOM/CSS → 重新渲染
bridge.set_style("#btn", "background", "red");
let png2 = bridge.render();

// ⑦ 获取元素位置
let rect = bridge.get_rect("#btn");

// ⑧ 点击测试
bridge.handle_click(100.0, 200.0);
```

### API 完整清单

| 类别 | 方法 | 说明 |
|------|------|------|
| DOM 读写 | `set_html`, `query`, `query_all`, `tag_name`, `get_attr`, `set_attr`, `text`, `query_text`, `get_rect`, `all_rects`, `hit_test` | |
| CSS 操作 | `set_css`, `set_style`, `clear_css` | |
| JS 执行 | `eval_js` | 需要 `--features js` |
| 渲染 | `render` → `Vec<u8>` PNG | |
| 事件绑定 | `on_click`, `on_form_submit`, `handle_click`, `handle_form_submit` | |
| 工具 | `dom`, `dom_mut`, `renderer`, `layout`, `set_viewport`, `viewport` | |

## 依赖库

| 库 | 版本 | 用途 |
|---|------|------|
| taffy | 0.10（本地） | CSS 布局引擎 |
| tiny-skia | 0.12（本地） | 2D 渲染 |
| cosmic-text | 0.19（本地） | 文本排版 |
| kuchiki | 0.12（本地） | HTML 解析 |
| selectors | 0.27 | CSS 选择器 |
| cssparser | 0.35 | CSS 语法解析 |
| resvg | 0.47 | SVG 渲染 |
| reqwest | 0.12 | HTTP 客户端 |
| image | 0.25 | 图片解码 |
| boa_engine | 0.21 | JS 引擎（纯 Rust，默认） |
| obscura-js | 本地 | JS 引擎（V8/deno_core，可选） |
| obscura-dom | 本地 | DOM 树（仅 obscura-js 使用） |
| regex | 1.11 | 预扫描 HTML |
| eframe/egui | 0.34 | GUI 窗口（可选） |

## 构建

## Feature 矩阵

| feature | 说明 |
|---------|------|
| `boa`（默认） | Boa JS 引擎（纯 Rust，0 原生依赖） |
| `js` | obscura-js（V8/deno_core） |
| `gui`（默认） | GUI 窗口（eframe/egui） |

> `boa` 和 `js` 互斥，不能同时启用。

```bash
# 默认构建（Boa JS + GUI）
cargo build

# 调试模式（opt-level=1，平衡编译速度和运行性能）
cargo build --profile dev

# 无 GUI 纯 lib 构建
cargo check -p rust-browser --lib --no-default-features

# V8 JS 引擎构建
cargo build --features js,gui

# 测试
cargo test

# 渲染相关测试
cargo test -- image_cache border svg taffy renderer text css_engine bridge

# 百度渲染测试
cargo run --example baidu_test
```

当前状态：渲染相关测试 **34/34 通过**，全量测试 **107/111 通过**（4 个 storage/IPC 测试失败，与渲染无关）。

## 许可证

MIT
