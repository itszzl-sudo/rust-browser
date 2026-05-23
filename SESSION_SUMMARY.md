# Rust Browser — 项目状态盘存

> 生成时间：2026-06-01
> 最后更新：本会话（console.log / 页面滚动 / iframe / canvas 2D / CSS position:sticky+fixed / CSS opacity+transform / 进程 IPC 管道层 / MessageName 重构 / 真实子进程启动骨架 / table 表格布局 / select 下拉框 / DevTools 面板 / WindowManager 修复）
> 用途：当前项目完整状态记录，供下一轮会话直接接手

---

## 一、当前项目状态

| 指标 | 数值 |
|------|------|
| **编译** | ✅ 所有 5 个 feature 组合零错误零警告 |
| **测试** | ✅ **309 / 309 通过** |
| **示例编译** | ✅ 3/3 通过 |
| **源文件** | 34 个 |
| **行数** | ~15,000+ 行 |

### Feature 编译验证

| 命令 | 结果 |
|------|------|
| `cargo check --features boa` | ✅ 0 error, 0 warning |
| `cargo check --lib --features headless` | ✅ 0 error, 0 warning |
| `cargo check --features gui,boa` | ✅ 0 error, 0 warning |
| `cargo check --features embed` | ✅ 0 error, 0 warning |
| `cargo check --all-targets --features boa` | ✅ 0 error, 0 warning |

---

## 二、已完成的所有工作

### 阶段 1：Arc 浏览器等价变换 — 架构增强

| # | 模块 | 文件 | 说明 |
|---|------|------|------|
| 1 | 合成层 | `src/compositor.rs` | UI 层与 Web 层独立 RGBA、alpha 混合合成 |
| 2 | 页面冻结 | `src/page_state.rs` | 4 级冻结（Active→LightFreeze→MediumFreeze→DeepFreeze），按非活跃时间自动降级 |
| 3 | 资源调度 | `src/resource_scheduler.rs` | 4 级优先级（Critical→Low），LRU 缓存（256 条目） |
| 4 | 异步管线 | `src/renderer/async_pipeline.rs` | 首屏阻塞渲染 + 后台工作线程 CSS 解析 |
| 5 | JS 分级调度 | `src/js_task_scheduler.rs` | 5 类 JS 分级、DOM 读写分离合并、属性自动映射 |
| 6 | 布局缓存 | `src/renderer/layout_cache.rs` | 指纹哈希 + DOM 版本号 + 子树跳过 |
| 7 | 图片管道 | `src/renderer/image_pipeline.rs` | 元数据预读 + 零阻塞占位布局 |

### 阶段 2：GUI 重构 — 从 GDI 到 winit + softbuffer

| # | 变更 | 说明 |
|---|------|------|
| 1 | **窗口管理** | 从 `CreateWindowExA` + `WNDCLASSA` 迁移到 `winit 0.30` + `ApplicationHandler` trait |
| 2 | **像素推送** | 从 GDI `CreateDIBSection` + `BitBlt` 迁移到 `softbuffer 0.4` + `Surface::buffer_mut()` |
| 3 | **窗口联动** | 主窗口 `WM_MOVE`/`WM_SIZE` → 工具条窗口同步位置/宽度 |
| 4 | **两窗口独立** | eframe 浮动工具条（80px, 无边框）+ winit 主窗口（页面内容），不再互相包住 |
| 5 | **跨平台** | 移除 `windows-sys` 依赖限制，winit + softbuffer 原生支持 Windows/Linux/macOS |

### 阶段 3：P0 — 致命缺失修复

| # | 特性 | 文件 | 说明 |
|---|------|------|------|
| 1 | **JS DOM 双向绑定** | `js_dom_bridge.rs`, `js_engine.rs` | 33 个 Boa 原生 DOM 函数：getElementById/querySelector/innerHTML/appendChild/setAttribute 等，修改真实 kuchiki DOM 树，脏节点追踪 |
| 2 | **JS DOM 修改 → 渲染同步** | `renderer.rs`, `bridge_impl.rs` | `JsDomBridge.has_pending_changes` → 增量重布局 + 重绘 |
| 3 | **setTimeout/setInterval 真实异步** | `js_timer_queue.rs`, `host.rs` | 基于 `Instant` 的定时器队列，每帧 tick，支持循环 interval，通过 GLOBAL_TIMER_QUEUE 全局调度 |

### 阶段 4：P1 — 高优先级修复

| # | 特性 | 文件 | 说明 |
|---|------|------|------|
| 1 | **localStorage/sessionStorage** | `web_storage.rs`, `js_engine.rs` | 按 origin 分域的存储引擎，6 个原生函数注册到 Boa |
| 2 | **document.cookie JS 桥接** | `web_storage.rs`, `network.rs` | 通过 `GLOBAL_COOKIE_JAR` 实现 JS 读写 Cookie，过滤 HttpOnly |
| 3 | **XMLHttpRequest** | `js_engine.rs` | 完整 polyfill + `nativeXhrRequest` 原生函数（reqwest 执行） |
| 4 | **CORS 跨域检查** | `cors.rs` | `CorsChecker`：简单请求、凭据请求、通配符/显式 origin 匹配，7 个测试 |
| 5 | **CSP 策略引擎** | `csp.rs` | 10 种指令支持、report-uri 违规上报、report-only 模式、6 个测试 |
| 6 | **HTTP 缓存层** | `http_cache.rs` | RFC 7234：Cache-Control/Expires/ETag/Last-Modified、304 条件请求刷新、LRU 淘汰、9 个测试 |

### 阶段 5：P2 — 中优先级 + 杂项修复

| # | 特性 | 文件 | 说明 |
|---|------|------|------|
| 1 | **CSS float: left/right** | `taffy_layout.rs` | `apply_float_layout()` 后处理偏移 |
| 2 | **CSS z-index 层叠顺序** | `taffy_layout.rs`, `renderer.rs` | `z_index` 字段 + 子节点排序渲染 |
| 3 | **Favicon 提取加载** | `engine.rs` | `<link rel="icon">` / `apple-touch-icon` 提取 |
| 4 | **书签管理增删改** | `main.rs` | `add_bookmark`/`remove_bookmark`/`edit_bookmark` + 编辑对话框 |
| 5 | **标签页固定 Pin UI** | `main.rs` | `pinned_tabs` 集合 + 📌/📍按钮切换 |
| 6 | **文本测量精确化** | `text.rs` | `measure_text` 标记 deprecated，新增 `measure_text_exact` 使用 cosmic-text |
| 7 | **媒体查询增强** | `css_engine/mod.rs` | 新增 `max-height`/`min-height`/`prefers-color-scheme`/`prefers-reduced-motion` |
| 8 | **innerHTML 递归序列化** | `js_dom_bridge.rs` | `serialize_node()` 递归序列化 + `outerHTML` |
| 9 | **HTTP 方法扩展** | `network.rs` | 新增 PUT/DELETE/PATCH/HEAD |
| 10 | **HTTP 日期解析** | `network.rs` | 使用 `httpdate` crate 替换手动 80+ 行解析 |
| 11 | **CSP report-uri 实现** | `csp.rs` | 违规报告 POST 发送 + report-only 模式 |
| 12 | **RepeatingTask 基础实现** | `task_queue/task.rs` | 后台线程 + loop sleep + 取消令牌 |
| 13 | **废弃 LayoutEngine** | `layout.rs` | 标记 `#[deprecated]`（已被 TaffyLayoutEngine 替代） |
| 14 | **font-style 解析** | `taffy_layout.rs` | 添加 `font_style` 字段 + `determine_font_style()` |

### 阶段 6：文档替换

| # | 操作 | 说明 |
|---|------|------|
| 1 | 删除 6 个旧文档 | `ARCHITECTURE.md`, `FEATURE_DESIGN.md`, `浏览器渲染并行优化完整方案.md`, `arc浏览器.txt`, `完整数据流图`, `渲染管线总览` |
| 2 | 创建 2 个新文档 | `README.md`（特性表 + 快速开始 + 项目结构），`ARCHITECTURE.md`（13 章完整架构） |

---

## 三、核心架构图

```
┌──────────────────────────────────────────────────┐
│  eframe 浮动工具栏窗口 (80px, 无边框, 始终可见)   │ ← egui 渲染 UI 条
│  标签栏 + 导航栏 + 书签栏 + 加载进度条            │
├──────────────────────────────────────────────────┤
│  winit 主窗口 (页面内容)                          │ ← softbuffer 像素推送
│  tiny-skia Pixmap → RGBA → softbuffer Buffer     │   跨平台 blit
│  → buffer.present() → 屏幕                       │
├──────────────────────────────────────────────────┤
│  状态栏窗口 (屏幕底部, 任务栏上方, 28px)           │ ← 独立窗口
└──────────────────────────────────────────────────┘

窗口联动: 主窗口 WM_MOVE → 工具条同步紧贴上方
        主窗口 WM_SIZE → 工具条同步宽度
        关闭主窗口 → 退出
```

## 四、关键文件映射

### 入口文件
| 文件 | 职责 |
|------|------|
| `src/main.rs` | GUI 入口（eframe 工具条）+ 窗口管理器创建/联动 |
| `src/lib.rs` | 所有模块注册 + `pub use` 导出 |

### 窗口管理
| 文件 | 职责 |
|------|------|
| `src/gui_window.rs` | `WindowManager`：winit 事件循环 + softbuffer 像素推送 + `ApplicationHandler` |

### 浏览器核心
| 文件 | 职责 |
|------|------|
| `src/browser/mod.rs` | `Browser` 主类 |
| `src/browser/engine.rs` | `BrowserEngine`：导航/DOM/JS/favicon |
| `src/browser/tabs.rs` | `TabManager`：标签页/历史/前进后退 |
| `src/browser_process/host.rs` | `BrowserProcessHost` + `run_renderer_process` 消息循环 |

### 渲染引擎
| 文件 | 职责 |
|------|------|
| `src/renderer/renderer.rs` | `Renderer` + `TaffyRenderer`：DOM→布局→绘制 |
| `src/renderer/taffy_layout.rs` | `TaffyLayoutEngine`：CSS→taffy→绝对坐标+float+z-index |
| `src/renderer/painter.rs` | `Painter`：tiny-skia 2D 绘制 |
| `src/renderer/text.rs` | `TextRenderer`：cosmic-text 排版 |

### JS 引擎
| 文件 | 职责 |
|------|------|
| `src/js_engine.rs` | `JsEngine`（Boa/V8 双后端）+ polyfill + 33 原生函数 |
| `src/js_dom_bridge.rs` | `JsDomBridge`：DOM 操作→真实 kuchiki DOM + 脏节点 |
| `src/js_timer_queue.rs` | `TimerQueue`：真实异步定时器 |
| `src/js_task_scheduler.rs` | `JsTaskScheduler`：JS 分级调度 |

### 桥接/网络/安全
| 文件 | 职责 |
|------|------|
| `src/bridge.rs` | `WebNativeBridge` trait 定义 |
| `src/bridge_impl.rs` | `DefaultWebNativeBridge` 实现 |
| `src/network.rs` | `NetworkClient`：HTTP + Cookie RFC 6265 |
| `src/http_cache.rs` | `HttpCache`：Cache-Control/ETag/304 |
| `src/csp.rs` | `CspManager`：CSP 策略引擎 |
| `src/cors.rs` | `CorsChecker`：CORS 跨域检查 |
| `src/web_storage.rs` | localStorage/sessionStorage/cookie JS 桥接 |

### 调度/缓存
| 文件 | 职责 |
|------|------|
| `src/task_queue/scheduler.rs` | `TaskScheduler`：前台/后台线程池 |
| `src/task_queue/thread_pool.rs` | Work-Stealing 线程池 |
| `src/resource_scheduler.rs` | 4 级资源优先级调度 |
| `src/page_state.rs` | 4 级页面冻结 |
| `src/renderer/layout_cache.rs` | 布局缓存 + 指纹 |
| `src/renderer/async_pipeline.rs` | 异步渲染管线 |

---

## 五、Cargo.toml 关键配置

### Features
```toml
[features]
default = ["browser", "boa"]
browser = ["gui"]
embed = []
boa = ["boa_engine", "boa_gc"]
v8 = ["deno_core"]
gui = ["eframe", "egui", "winit", "softbuffer", "raw-window-handle"]
headless = ["embed"]
```

### 核心依赖
```
winit = "0.30"          # 跨平台窗口管理（rwh_06 feature）
softbuffer = "0.4"      # 软件像素推送
eframe = "0.34"         # egui 框架（工具条 GUI）
egui = "0.34"
tiny-skia = { path = "../tiny-skia-0.12.0" }  # 本地 2D 渲染
taffy = { path = "../taffy-0.10.0" }          # 本地 CSS 布局
cosmic-text = { path = "../cosmic-text-0.19.0" }
kuchiki = { path = "../kuchikikiki-0.12.0" }
boa_engine = "0.21"     # 纯 Rust JS 引擎
reqwest = "0.12"        # HTTP 客户端
httpdate = "1.0"        # HTTP 日期解析
crossbeam-deque = "0.8" # Work-Stealing 线程池
```

---

## 六、下一轮会话可继续的方向

### 🔴 P0（紧急）
- ~~<iframe> 支持（嵌入页面渲染）~~ ✅ **已完成**
- ~~<canvas> 2D 上下文（Web 图形应用依赖）~~ ✅ **已完成**
- ~~console.log 从空函数改为真实输出到日志面板~~ ✅ **已完成**

### 🟠 P1（高优先级）  
- ~~页面滚动（winit 窗口滚动事件 → 渲染偏移）~~ ✅ **已完成**
- 将线程模拟的多进程改为真实进程隔离
- ~~CSS position: sticky / position: fixed 完整实现~~ ✅ **已完成**
- ~~CSS opacity / transform 支持~~ ✅ **已完成（opacity + transform translate）**

### 🟡 P2（中优先级）
- ~~<table> 表格布局引擎~~ ✅ **已完成（Grid 模拟 table/tr/td/th/thead/tbody）**
- ~~<select> / <option> 下拉框~~ ✅ **已完成（按钮样式+三角箭头+选中项文本+JS交互）**
- HTML 表单自动提交
- Service Worker 骨架
- WebSocket 支持

### 🟢 P3（低优先级）
- ~~DevTools DOM 检查器／控制台 Console~~ ✅ **已完成**
- ~~DevTools 系统信息面板~~ ✅ **已完成**
- 下载管理器（进度/历史）
- 密码管理器/自动填充
- 扩展/插件系统
- 设置页面（字体/缩放/代理）
