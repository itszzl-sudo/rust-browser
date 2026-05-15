# Rust Browser - 基于 Taffy + tiny-skia 的浏览器引擎

一个轻量级的浏览器渲染引擎，使用 Rust 语言开发。

## 核心特性

- **Taffy 布局引擎** - CSS Flexbox/Grid 布局计算
- **tiny-skia 渲染** - 高性能 2D 图形绘制
- **Chrome 风格 UI** - 完整的标签页和地址栏界面
- **多标签页支持** - 创建、切换、关闭标签页
- **前进/后退功能** - 完整的浏览历史记录
- **HTTP 网络请求** - 加载网页内容
- **CSS 解析** - 解析和应用样式
- **DOM 树管理** - 文档对象模型
- **截图功能** - 生成 PNG 图像
- **双击运行** - 无需命令行参数，自动打开默认首页
- **默认首页** - 自动加载百度首页 (https://www.baidu.com)

## 项目结构

```
rust-browser/
├── Cargo.toml           # 项目配置
├── README.md            # 项目说明
├── src/
│   ├── main.rs          # 命令行入口
│   ├── lib.rs           # 库入口
│   ├── browser/         # 浏览器核心
│   │   ├── mod.rs       # Browser 主类
│   │   ├── engine.rs    # 浏览器引擎（HTTP 请求、文档解析）
│   │   ├── page.rs      # 页面管理
│   │   ├── tabs.rs      # 标签页管理
│   │   └── ui.rs        # Chrome 风格 UI 绘制
│   ├── css/             # CSS 处理
│   │   ├── mod.rs
│   │   ├── parser.rs    # CSS 解析器
│   │   ├── stylesheet.rs # 样式表、选择器
│   │   └── values.rs    # CSS 值类型
│   ├── dom/             # DOM 树
│   │   ├── mod.rs
│   │   ├── node.rs      # DOM 节点
│   │   └── visitor.rs   # DOM 遍历
│   └── renderer/        # 渲染引擎
│       ├── mod.rs
│       ├── renderer.rs   # 主渲染器
│       ├── painter.rs   # tiny-skia 绘制
│       ├── layout.rs    # Taffy 布局
│       ├── context.rs   # 渲染上下文
│       └── text.rs      # 文本处理
└── examples/
    └── example.html     # 示例 HTML
```

## Chrome 风格 UI 功能

### 标签页（Tabs）
- 多标签页支持
- 标签页标题显示
- 新建标签页按钮
- 关闭标签页按钮
- 标签页切换

### 地址栏（Address Bar）
- URL 显示
- HTTPS 安全指示器（绿色锁图标）
- 导航按钮：后退、前进、刷新、主页
- 菜单按钮

### 功能
- **后退/前进** - 完整的浏览历史记录
- **刷新** - 重新加载当前页面
- **新建标签页** - 打开新标签页
- **关闭标签页** - 关闭当前标签页

## 依赖库

- **taffy 0.4** - CSS 布局引擎
- **tiny-skia 0.11** - 2D 渲染库
- **reqwest** - HTTP 客户端
- **image** - 图像处理
- **log + env_logger** - 日志系统
- **clap** - 命令行参数解析
- **thiserror** - 错误处理

## 使用方法

### 双击运行（无需命令行）

构建后直接双击可执行文件，将自动打开默认首页（百度）：

```bash
# 首先构建
cargo build --release

# 然后在 target/release/ 目录下找到 rust_browser.exe
# 双击运行即可打开 https://www.baidu.com
```

### 基本用法

```bash
# 加载网页并截图
cargo run -- "https://example.com" --output screenshot.png

# 加载本地 HTML 文件
cargo run -- "examples/example.html" --output local_screenshot.png

# 自定义视口尺寸
cargo run -- "https://example.com" --width 1920 --height 1080

# 启用调试模式
cargo run -- "https://example.com" --debug
```

### 命令行参数

- `url` - URL 或本地 HTML 文件路径（默认：https://www.baidu.com）
- `--output, -o` - 输出文件路径（用于截图）
- `--width` - 视口宽度（默认：1280）
- `--height` - 视口高度（默认：720）
- `--debug, -d` - 启用调试模式

## Rust API 示例

```rust
use rust_browser::Browser;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 创建浏览器实例
    let mut browser = Browser::new()?;
    
    // 设置视口
    browser = browser.with_viewport(1280, 720);
    
    // 导航到 URL
    browser.navigate("https://example.com")?;
    
    // 打印页面信息
    if let Some(title) = browser.title() {
        println!("页面标题: {}", title);
    }
    println!("当前 URL: {}", browser.url());
    
    // 保存截图
    browser.screenshot("output.png")?;
    
    Ok(())
}
```

### 多标签页操作

```rust
use rust_browser::Browser;

// 创建新标签页
browser.new_tab()?;

// 切换标签页
browser.switch_to_tab(0);

// 关闭标签页
browser.close_tab()?;

// 后退
browser.go_back()?;

// 前进
browser.go_forward()?;

// 刷新
browser.reload()?;
```

## 构建

```bash
# Debug 构建
cargo build

# Release 构建
cargo build --release

# 运行测试
cargo test

# 检查代码
cargo check
```

## 许可证

MIT
