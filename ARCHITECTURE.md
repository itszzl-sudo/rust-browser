# Rust Browser 架构文档

> 最后更新：2026-05-17（v2：新增 Boa JS 引擎双后端、profile.dev 配置、建筑式渲染美化）
>
> 本文档面向**项目参与者**，记录关键架构决策、模块职责、技术选型和未完成工作。

---

## 一、项目定位

Rust Browser 是一个用 Rust 从零构建的轻量级浏览器引擎，核心目标不是"用 Rust 重写 Chrome"，而是：

1. **Web → Native 转换工具**：配合一个将 Vite 打包产物翻译为 Rust 代码的工具，Rust Browser 提供渲染、事件捕获、输入处理的运行时能力，实现完整的 web-to-native 转换
2. **演示/实验**：验证 Rust 在浏览器引擎领域的能力

项目使用多进程架构（类似 Chrome），通过 Mojo IPC 通信。

---

## 二、多进程架构

```
┌─────────────────────────────────────────────────────┐
│                   主进程 (main.rs)                    │
│   eframe/egui Chrome 风格 UI（地址栏/标签/菜单）      │
│   ← 鼠标拖动选择 → ← 右键菜单 → ← 历史面板 →         │
│   ← 图片拷贝 → ← 下载页面 → ← 日志面板 →             │
├─────────────────────────────────────────────────────┤
│                 BrowserProcessHost                   │
│   多标签管理 → Mojo IPC → 渲染器线程                  │
│   ┌─────────────────────────────────────────────────┐│
│   │  Mojo IPC 三条管道 (per renderer)               ││
│   │  Navigation  (Browser→Renderer)                 ││
│   │  InputEvent  (Browser→Renderer)                 ││
│   │  RenderResult (Renderer→Browser)                ││
│   └─────────────────────────────────────────────────┘│
├─────────────────────────────────────────────────────┤
│              Renderer Process（线程，每个标签独立）    │
│                                                      │
│  ┌─────────┐  ┌──────────────┐  ┌────────────────┐  │
│  │ kuchiki │  │ selectors    │  │ obscura-js     │  │
│  │ DOM     │→ │ CSS Engine   │→ │ (V8/Deno)      │  │
│  └─────────┘  └──────┬───────┘  └────────────────┘  │
│                      ▼                               │
│  ┌──────────────────────────────────────────────┐    │
│  │         TaffyLayoutEngine                    │    │
│  │  Block / Inline / Flex / Grid / Position     │    │
│  └──────────────────┬───────────────────────────┘    │
│                     ▼                                 │
│  ┌──────────────────────────────────────────────┐    │
│  │           TaffyRenderer                      │    │
│  │  cosmic-text / tiny-skia / resvg / ImageCache │    │
│  └──────────────────┬───────────────────────────┘    │
│                     ▼                                 │
│  ┌──────────────────────────────────────────────┐    │
│  │         Painter → Pixmap → PNG               │    │
│  └──────────────────────────────────────────────┘    │
└─────────────────────────────────────────────────────┘
```

### IPC 通信协议（Mojo）

每条管道使用**二进制长度前缀编码**，避免文本分隔符冲突：

- 字符串: `[4字节小端长度][UTF-8字节]`
- u32: `[4字节小端]`
- f32: `[4字节小端]`

| 接口 | 方向 | 消息 |
|------|------|------|
| `Navigation` | Browser → Renderer | `Navigate { url, width, height }`、`Resize { width, height }` |
| `InputEvent` | Browser → Renderer | `MouseMove { x, y }`、`MouseClick { x, y, button }`、`KeyPress { key }`、`Scroll { delta_x, delta_y }` |
| `RenderResult` | Renderer → Browser | `FramePainted { png_data, width, height, title }` |

---

## 三、渲染管线

```
                              selectors 0.27（完整 CSS 选择器）
                                      ↓
HTML → kuchiki DOM → StyleMap (tag/class/id → CSS decls)
                          ↓
                  TaffyLayoutEngine
                          ↓
                  taffy 0.10 (Block / Inline / Flex / Grid / Position)
                          ↓
                    TaffyLayoutNode[]
    ├─ font_size / font_color / font_family → cosmic-text
    ├─ position_type / float_type           → taffy Position
    ├─ background_color / background_image  → ImageCache
    ├─ border / border-radius               → border.rs (tiny-skia PathBuilder)
    ├─ box-shadow                           → border.rs (3-pass box blur)
    ├─ line_height                          → cosmic-text Metrics
    └─ bg_position_x/y                     → ImageCache::crop_sprite()
                          ↓
                    TaffyRenderer
    ├─ render_text_at()           ← cosmic-text（支持 font-size/color/line-height）
    ├─ render_background_image()  ← ImageCache（网络图片 + sprite 裁剪）
    ├─ render_img_element()       ← ImageCache（<img> 标签）
    ├─ render_element_box()       ← border.rs + input/textarea 渲染
    ├─ render_box_shadow()        ← border.rs（3-pass box blur）
    ├─ text-decoration underline  ← painter line draw
    ├─ render_input_element()     ← 输入框 + 光标闪烁
    └─ SVG assets                 ← resvg 0.47
                          ↓
              Painter → tiny-skia Pixmap → PNG
```

### 渲染器 vs 浏览器进程协作

```
1. 用户输入 URL / 点击链接
2. BrowserProcess → Navigation IPC → RendererProcess
3. Renderer: HTML 下载 → 预扫描资源 → 并行下载 JS/CSS/图片
4. Renderer: DOM 解析（与 3 并行）
5. Renderer: 执行 JS（修改 DOM）
6. Renderer: 提取 CSS → rules_to_style_map → Taffy 布局
7. Renderer: TaffyRenderer 绘制 → PNG
8. Renderer → RenderResult IPC → BrowserProcess
9. BrowserProcess: PNG → egui ColorImage → 显示
```

---

## 四、模块职责

### `src/renderer/` — 渲染引擎

| 文件 | 职责 |
|------|------|
| `renderer.rs` | **主渲染器**。`Renderer` 管理 Painter + TextRenderer + TaffyLayoutEngine + 焦点/光标。`TaffyRenderer` 使用 Taffy 布局结果遍历 DOM 并绘制。`extract_style_tags()` 提取 `<style>` + `<link>` CSS |
| `taffy_layout.rs` | **Taffy 布局引擎桥接**。`TaffyLayoutEngine` 从 kuchiki NodeRef 构建 taffy 树，管理 `dom_to_layout` 映射，提供 `hit_test()`。`TaffyLayoutNode` 存储 layout 结果 + font-size/color/position/float/background/line-height |
| `border.rs` | CSS border 绘制（`PathBuilder` + `stroke_path`）+ box-shadow（3-pass box blur on RGBA） |
| `image_cache.rs` | 网络图片下载/缓存（`reqwest` + `image` + `HashMap`），CSS sprite 裁剪（`crop_sprite()`） |
| `svg.rs` | SVG 渲染（`resvg 0.47` + `usvg`） |
| `cursor.rs` | 光标系统（530ms 闪烁、左右移动、选区） |
| `painter.rs` | tiny-skia 绘制封装（fill_rect、draw_rect_border） |
| `layout.rs` | 旧版布局引擎（过渡用，已弃用） |
| `context.rs` | 渲染上下文（视口、缩放） |
| `text.rs` | 文本渲染器（旧版，cosmic-text 直接调用已在 renderer.rs 中） |

### `src/css_engine/` — CSS 引擎

| 文件 | 职责 |
|------|------|
| `mod.rs` | CSS 规则解析（`parse_css_rules`、`parse_inline_style`、`parse_length`），`@media` 块支持（screen/min-width/max-width） |
| `selector.rs` | **`selectors 0.27` crate 适配层**。实现 `SelectorImpl` 和 `Element` trait，将 kuchiki NodeRef 适配为 selectors 可用的 DOM 元素。支持标签/类/ID/后代/属性选择器和 `:hover`/`:focus`/`:active` 伪类 |

### `src/browser/` — 浏览器核心

| 文件 | 职责 |
|------|------|
| `engine.rs` | `BrowserEngine`。HTTP 请求 → Document 创建 → JsEngine 初始化 → JS 执行 → 渲染。`execute_js()` 真实调用 `obscura_js` |
| `mod.rs` | `Browser` 主类。多引擎管理 + 标签页 |
| `tabs.rs` | `TabManager`，浏览历史（前进/后退） |
| `ui.rs` | Chrome 风格 UI 绘制（tiny-skia 直接绘制，非 egui） |

### `src/bridge.rs` — Web → Native 桥接层

供 web-to-native 工具产出的 Rust 代码直接调用的统一 API：

| 类别 | 方法 |
|------|------|
| DOM 读写 | `set_html`、`query`、`query_all`、`tag_name`、`get_attr`、`set_attr`、`text`、`query_text`、`get_rect`、`all_rects`、`hit_test` |
| CSS 操作 | `set_css`、`set_style`、`clear_css` |
| JS 执行 | `eval_js`（需要 `--features js`） |
| 渲染 | `render` → `Vec<u8>` PNG |
| 事件绑定 | `on_click`、`on_form_submit`、`handle_click`、`handle_form_submit` |
| 工具 | `dom`、`dom_mut`、`renderer`、`layout`、`set_viewport`、`viewport` |

### `src/loader.rs` — 页面加载优化

`PageLoader`：HTML 下载后**预扫描**资源 URL（正则，O(n)，不构建 DOM）→ 启动**并行下载** JS/CSS/图片（`tokio::spawn`）→ 同时 DOM 解析 → 等待 JS/CSS → 执行 JS → 渲染。使首次显示时间从串行 5 秒降至并行 < 2 秒。

---

## 五、JS 引擎双后端

JsEngine 通过 feature 切换两个后端，对外暴露统一 API：

```rust
// 无论哪个后端，外部代码都这样用：
let mut engine = JsEngine::new();
engine.initialize("https://example.com").unwrap();
let result = engine.evaluate("1 + 2").unwrap();
```

### Boa 后端（`features = ["boa"]`，默认）

- **纯 Rust**，0 外部原生依赖，编译即可运行
- `Source::from_bytes()` 解析 JS，`Context::eval()` 执行
- 初始化时注入简化的 `document`、`window`、`console`、`Event` 等全局对象
- 没有真实 DOM 绑定，`dispatchEvent()` 为 no-op

### obscura-js 后端（`features = ["js"]`）

- 基于 **deno_core/V8**，需要 V8 编译环境和 `snapshot`
- 通过 `op_dom()` op 连接 `obscura_dom::DomTree`，**有真实 DOM 绑定**
- 支持 `document.getElementById()`、`querySelector()` 等完整 DOM API
- `dispatchEvent()` 真实触发节点事件

### feature 互斥

`boa` 和 `js` 互斥。如果都不启用，`JsEngine::evaluate()` 返回 `Err`。

---

## 六、关键依赖

| 库 | 版本 | 用途 | 来源 |
|----|------|------|------|
| `taffy` | 0.10（本地源码） | CSS 布局引擎（Block/Flex/Grid/Position） | 底层框架 |
| `tiny-skia` | 0.12（本地源码） | 2D 图形渲染 | 底层框架 |
| `cosmic-text` | 0.19（本地源码） | 文本排版与字形渲染 | 底层框架 |
| `kuchiki` | 0.12（本地源码，kuchikikiki） | HTML 解析（基于 html5ever） | 底层框架 |
| `selectors` | 0.27 | CSS 选择器匹配引擎（Mozilla Servo） | 开源组件（适配 kuchiki） |
| `cssparser` | 0.35 | CSS 语法解析 | 开源组件 |
| `obscura-js` | 本地 | JS 引擎（deno_core/V8），`features = ["js"]` | 开源组件 |
| `obscura-dom` | 本地 | DOM 树（与 kuchiki 不互通），仅 `features = ["js"]` 时使用 | 开源组件 |
| `boa_engine` | 0.21 | JS 引擎（纯 Rust，无外部依赖），`features = ["boa"]`（**默认**） | 开源组件 |
| `resvg` | 0.47 | SVG 渲染 | 开源组件 |
| `reqwest` | 0.12 | HTTP 客户端 | 底层框架 |
| `image` | 0.25 | 图片解码 | 底层框架 |
| `regex` | 1.11 | 预扫描 HTML 资源 URL | 开源组件 |
| `eframe/egui` | 0.34 | GUI 窗口（可选，默认带 gui feature） | 底层框架 |

### Feature 矩阵

```
default = ["boa", "gui"]
boa = ["boa_engine", "boa_gc"]  # Boa JS 引擎（纯 Rust，默认）
js  = ["obscura-js"]             # obscura-js（V8/deno_core）
gui = ["eframe", "egui"]         # GUI 窗口
```

| 命令 | JS 引擎 | GUI |
|------|---------|-----|
| `cargo build` | **Boa**（纯 Rust） | ✅ |
| `cargo build --no-default-features` | 无 | ❌ |
| `cargo build --features js` | **V8**（obscura-js） | ❌ |
| `cargo build --features boa,gui` | Boa | ✅ |
| `cargo build --features js,gui` | V8 | ✅ |

> `boa` 和 `js` 互斥，不能同时启用。

### 版本冲突注意

Windows 上 `eframe 0.34` → `wgpu` → `wgpu-hal` 与 `windows` crate 版本存在冲突。编译 lib 时用 `--no-default-features` 跳过 GUI 即可。被捕获的错误全部来自 `wgpu-hal` 的传递依赖，不影响 rust-browser 源码。

---

## 六、技术选型 vs 自研代码量

| 功能 | 方案 | 代码来源 | 代码量 |
|------|------|---------|--------|
| CSS 布局引擎 | **taffy 0.10** | 底层框架 | — |
| CSS 选择器匹配 | **selectors 0.27**（Mozilla Servo） | 开源组件（本项目适配 kuchiki） | ~650 行适配 |
| CSS 值解析 | **cssparser 0.35** + 自研补充 | 混合 | ~100 行 |
| 2D 渲染 | **tiny-skia 0.12** | 底层框架 | — |
| 文本排版 | **cosmic-text 0.19** | 底层框架 | — |
| SVG 渲染 | **resvg 0.47** | 开源组件 | ~220 行封装 |
| HTML 解析 | **kuchiki 0.12** | 底层框架 | — |
| HTTP 网络 | **reqwest 0.12** | 底层框架 | — |
| 图片解码 | **image 0.25** | 底层框架 | — |
| JS 引擎（默认） | **Boa Engine 0.21**（纯 Rust） | 开源组件 | ~260 行封装 |
| JS 引擎（V8） | **obscura-js**（deno_core/V8） | 开源组件 | ~60 行封装 |
| 图片网络加载 + 缓存 | 自研 | reqwest + image + HashMap | ~360 行 |
| CSS sprite 裁剪 | 自研 | image::crop_imm + Pixmap::clone_rect | ~20 行 |
| border 绘制 + 圆角 | 自研 | tiny-skia PathBuilder + stroke_path | ~180 行 |
| box-shadow | 自研 | 3-pass box blur on RGBA | ~120 行 |
| font-size/color/family/line-height | 自研桥接 | CSS → cosmic-text Mapping | ~80 行 |
| text-decoration | 自研 | 基线绘制 | ~20 行 |
| inline/float/position | 自研桥接 | CSS → taffy Mapping | ~80 行 |
| 输入 + 光标 + 焦点 | 自研 | focus_by_click + cursor blinking + KeyPress | ~300 行 |
| 预扫描 + 并行下载 | 自研 | regex prescan + tokio::spawn | ~260 行 |
| `@media` 查询 | 自研 | CSS 解析层 | ~80 行 |
| WebNativeBridge | 自研 | 统一 API 层 | ~400 行 |

---

## 七、两套 DOM 并存问题

这是一个**已知架构缺陷**：

| DOM | 用于 | 基于 |
|-----|------|------|
| `DomWrapper`（kuchiki） | 渲染管线（布局、样式计算、事件 hit test） | `kuchiki::NodeRef` |
| `obscura_dom::DomTree` | JS 引擎（`JsEngine.set_dom()`） | `html5ever` 独立树 |

**两者不同步**。JS 执行 `document.body.innerHTML = '...'` 修改的是 `obscura_dom`，kuchiki 不知道。反过来 Rust 侧通过 `DomWrapper::set_attribute()` 修改 DOM，`obscura_js` 也不知道。

**当前缓解措施**：每次 `set_html()` 或 `render()` 时重新解析 HTML 到 kuchiki，同时重新初始化 `JsEngine` 并再次执行所有 `<script>`，或调用 `eval_js` 传入必要的 JS 代码。

**长期方案**：统一到一套 DOM，或实现双向同步层。

---

## 八、输入系统架构

```
用户按键 'A'
  │
  ▼
egui → ctx.input().events → BrowserApp::update()
  │
  ▼ 关键映射
Key::A + 无 Shift → "a"
Key::A + Shift    → "A"
Key::Backspace    → "Backspace"
Key::Enter        → "Enter"
Key::Space        → " "
  │
  ▼
host.send_input(InputEvent::KeyPress { key })
  │  Mojo IPC
  ▼
run_renderer_process() 处理 KeyPress
  │
  ├── ① 检查 renderer.focused_node
  │
  ├── ② 读取 DOM 属性
  │     let value = dom.attribute(focused, "value")
  │
  ├── ③ 修改 DOM
  │     match key:
  │       "Backspace" → value.pop()
  │       "Enter"     → if textarea: value.push('\n')
  │                      else: 触发表单提交
  │       _           → value.push_str(&key)
  │     dom.set_attribute(focused, "value", &value)
  │
  ├── ④ 触发 JS input 事件
  │     js_engine.evaluate("el.dispatchEvent(new Event('input'))")
  │
  ├── ⑤ 重新渲染 → 新 PNG
  │
  ▼
BrowserProcess 收到 RenderResult → egui 显示
```

### 焦点系统

| 事件 | 行为 |
|------|------|
| 点击 `<input>` | `focus_by_click()` → 设 `focused_node` → 光标可见 → `:focus` CSS 伪类 |
| 点击 `<textarea>` | 同上 |
| 点击其他元素 | `focused_node = None`，光标消失 |
| 有焦点时按键 | KeyPress 处理 → 修改 value → 重渲染 |

### 光标系统（`cursor.rs`）

- `CursorRenderer`：`position.offset`（字符偏移）、`blink_timer`（530ms 闪烁）、`visible`（是否绘制竖线）
- 每帧调用 `update(dt)` 更新闪烁状态
- `render_input_element()` 在焦点元素文字末尾绘制 2px 黑色竖线

### 限制

- **不支持中文 IME**：Windows IME 输入中文时产生多个 KeyPress 事件（组合字符），egui 不会合并它们。需要 IME 专用通道（`WM_IME_COMPOSITION`），这是 eframe 不暴露的 Windows 平台特性，预估 2-3 天纯 Windows 代码
- **选区（Selection）**：`TextSelection` 结构体已定义，`CursorRenderer` 已预留 `selection` 字段，但键盘交互（Shift+方向键扩展选区）和选区高亮渲染未实现，预估 1 天
- **`<input type="password">`**：当前显示明文，未做密码圆点
- **占位符（placeholder）**：当前不显示

---

## 九、`<input>` / `<textarea>` 渲染

渲染在 `TaffyRenderer::render_element_box()` 中实现：

```
"input" →
    - 白色背景 fill_rect(x, y, w, h, #FFFFFF)
    - 灰色 1px 边框 (border)
    - 读取 value 属性 → left + font_size + color 渲染
    - 如果有焦点且 cursor.visible → 在文字末尾画 2px 竖线 (#333333)
    - 行内元素 padding (2, 1)
    - 尺寸来自 taffy 布局或 CSS width/height

"textarea" →
    - 同上
    - 文本使用 text_content_recursive（支持换行）
    - 多行文本渲染
```

---

## 十、布局引擎细节

### TaffyLayoutNode 字段

```rust
pub struct TaffyLayoutNode {
    pub node: NodeId,                    // taffy 内部节点 ID
    pub dom_node: usize,                 // DomWrapper 中的索引
    pub tag_name: String,
    pub x: f32, pub y: f32,             // 绝对坐标
    pub width: f32, pub height: f32,     // 尺寸
    pub background: Option<Color>,
    pub color: Option<Color>,
    pub depth: usize,
    pub font_size: f32,
    pub font_color: Option<Color>,
    pub font_family: Vec<String>,        // 备选字体链
    pub position_type: PositionType,     // Static/Relative/Absolute/Fixed
    pub float_type: FloatType,           // None/Left/Right
    pub background_image: Option<String>,
    pub line_height: f32,
    pub bg_position_x: f32,
    pub bg_position_y: f32,
}
```

### 样式确定顺序

在 `build_from_noderef()` 中：

1. 从 `style_map` 获取 tag 匹配的 CSS 声明（来自 `<style>` 标签和 `<link>`）
2. 从 `style_map` 获取 class/id 匹配的 CSS 声明
3. 从 `style_map` 获取 `:hover`/`:focus` 伪类声明（如果当前节点匹配）
4. 解析元素内联 `style` 属性
5. 合并：内联 style 覆盖 style_map，style_map 覆盖标签默认值
6. 传入 `determine_style()` 生成 taffy `Style`

---

## 十一、CSS 引擎细节

### `css_engine/selector.rs`

使用 `selectors 0.27` crate。关键适配：

- `KuchikiSelectorImpl`：实现 `SelectorImpl` trait，定义 DOM 元素如何表达标签名、类名、ID、属性
- `KuchikiElement`：实现 `selectors::Element` trait，包装 `kuchiki::NodeRef`，提供父元素、兄弟元素、子元素遍历
- `KuchikiSelectorParser`：实现 `selectors::Parser` trait，解析 CSS 选择器字符串
- `rules_to_style_map_with_selectors()`：遍历文档所有元素，对每个 CSS 规则检查选择器匹配，将匹配的声明按 tag_name 分组存入 `StyleMap`
- `:hover` / `:focus` / `:active` 伪类注册在 `KuchikiNonTSPseudoClass` 中

### `css_engine/mod.rs`

- `parse_css_rules()`：手动 CSS 解析（因 `cssparser` 是 tokenizer，上层还需要自建）
- 支持 `@media screen and (min/max-width: Xpx)` 条件判断
- `Declaration`：`{ property: String, value: String }`
- `StyleMap`：`HashMap<String, Vec<Declaration>>`（tag_name → 声明列表）

---

## 十二、PageLoader 加速方案

### 优化前后对比

```
优化前（串行 5 秒）：
  HTML 下载 → DOM 解析 → 逐个下载 JS/CSS → 执行 JS → 渲染
  ├── 1.5s ──┤── 0.3s ──┤───── 2.5s ─────┤── 0.5s ─┤── 0.2s ─┤

优化后（并行 < 2 秒）：
  HTML 下载
    │
    ├── ① 预扫描（0.01s）→ ② 并行下载 JS/CSS/图片（tokio::spawn）
    │                                          │
    └── ③ DOM 解析（0.3s）────────────────────┤
                                                │
                                    ④ 等待下载完成 → ⑤ 执行 JS → ⑥ 渲染
                                    ├── 并行 ──┤── 0.5s ─┤── 0.2s ─┤
```

### 策略

1. **预扫描**：`PageLoader::prescan()` 用 `regex::Regex` 扫描 HTML 提取 `<script src>`、`<link rel="stylesheet">`、`<img src>`、`<link rel="preload">` 的 URL。O(n) 时间，n = HTML 长度，不构建 DOM
2. **并行下载**：`tokio::spawn` 并发下载所有资源，每个写入 `ResourceCache`（线程安全，`Arc<Mutex<HashMap>>`）
3. **去重**：`ResourceCache.inflight` 防止同一 URL 重复下载
4. **超时控制**：`wait_for_all(timeout_ms)` 防止单个资源拖垮整个页面

---

## 十三、WebNativeBridge 使用示例

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

---

## 十四、已知问题与限制

### 布局引擎限制（taffy 0.10 版本上限）

| 缺失能力 | 原因 | 影响 |
|---------|------|------|
| `order` | taffy 0.10 `Style` 无此字段 | Flex 子项无法重排序 |
| `overflow: hidden/scroll` | taffy 0.10 无对应类型 | 内容溢出无法裁剪 |
| `white-space: nowrap` | 同上 | 文本无法禁止换行 |
| `position: fixed/sticky` | taffy 0.10 `Position` 枚举缺少对应变体 | 固定/粘性定位不可用 |
| `gap` 在 grid 布局中 | taffy 0.10 grid gap 支持不完整 | Grid 间距可能异常 |

### 已支持但可能不完整的布局能力

| 能力 | 状态 | 说明 |
|------|------|------|
| `font-weight` | ✅ 完整 | bold/normal/100-900 → cosmic-text `Weight` |
| `box-sizing` | ✅ 完整 | border-box / content-box |
| `position: relative/absolute` | ✅ 完整 | 配合 `top/right/bottom/left` |
| `align-self` | ✅ 完整 | 单子项对齐 |
| `flex` 简写 | ✅ 完整 | `flex: N` → grow + basis:0% |
| `gap`（flex） | ✅ 完整 | px / % |
| `min-width/height` / `max-width/height` | ✅ 完整 | |
| `border-width`（统一四边）| ✅ | `border-width` 和 `border` 简写中提取宽度 |

### 布局引擎缺失（CSS → 布局/渲染链路不完整）

| 缺失能力 | 说明 | 链路断裂点 |
|---------|------|-----------|
| `border-radius` | painter 已实现绘制，但 `taffy_layout` 未解析 CSS 值 | 布局层未将 radius 传递给渲染器 |
| `border-color` / `border-style` | 仅支持 border-width，颜色和样式未独立解析 | `determine_style` 未解析 |
| `box-shadow` | painter 已实现，但仅通过 `StyleMap` 间接传递 | 未在 `determine_style` 中直接解析 |
| `background-size` / `background-repeat` | 背景图片已加载，但尺寸和重复模式未处理 | 图片渲染未控制缩放/平铺 |
| `text-decoration` | 完全未实现 | 渲染器无装饰线绘制 |
| `text-transform` | 完全未实现 | 无大小写转换 |
| `letter-spacing` / `word-spacing` | 完全未实现 | cosmic-text `Attrs` 未设置 |
| `outline` | 完全未实现 | 无轮廓线绘制 |
| `list-style` | 完全未实现 | 无 bullet 渲染 |
| `opacity` | 完全未实现 | 无透明度合成 |
| `cursor` | 完全未实现 | 无鼠标样式变化 |
| `white-space` | 已解析但不生效 | taffy 0.10 无对应类型 |
| `overflow` | 已注释掉 | taffy 0.10 无对应类型 |

### 测试相关

```
browser_process::interfaces::tests::test_render_result_message_roundtrip
  → IPC 二进制编码 test 与当前实现不一致，不影响运行时

storage::local_storage::tests::test_remove_item
storage::local_storage::tests::test_set_and_get_item
storage::local_storage::tests::test_used_bytes_tracking
  → localStorage 实现有 bug，已弃用 storage 模块
```

### JS 引擎限制

| 限制 | 说明 |
|------|------|
| Boa `fetch()` 同步阻塞 | 使用 `block_on` 执行异步请求，会阻塞 JS 引擎线程 |
| BOM/DOM API 不完整 | 仅注入最小 polyfill（console/document/location/navigator/Event） |
| `fetch()` options | 仅支持 method 参数，不支持 body/headers 等 |
| `XMLHttpRequest` | 未实现 |

### 多进程架构限制

| 限制 | 说明 |
|------|------|
| 渲染器线程首次导航与 IPC 导航重复 | 渲染器启动时自动加载 URL，主线程 navigate() 会发送第二条 IPC 导致重复加载（已修复：首帧不调 navigate） |
| Mojo IPC 单线程 | IPC 管道基于 `Mutex<VecDeque>`，非高性能共享内存 |
| 渲染器进程内存隔离 | 当前使用线程而非独立进程，无沙箱隔离 |

### 其他已知问题

| 问题 | 说明 |
|------|------|
| eframe/egui GUI 编译慢 | wgpu/naga/Vulkan 后端编译耗时较长 |
| 增量编译链接错误 | Windows 下偶发 LNK 链接失败，`cargo clean` 可解决 |
| `env_logger` 需手动初始化 | example 中未调用 `env_logger::init()`，`log::info!` 不可见 |
| Windows 字体回退 | `mstmc.ttf` 无法加载（非致命，仅 warning） |
| 长截图视口恢复 | 长页面渲染后 `pixmap` 恢复原始大小，仅供截图用途 |

---

## 十五、未完成工作

### 短期（<= 3 天）

| 工作 | 说明 | 预估 |
|------|------|------|
| `<input type="password">` | 密码圆点字符 | 0.3d |
| `<input placeholder>` | 占位符文本 | 0.3d |
| `<li>` 列表数字/字母 | list-style-type: decimal/alpha | 1d |
| `border-radius` CSS 解析 | 从 CSS 提取 border-radius 传入 painter | 0.5d |
| `border-color` / `border-style` | 边框颜色和样式独立于宽度控制 | 0.5d |
| `box-shadow` CSS 解析 | 在 `determine_style` 中直接解析 box-shadow | 0.3d |
| `background-size` / `background-repeat` | 控制背景图片缩放和平铺 | 0.5d |
| `text-decoration` | 下划线/删除线渲染 | 0.5d |

### 中期（3-10 天）

| 工作 | 说明 | 预估 |
|------|------|------|
| 中文 IME 输入 | Windows 原生消息处理 | 2-3d |
| `<iframe>` | 子渲染器进程 | 2d |
| CSS `::before`/`::after` | 伪元素渲染 | 1d |
| 表格 `<colspan>`/`<rowspan>` | 表格合并单元格 | 1d |
| `overflow: hidden/scroll` | 内容裁剪 + 滚动 | 1d |
| `z-index` | 堆叠上下文 | 0.5d |

### 长期（> 10 天）

| 工作 | 说明 |
|------|------|
| CSS 动画/过渡 | animation/transition |
| `<video>` / `<audio>` | 媒体标签 |
| WebSocket | 全双工通信 |
| DevTools | 开发者工具 |
| Service Worker / PWA | 离线能力 |
| WebGL / Canvas 2D | 图形 API |
| 两套 DOM 统一 | kuchiki ↔ obscura_dom 同步 |

---

## 十五、构建与测试

```bash
# 默认构建（Boa JS + GUI）
cargo build

# 纯 lib 构建（不依赖 GUI，避开 Windows wgpu 问题）
cargo check -p rust-browser --lib --no-default-features

# 带 V8 JS 引擎
cargo check -p rust-browser --lib --features js

# 调试构建（opt-level = 1，平衡编译速度和运行性能）
cargo build --profile dev

# 测试
cargo test -p rust-browser --lib --no-default-features -- image_cache border svg taffy renderer text css_engine bridge loader

# 全量测试（107/111 通过，4 个已知失败）
cargo test -p rust-browser --lib --no-default-features
```

### 已知测试失败

```
browser_process::interfaces::tests::test_render_result_message_roundtrip
  → IPC 二进制编码 test 与当前实现不一致，不影响运行时

storage::local_storage::tests::test_remove_item
storage::local_storage::tests::test_set_and_get_item
storage::local_storage::tests::test_used_bytes_tracking
  → localStorage 实现有 bug，与渲染引擎无关
```

---

## 十六、提交历史

```
42fc312  输入能力完整实现: 焦点+光标+input/textarea+KeyPress全链路
83a2a20  PageLoader: 预扫描+并行下载加速页面加载
1544996  loading进度条 + hover伪类 + @media查询 + font-family备选链
6523241  超长截图 + 多标签页UI→IPC + box-shadow从style解析 + background-position管线集成
d5d0bcc  line-height CSS属性支持 + README更新
40b81af  布局引擎完整版 + CSS选择器(selectors) + border/box-shadow + 图片缓存/网络加载 + resvg SVG + JS引擎接入 + hit testing + 事件IPC + 外部CSS加载 + WebNativeBridge
921afab  多进程ipc，多线程queue
81f778f  多进程ipc，多线程queue
7e59f53  精美渲染效果: 圆角输入框/按钮/标题装饰/渐变分割线
bebff6b  feature隔离: Boa JS引擎(默认)+obscura-js(V8)双后端, profile.dev配置
```
