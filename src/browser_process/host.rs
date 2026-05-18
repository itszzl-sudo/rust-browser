//! BrowserProcessHost - 浏览器进程宿主
//!
//! 管理多个渲染器进程（每个标签页一个），
//! 通过 Mojo IPC 与渲染器通信。
//!
//! # 架构
//!
//! ```text
//! ┌─────────────────────────────────────────────────────┐
//! │                BrowserProcessHost                    │
//! │  ┌──────────────┐  ┌──────────────┐                 │
//! │  │ RendererCh.1 │  │ RendererCh.2 │  ...            │
//! │  │ (Tab 1)      │  │ (Tab 2)      │                 │
//! │  └──────┬───────┘  └──────┬───────┘                 │
//! │         │                 │                          │
//! │    ┌────▼─────────────────▼────┐                    │
//! │    │   Mojo IPC (3 pipes/tab)  │                    │
//! │    │  [Nav] [Input] [Result]   │                    │
//! │    └────────┬──────────────────┘                    │
//! │             │                                       │
//! │    ┌────────▼──────┐                                │
//! │    │  Renderer #1  │  Renderer #2  ...              │
//! │    │  (thread)     │  (thread)                      │
//! │    └───────────────┘                                │
//! └─────────────────────────────────────────────────────┘
//! ```

use crate::browser::Document;
use crate::browser_process::interfaces::*;
use crate::mojo::interface::{InterfaceBinding, InterfaceProxy};
use crate::renderer::Renderer;
use log::{debug, info, warn};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;

/// 全局单调递增的渲染器 ID 生成器
static NEXT_RENDERER_ID: AtomicU64 = AtomicU64::new(1);

/// 渲染器进程的 Mojo IPC 通道集合
///
/// 每个渲染器（标签页）包含三条 Mojo 接口管道：
/// - `navigation_remote`: 浏览器→渲染器，发送导航指令
/// - `input_remote`: 浏览器→渲染器，发送输入事件
/// - `result_binding`: 浏览器端绑定，接收渲染结果
pub struct RendererChannel {
    /// 渲染器唯一标识
    pub id: u64,
    /// 导航接口代理 (Browser → Renderer)
    pub navigation_remote: InterfaceProxy,
    /// 输入事件接口代理 (Browser → Renderer)
    pub input_remote: InterfaceProxy,
    /// 渲染结果接口绑定 (Renderer → Browser)
    pub result_binding: InterfaceBinding,
    /// 当前标签页 URL
    pub url: String,
    /// 页面标题（从渲染结果中更新）
    pub title: Option<String>,
    /// 视口宽度
    pub width: u32,
    /// 视口高度
    pub height: u32,
}

impl RendererChannel {
    /// 向此渲染器发送导航消息
    pub fn navigate(&self, url: &str) -> Result<(), String> {
        let msg = NavigationMessage {
            url: url.to_string(),
            width: self.width,
            height: self.height,
        };
        debug!("RendererChannel #{} navigate: {}", self.id, url);
        self.navigation_remote.send_message(msg.to_message())
    }

    /// 向此渲染器发送输入事件
    pub fn send_input_event(&self, event: InputEvent) -> Result<(), String> {
        debug!("RendererChannel #{} send_input: {:?}", self.id, event);
        self.input_remote.send_message(event.to_message())
    }

    /// 通知此渲染器调整视口尺寸
    pub fn resize(&self, width: u32, height: u32) -> Result<(), String> {
        debug!("RendererChannel #{} resize: {}x{}", self.id, width, height);
        let msg = ResizeMessage { width, height };
        self.navigation_remote.send_message(msg.to_message())
    }
}

/// 浏览器进程宿主 - 管理所有渲染器进程
///
/// 类似 Chrome 的 `BrowserProcessImpl`，负责：
/// - 为每个标签页创建渲染器线程
/// - 通过 Mojo IPC 与渲染器通信
/// - 转发用户输入事件到活跃渲染器
/// - 收集渲染结果
pub struct BrowserProcessHost {
    /// 按 ID 索引的活跃渲染器通道
    renderers: HashMap<u64, RendererChannel>,
    /// 当前活跃的标签页 ID
    active_tab_id: Option<u64>,
}

impl BrowserProcessHost {
    /// 创建新的浏览器进程宿主
    pub fn new() -> Self {
        info!("BrowserProcessHost 启动");
        Self {
            renderers: HashMap::new(),
            active_tab_id: None,
        }
    }

    /// 创建新的渲染器进程并建立 IPC 通道
    ///
    /// 每个渲染器运行在独立的线程中，通过三条 Mojo 管道与浏览器通信。
    ///
    /// # 参数
    ///
    /// * `url` - 初始导航 URL
    /// * `width` - 视口宽度
    /// * `height` - 视口高度
    ///
    /// # 返回
    ///
    /// 新创建的渲染器 ID
    pub fn spawn_renderer(&mut self, url: &str, width: u32, height: u32) -> Result<u64, String> {
        let id = NEXT_RENDERER_ID.fetch_add(1, Ordering::SeqCst);
        info!("创建渲染器进程 #{}: {} ({}x{})", id, url, width, height);

        // 1. 创建三条 Mojo 接口管道
        //    Navigation:   Browser → Renderer
        //    InputEvent:   Browser → Renderer
        //    RenderResult: Renderer → Browser
        let (mut nav_remote, mut nav_receiver) = create_navigation_pipe();
        let (mut input_remote, mut input_receiver) = create_input_event_pipe();
        let (mut result_remote, mut result_receiver) = create_render_result_pipe();

        // 2. 在浏览器端绑定代理/绑定
        let channel = RendererChannel {
            id,
            navigation_remote: nav_remote.bind(),
            input_remote: input_remote.bind(),
            result_binding: result_receiver.bind(),
            url: url.to_string(),
            title: None,
            width,
            height,
        };

        // 3. 在渲染器端绑定端点（将在新线程中使用）
        let nav_binding = nav_receiver.bind();
        let input_binding = input_receiver.bind();
        let result_proxy = result_remote.bind();

        // 4. 启动渲染器线程
        let renderer_url = url.to_string();
        thread::Builder::new()
            .name(format!("Renderer-{}", id))
            .spawn(move || {
                info!("渲染器进程 #{} 线程已启动", id);
                run_renderer_process(
                    id,
                    renderer_url,
                    width,
                    height,
                    nav_binding,
                    input_binding,
                    result_proxy,
                );
            })
            .map_err(|e| format!("创建渲染器线程失败: {}", e))?;

        // 5. 保存通道并设置为活跃
        self.renderers.insert(id, channel);
        self.active_tab_id = Some(id);
        Ok(id)
    }

    /// 向活跃渲染器发送导航消息
    pub fn navigate(&self, url: &str) -> Result<(), String> {
        match self.active_tab_id {
            Some(id) => match self.renderers.get(&id) {
                Some(renderer) => renderer.navigate(url),
                None => Err("未找到活跃渲染器".to_string()),
            },
            None => Err("没有活跃标签页".to_string()),
        }
    }

    /// 投递输入事件到活跃渲染器
    pub fn send_input(&self, event: InputEvent) -> Result<(), String> {
        if let Some(id) = self.active_tab_id {
            if let Some(renderer) = self.renderers.get(&id) {
                renderer.send_input_event(event)
            } else {
                Ok(())
            }
        } else {
            Ok(())
        }
    }

    /// 从活跃渲染器的结果接口非阻塞地接收渲染结果
    pub fn try_receive_result(&mut self) -> Option<RenderResultMessage> {
        let id = self.active_tab_id?;
        let renderer = self.renderers.get(&id)?;
        if let Some(msg) = renderer.result_binding.try_receive() {
            let result = RenderResultMessage::from_message(&msg);
            // 更新标签页标题
            if let Some(ref result) = result {
                if let Some(ref title) = result.title {
                    if let Some(channel) = self.renderers.get_mut(&id) {
                        channel.title = Some(title.clone());
                    }
                }
            }
            result
        } else {
            None
        }
    }

    /// 获取指定渲染器的通道引用
    pub fn get_renderer(&self, id: u64) -> Option<&RendererChannel> {
        self.renderers.get(&id)
    }

    /// 获取活跃渲染器的通道引用
    pub fn active_renderer(&self) -> Option<&RendererChannel> {
        let id = self.active_tab_id?;
        self.renderers.get(&id)
    }

    /// 切换到指定标签页
    pub fn switch_to_tab(&mut self, id: u64) -> bool {
        if self.renderers.contains_key(&id) {
            self.active_tab_id = Some(id);
            info!("切换到标签页 #{}", id);
            true
        } else {
            warn!("标签页 #{} 不存在", id);
            false
        }
    }

    /// 关闭并移除指定渲染器
    pub fn close_renderer(&mut self, id: u64) -> bool {
        if self.renderers.remove(&id).is_some() {
            info!("关闭渲染器 #{}", id);
            if self.active_tab_id == Some(id) {
                self.active_tab_id = self.renderers.keys().next().copied();
            }
            true
        } else {
            false
        }
    }

    /// 返回当前渲染器数量
    pub fn renderer_count(&self) -> usize {
        self.renderers.len()
    }

    /// 返回所有渲染器 ID 列表
    pub fn renderer_ids(&self) -> Vec<u64> {
        self.renderers.keys().copied().collect()
    }
}

impl Default for BrowserProcessHost {
    fn default() -> Self {
        Self::new()
    }
}

// ==========================================================================
// 渲染器进程主循环
// ==========================================================================

/// 渲染器进程的主循环，运行在独立的线程中。
///
/// 负责：
/// 1. 创建并初始化 `Renderer` 实例
/// 2. 监听来自浏览器进程的导航/输入消息
/// 3. 执行页面加载和渲染
/// 4. 将渲染结果通过 Mojo IPC 发回浏览器
fn run_renderer_process(
    id: u64,
    initial_url: String,
    width: u32,
    height: u32,
    nav_binding: InterfaceBinding,
    input_binding: InterfaceBinding,
    result_proxy: InterfaceProxy,
) {
    info!("Renderer #{} 初始化 ({}x{})", id, width, height);

    // 创建渲染器实例
    let mut renderer = Renderer::new(width, height);
    let mut current_url = initial_url.clone();
    let mut running = true;

    // 首次导航（如果 URL 不是空白页）
    if !current_url.is_empty() && current_url != "about:blank" {
        info!("Renderer #{} 首次导航到: {}", id, current_url);
        if let Ok(doc) = load_document(&current_url) {
            let title = doc.title.clone();
            let doc_opt = Some(doc);
            if let Ok(png) = renderer.render(&doc_opt) {
                let result = RenderResultMessage {
                    png_data: png,
                    width,
                    height,
                    title,
                };
                let _ = result_proxy.send_message(result.to_message());
            }
        }
    }

    // 消息循环: 处理来自浏览器进程的 IPC 消息
    while running {
        // 处理导航消息
        while let Some(msg) = nav_binding.try_receive() {
            match msg.name {
                "Navigate" => {
                    if let Some(nav) = NavigationMessage::from_message(&msg) {
                        info!("Renderer #{} 导航到: {}", id, nav.url);
                        current_url = nav.url.clone();
                        let new_width = nav.width;
                        let new_height = nav.height;
                        renderer.set_viewport(new_width, new_height);

                        if let Ok(doc) = load_document(&current_url) {
                            let title = doc.title.clone();
                            let doc_opt = Some(doc);
                            if let Ok(png) = renderer.render(&doc_opt) {
                                let result = RenderResultMessage {
                                    png_data: png,
                                    width: new_width,
                                    height: new_height,
                                    title,
                                };
                                let _ = result_proxy.send_message(result.to_message());
                            }
                        }
                    }
                }
                "Resize" => {
                    // 解析尺寸变更（格式: width|height）
                    let s = String::from_utf8_lossy(&msg.data);
                    let parts: Vec<&str> = s.split('|').collect();
                    if parts.len() >= 2 {
                        let w: u32 = parts[0].parse().unwrap_or(width);
                        let h: u32 = parts[1].parse().unwrap_or(height);
                        debug!("Renderer #{} resize: {}x{}", id, w, h);
                        renderer.set_viewport(w, h);
                    }
                }
                _ => {
                    debug!("Renderer #{} 未知导航消息: {}", id, msg.name);
                }
            }
        }

        // 处理输入消息
        while let Some(msg) = input_binding.try_receive() {
            match msg.name.as_ref() {
                "MouseClick" => {
                    // 解析坐标（格式: x|y|button）
                    let s = String::from_utf8_lossy(&msg.data);
                    let parts: Vec<&str> = s.split('|').collect();
                    if parts.len() >= 2 {
                        let x: f32 = parts[0].parse().unwrap_or(0.0);
                        let y: f32 = parts[1].parse().unwrap_or(0.0);
                        debug!("Renderer #{} MouseClick at ({:.1}, {:.1})", id, x, y);

                        // hit testing：在渲染器的布局结果中查找点击位置对应的元素
                        if let Some(doc) = &renderer.document() {
                            let dom = doc.get_dom();
                            if let Some(href) = renderer.hit_test_link(x, y, dom) {
                                info!("Renderer #{} 点击链接: {} -> 导航", id, href);
                                // 处理相对 URL
                                let absolute_url = if href.starts_with("http://")
                                    || href.starts_with("https://")
                                    || href.starts_with("file://")
                                {
                                    href.clone()
                                } else {
                                    // 基于当前 URL 解析相对路径
                                    let base = current_url.trim_end_matches('/');
                                    if href.starts_with('/') {
                                        // 绝对路径，基于域名
                                        if let Some(pos) = base.find("//") {
                                            let after_scheme = &base[pos + 2..];
                                            if let Some(slash_pos) = after_scheme.find('/') {
                                                format!(
                                                    "{}://{}{}",
                                                    &base[..base.find("//").unwrap_or(pos)],
                                                    &after_scheme[..slash_pos],
                                                    href
                                                )
                                            } else {
                                                format!("{}{}", base, href)
                                            }
                                        } else {
                                            format!("{}{}", base, href)
                                        }
                                    } else {
                                        format!("{}/{}", base, href)
                                    }
                                };
                                current_url = absolute_url.clone();

                                // 重新加载新页面
                                let new_width = renderer.context().viewport().0;
                                let new_height = renderer.context().viewport().1;
                                renderer.set_viewport(new_width, new_height);

                                if let Ok(doc) = load_document(&current_url) {
                                    let title = doc.title.clone();
                                    let doc_opt = Some(doc);
                                    if let Ok(png) = renderer.render(&doc_opt) {
                                        let result = RenderResultMessage {
                                            png_data: png,
                                            width: new_width,
                                            height: new_height,
                                            title,
                                        };
                                        let _ = result_proxy.send_message(result.to_message());
                                    }
                                }
                            }
                        }
                    }
                }
                "Scroll" => {
                    debug!("Renderer #{} Scroll event received", id);
                }
                "MouseMove" => {
                    debug!("Renderer #{} MouseMove event received", id);
                }
                _ => {
                    debug!("Renderer #{} 未知输入消息: {}", id, msg.name);
                }
            }
        }

        // 检查通道是否关闭（浏览器端已断开连接）
        if nav_binding.port().is_closed() && input_binding.port().is_closed() {
            running = false;
        }

        // 避免忙等——10ms 轮询间隔
        std::thread::sleep(std::time::Duration::from_millis(10));
    }

    info!("Renderer #{} 关闭", id);
}

// ==========================================================================
// 文档加载辅助
// ==========================================================================

/// 加载文档（网络请求或本地文件）
///
/// 支持以下 URL 格式：
/// - `https://` / `http://` 网络资源
/// - `file://` 本地文件
/// - `*.html` / `*.htm` 本地 HTML 文件
fn load_document(url: &str) -> Result<Document, String> {
    if url.starts_with("file://")
        || url.ends_with(".html")
        || url.ends_with(".htm")
        || url == "about:blank"
    {
        let path = url.trim_start_matches("file://");
        if url == "about:blank" {
            let blank_html = r#"<!DOCTYPE html>
<html>
<head><title>空白页面</title></head>
<body style="background: white; display: flex; align-items: center; justify-content: center; height: 100vh; margin: 0;">
    <h1 style="color: #333;">空白页面</h1>
</body>
</html>"#;
            Ok(Document::from_html(blank_html, url))
        } else {
            let html = std::fs::read_to_string(path).map_err(|e| format!("读取文件失败: {}", e))?;
            Ok(Document::from_html(&html, url))
        }
    } else {
        // 网络请求（使用现有网络客户端 + tokio runtime）
        let rt = tokio::runtime::Runtime::new().map_err(|e| format!("创建运行时失败: {}", e))?;
        let html = rt
            .block_on(crate::network::NetworkClient::new().fetch_html(url))
            .map_err(|e| format!("网络请求失败: {}", e))?;
        Ok(Document::from_html(&html, url))
    }
}

// ==========================================================================
// Tests
// ==========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_browser_process_host_creation() {
        let host = BrowserProcessHost::new();
        assert_eq!(host.renderer_count(), 0);
        assert!(host.active_renderer().is_none());
    }

    #[test]
    fn test_spawn_renderer() {
        let mut host = BrowserProcessHost::new();
        let id = host
            .spawn_renderer("about:blank", 800, 600)
            .expect("should spawn renderer");
        assert_eq!(host.renderer_count(), 1);
        assert!(host.active_renderer().is_some());
        assert_eq!(host.active_renderer().unwrap().id, id);
    }

    #[test]
    fn test_spawn_multiple_renderers() {
        let mut host = BrowserProcessHost::new();
        let id1 = host.spawn_renderer("about:blank", 800, 600).unwrap();
        let id2 = host.spawn_renderer("about:blank", 1024, 768).unwrap();
        assert_eq!(host.renderer_count(), 2);
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_switch_tab() {
        let mut host = BrowserProcessHost::new();
        let id1 = host.spawn_renderer("about:blank", 800, 600).unwrap();
        let id2 = host.spawn_renderer("about:blank", 800, 600).unwrap();

        assert!(host.switch_to_tab(id1));
        assert_eq!(host.active_renderer().unwrap().id, id1);

        assert!(host.switch_to_tab(id2));
        assert_eq!(host.active_renderer().unwrap().id, id2);

        assert!(!host.switch_to_tab(999));
    }

    #[test]
    fn test_close_renderer() {
        let mut host = BrowserProcessHost::new();
        let id = host.spawn_renderer("about:blank", 800, 600).unwrap();
        assert_eq!(host.renderer_count(), 1);

        assert!(host.close_renderer(id));
        assert_eq!(host.renderer_count(), 0);
    }
}
