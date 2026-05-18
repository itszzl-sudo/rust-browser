# Rust Browser - 基于 Taffy + tiny-skia 的浏览器引擎

一个轻量级的浏览器渲染引擎，使用 Rust 语言开发。

## 核心特性

- **Taffy 布局引擎** - CSS Flexbox/Grid/Block/Inline 布局计算
- **完整 CSS 选择器** - 基于 selectors crate（Mozilla Servo），支持标签/类/ID/后代/属性/伪类
- **tiny-skia 渲染** - 高性能 2D 图形绘制，支持 border、圆角、box-shadow
- **cosmic-text 排版** - 支持 font-size、color、font-family，中文文本渲染
- **网络图片加载** - 基于 reqwest + image crate 的图片下载与缓存
- **长页面截图** - 自动检测页面高度，输出完整 PNG（不限于视口）
- **JS 引擎** - 支持 Boa（纯 Rust，默认）和 obscura-js（V8/deno_core）双后端
- **Web → Native 桥接** - WebNativeBridge，统一 DOM/CSS/JS/布局/渲染/事件 API
- **事件冒泡** - 点击事件沿 DOM 树向上冒泡，命中任意祖先匹配的处理器
- **headless 模式** - 无 GUI/JS/首页加载，纯 CLI 渲染与测试
- **多进程架构** - Browser Process + Renderer Process 分离，Mojo IPC 通信
- **Chrome 风格 GUI** - 标签页、地址栏（可选，通过 `gui` feature）

## 项目结构

```
rust-browser/
├── Cargo.toml                # 项目配置（features: boa/js/gui/headless）
├── README.md                 # 项目说明
├── src/
│   ├── main.rs               # GUI 入口（Chrome 风格窗口，需要 gui feature）
│   ├── lib.rs                # 库入口
│   ├── bridge.rs             # WebNativeBridge（Web → Native 桥接层）
│   ├── browser/              # 浏览器核心
│   │   ├── mod.rs            # Browser 主类
│   │   ├── engine.rs         # 浏览器引擎（导航/JS执行/DOM管理）
│   │   ├── page.rs           # 页面管理
│   │   ├── tabs.rs           # 标签页管理
│   │   └── ui.rs             # Chrome 风格 UI 绘制
│   ├── browser_process/      # 多进程 IPC
│   │   ├── host.rs           # BrowserProcessHost
│   │   └── interfaces.rs     # Mojo IPC 接口定义
│   ├── css/                  # CSS 值类型
│   ├── css_engine/           # CSS 引擎（selectors crate 适配）
│   ├── dom_wrapper.rs        # kuchiki DOM 包装器
│   ├── js_engine.rs          # JS 引擎（Boa / obscura-js）
│   ├── network.rs            # HTTP 网络客户端（reqwest）
│   ├── renderer/             # 渲染引擎
│   │   ├── renderer.rs       # 主渲染器（支持长页面截图）
│   │   ├── taffy_layout.rs   # Taffy 布局引擎（绝对坐标）
│   │   ├── border.rs         # border/box-shadow 绘制
│   │   ├── image_cache.rs    # 网络图片加载/缓存
│   │   ├── painter.rs        # tiny-skia 绘制
│   │   ├── context.rs        # 渲染上下文
│   │   ├── text.rs           # cosmic-text 排版
│   │   └── cursor.rs         # 光标渲染
│   ├── mojo/                 # Mojo IPC 管道
│   └── task_queue/           # 任务队列
└── examples/
    ├── bridge_dom_test.rs     # bridge 渲染 + 事件测试（headless）
    ├── baidu_test.rs          # 百度渲染测试
    ├── web_test.rs            # 网页能力测试
    └── example.html           # 示例 HTML
```

## 渲染管线

```
HTML → kuchiki DOM ─→ css_engine（selectors 完整选择器）
                          ↓
                    TaffyLayoutEngine → taffy（Block/Flex/Grid）
                          ↓  （相对坐标 → 绝对坐标）
                    TaffyLayoutNode[]  ← hit_test(x,y) 事件冒泡
                          ↓
                    TaffyRenderer（通过 `render_with_taffy`）
    ├─ cosmic-text（font-size/color/family，跳过 style/script）
    ├─ border/box-shadow（tiny-skia）
    ├─ ImageCache（reqwest + image）
    └─ 长页面截图画布（自动扩展）
                          ↓
              tiny-skia Pixmap → PNG（完整页面）
```

## Feature 矩阵

| feature | 默认 | 说明 |
|---------|------|------|
| `boa` | ✅ | Boa JS 引擎（纯 Rust） |
| `js` | | obscura-js（V8/deno_core） |
| `gui` | ✅ | GUI 窗口（eframe/egui） |
| `headless` | | 无 GUI/JS/首页加载，纯渲染核心（隐含 `boa`） |

> `boa` 和 `js` 互斥，`headless` 不依赖任何 JS 引擎。

```bash
# 默认构建（Boa JS + GUI）
cargo build

# headless 模式（服务器端渲染/自动化测试）
cargo build --lib --no-default-features --features headless

# 运行 bridge DOM 测试（headless）
cargo run --example bridge_dom_test --no-default-features --features headless

# GUI 模式
cargo run

# 指定 URL
cargo run -- "https://www.baidu.com"
```

## WebNativeBridge API

供 web-to-native 工具产出的 Rust 代码直接调用。完整示例见 `examples/bridge_dom_test.rs`。

```rust
use rust_browser::bridge::WebNativeBridge;

let mut bridge = WebNativeBridge::new(1280, 720);

// ① 写入 DOM
bridge.set_html(r#"<div id="app"><button id="btn">点击</button></div>"#);

// ② 写入 CSS
bridge.set_css("#btn { background: blue; border-radius: 8px; }");
bridge.set_style("#btn", "color", "white");

// ③ 执行 JS（需要 boa/js feature）
bridge.eval_js("console.log('hello')");

// ④ 绑定事件 → Rust 回调
bridge.on_click("#btn", Box::new(|x, y| {
    println!("按钮点击: ({:.0}, {:.0})", x, y);
}));

// ⑤ 渲染 → PNG（长页面自动扩展）
let png: Vec<u8> = bridge.render();

// ⑥ 修改样式 → 重新渲染
bridge.set_style("#btn", "background", "red");
let png2 = bridge.render();

// ⑦ 获取元素位置
if let Some((x, y, w, h)) = bridge.get_rect("#btn") {
    println!("按钮位置: ({:.0},{:.0}) {:.0}x{:.0}", x, y, w, h);
}

// ⑧ 模拟点击（事件冒泡）
bridge.handle_click(100.0, 200.0);
```

### API 清单

| 类别 | 方法 | 说明 |
|------|------|------|
| DOM | `set_html`, `query`, `query_all`, `tag_name`, `get_attr`, `set_attr`, `text`, `query_text`, `parent_node` | |
| 布局 | `get_rect`, `all_rects`, `hit_test` | 绝对坐标 |
| CSS | `set_css`, `set_style`, `clear_css` | |
| JS | `eval_js` | 需 `boa`/`js` feature |
| 渲染 | `render` → `Vec<u8>` | 自动长页面 |
| 事件 | `on_click`, `on_form_submit`, `handle_click`, `handle_form_submit` | 冒泡机制 |
| 工具 | `set_viewport`, `viewport` | |

### 独立接口定义

`bridge.rs` 是**纯 trait 定义**，不依赖任何内部引擎类型。外部项目只需复制此文件，实现 `WebNativeBridge` trait 即可接入自己的渲染引擎。独立类型（`Color`、`LayoutRect`、`LayoutNode`、`Declaration`）均定义在 `bridge.rs` 内，无外部依赖。

内置 Mock 测试覆盖全部 API（23 项全部通过）：

```bash
cargo test --lib bridge::tests --no-default-features --features headless

## 依赖库

| 库 | 版本 | 用途 |
|---|------|------|
| taffy | 0.10（本地） | CSS 布局引擎 |
| tiny-skia | 0.12（本地） | 2D 渲染 |
| cosmic-text | 0.19（本地） | 文本排版 |
| kuchiki | 0.12（本地） | HTML 解析 |
| selectors | 0.27 | CSS 选择器 |
| cssparser | 0.35 | CSS 语法解析 |
| reqwest | 0.12 | HTTP 客户端 |
| image | 0.25 | 图片解码 |
| boa_engine | 0.21 | JS 引擎（纯 Rust） |
| eframe/egui | 0.34 | GUI 窗口 |

## 测试

```bash
# 1. Bridge Trait 接口测试（23项，不依赖渲染引擎）
cargo test --lib bridge::tests --no-default-features --features headless

# 2. 完整渲染测试（headless，输出 bridge_dom_test_output.png）
cargo run --example bridge_dom_test --no-default-features --features headless

# 3. 百度渲染测试
cargo run --example baidu_test

# 4. 网页加载测试
cargo run --example web_test
```

### bridge_dom_test 输出内容

`bridge_dom_test_output.png`（长页面截图，自动扩展高度）：
- 蓝色头栏（标题 + 副标题）
- 颜色色块（红/绿/蓝方块 + 橙/紫圆形，Flex 水平排列）
- 文本渲染测试（中英文混排 + 引用块）
- Flex 三栏布局（flex:1 / flex:2 / flex:1）
- 按钮 + 链接交互测试
- 深色页脚
- 事件冒泡验证（点击任意子元素冒泡到父容器）
- 控制台输出布局坐标和点击命中链

## 许可证

MIT
