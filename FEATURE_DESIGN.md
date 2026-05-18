# rust-browser Feature 设计

## 用途分类

### 1. 嵌入模式 (embed)
给 javascript-web-to-rust-native 项目使用：
- 无 GUI
- WebNativeBridge API
- 事件系统（点击、表单、window.open）
- 网络请求（HTTP GET/POST）
- 文件操作（下载、读写）
- 渲染到内存（PNG 字节）
- JS 引擎可选

### 2. 浏览器模式 (browser)
正常浏览器使用：
- GUI 窗口（egui）
- 多标签页
- 地址栏、导航按钮
- JS 引擎必需
- 完整浏览器功能

## JS 引擎（三种，互斥）

| 引擎 | Feature | 说明 | 适用场景 |
|------|---------|------|----------|
| Boa | `boa` | 纯 Rust，无需外部依赖 | 嵌入模式默认，跨平台 |
| QuickJS | `quickjs` | 轻量级 C 引擎，启动快 | 嵌入模式，嵌入式设备 |
| V8 | `v8` | obscura-js (Deno) | 浏览器模式，完整 JS |

## Feature 矩阵

```toml
[features]
default = ["browser", "boa"]

# ── 用途模式（互斥） ──

# 嵌入模式：给 jrust-runtime 使用
# - 无 GUI
# - WebNativeBridge API
# - JS 引擎可选
embed = []

# 浏览器模式：正常浏览器
# - GUI 窗口
# - 多标签页
# - JS 引擎必需
browser = ["gui"]

# ── JS 引擎（互斥） ──

# Boa JS 引擎（纯 Rust）
boa = ["boa_engine", "boa_gc"]

# QuickJS 引擎（C 库，轻量级）
# quickjs = ["quickjs-rs"]  # TODO: 添加依赖

# V8 引擎（obscura-js / Deno）
v8 = ["obscura-js"]

# ── GUI（仅浏览器模式） ──

gui = ["eframe", "egui"]

# ── 向后兼容 ──

# headless 重命名为 embed
headless = ["embed"]
```

## 使用示例

### 嵌入模式 + Boa (默认嵌入)
```toml
rust-browser = { 
  path = "...", 
  default-features = false, 
  features = ["embed", "boa"] 
}
```

### 嵌入模式 + QuickJS
```toml
rust-browser = { 
  path = "...", 
  default-features = false, 
  features = ["embed", "quickjs"] 
}
```

### 嵌入模式无 JS (纯渲染)
```toml
rust-browser = { 
  path = "...", 
  default-features = false, 
  features = ["embed"] 
}
```

### 浏览器模式 + Boa
```toml
rust-browser = { 
  path = "...", 
  default-features = false, 
  features = ["browser", "boa"] 
}
```

### 浏览器模式 + V8 (完整 JS)
```toml
rust-browser = { 
  path = "...", 
  default-features = false, 
  features = ["browser", "v8"] 
}
```

## 编译命令

```bash
# 默认：浏览器模式 + Boa
cargo build

# 嵌入模式 + Boa (jrust-browser 使用)
cargo build --no-default-features --features embed,boa

# 嵌入模式无 JS (最小体积)
cargo build --no-default-features --features embed

# 浏览器模式 + V8
cargo build --no-default-features --features browser,v8

# 向后兼容：headless
cargo build --no-default-features --features headless
```

## 功能对比

| 功能 | embed | browser |
|------|-------|---------|
| GUI 窗口 | ❌ | ✅ |
| 多标签页 | ❌ | ✅ |
| WebNativeBridge | ✅ | ✅ |
| 事件系统 | ✅ | ✅ |
| 网络请求 | ✅ | ✅ |
| 文件操作 | ✅ | ✅ |
| 渲染到内存 | ✅ | ✅ |
| 渲染到窗口 | ❌ | ✅ |
| JS 引擎 | 可选 | 必需 |

## JS 引擎对比

| 特性 | Boa | QuickJS | V8 |
|------|-----|---------|-----|
| 语言 | Rust | C | C++ |
| 体积 | ~2MB | ~700KB | ~10MB |
| 启动速度 | 中 | 快 | 慢 |
| 执行性能 | 中 | 中 | 快 |
| ES6+ 支持 | 部分 | 完整 | 完整 |
| 跨平台 | ✅ | 需编译 | 需编译 |
| 适用场景 | 嵌入 | 嵌入式 | 浏览器 |

## 迁移指南

### 当前代码 → 新架构

**jrust-browser/Cargo.toml**:
```toml
# 旧
rust-browser = { ..., features = ["headless"] }

# 新
rust-browser = { ..., features = ["embed"] }
```

**rust-browser main.rs**:
```rust
// 旧
#[cfg(feature = "gui")]
struct BrowserApp { ... }

// 新
#[cfg(feature = "browser")]
struct BrowserApp { ... }
```

## 下一步

1. 添加 QuickJS 依赖和 feature
2. 重命名 headless → embed
3. 添加 browser feature (隐含 gui)
4. 更新文档
5. 验证所有编译组合
