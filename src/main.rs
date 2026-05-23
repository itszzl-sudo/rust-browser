//! Rust Browser - 主程序入口
//!
//! Chrome 多进程架构浏览器
//! - Browser Process (主线程/UI)
//! - Renderer Processes (每个标签页独立线程)
//! - Mojo IPC 通信
//! - Task Queue 任务调度

#![cfg_attr(not(feature = "gui"), allow(dead_code, unused_imports))]

use clap::Parser;
use image::{ImageBuffer, ImageFormat, Rgba};
use log::{error, info};
use rust_browser::browser_process::host::BrowserProcessHost;
use rust_browser::js_engine::CONSOLE_LOG_BUFFER;
use rust_browser::task_queue::GLOBAL_SCHEDULER;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Instant;

#[cfg(not(feature = "gui"))]
use std::time::Duration;

// 全局日志消息
static LOG_MESSAGES: Mutex<Vec<String>> = Mutex::new(Vec::new());

fn add_log_message(msg: String) {
    if let Ok(mut logs) = LOG_MESSAGES.lock() {
        logs.push(msg);
        if logs.len() > 1000 {
            logs.drain(0..500);
        }
    }
}

fn get_log_messages() -> Vec<String> {
    LOG_MESSAGES.lock().map(|l| l.clone()).unwrap_or_default()
}

const DEFAULT_URL: &str = "https://www.baidu.com";
const LOAD_TIMEOUT_SECS: u64 = 30;

/// 命令行参数
#[derive(Parser, Debug)]
#[command(name = "rust-browser", version, about = "A Web Browser in Rust")]
struct Args {
    /// 初始 URL
    #[arg(default_value = DEFAULT_URL)]
    url: String,

    /// 截图输出路径（截图模式）
    #[arg(short = 'o', long)]
    output: Option<PathBuf>,

    /// 视口宽度
    #[arg(short = 'W', long, default_value = "1280")]
    width: u32,

    /// 视口高度
    #[arg(short = 'H', long, default_value = "720")]
    height: u32,

    /// 调试模式
    #[arg(short = 'd', long)]
    debug: bool,

    /// 渲染器进程模式（由浏览器进程启动子进程时使用，用户不需要手动设置）
    #[arg(long, hide = true)]
    renderer_process: bool,

    /// 渲染器进程 ID（仅在 --renderer-process 模式下使用）
    #[arg(long, hide = true, default_value = "0")]
    renderer_id: u64,

    /// 渲染器进程的 IPC 管道句柄（Windows 下为管道句柄值）
    #[arg(long, hide = true, default_value = "0")]
    ipc_handle: u64,
}

#[cfg(feature = "gui")]
struct BrowserApp {
    /// 浏览器进程宿主（管理渲染器进程）
    browser_host: Option<BrowserProcessHost>,
    /// 当前活跃渲染器 ID
    active_renderer_id: Option<u64>,
    /// URL 输入框内容
    url_input: String,
    /// RGBA 像素数据（直接传给 winit 主窗口，不经过 egui）
    rgba_pixels: Option<(u32, u32, Vec<u8>)>,
    /// 错误信息
    error_message: Option<String>,
    /// 初始 URL
    #[allow(dead_code)]
    initial_url: String,
    /// 是否正在加载
    is_loading: bool,
    /// 首帧标记
    first_frame: bool,
    /// 加载开始时间
    load_start_time: Option<Instant>,
    /// 待自动加载
    auto_load_pending: bool,
    /// 视口尺寸
    width: u32,
    height: u32,
    /// 导航历史（用于后退/前进）
    nav_history: Vec<String>,
    /// 当前在历史中的位置
    nav_history_pos: i32,
    /// 是否显示书签栏
    show_bookmarks: bool,
    /// 是否显示调试日志面板
    show_log_panel: bool,
    /// 是否显示 DevTools 面板
    show_devtools: bool,
    /// DevTools 当前选中的标签页（"dom", "console", "network"）
    devtools_tab: String,
    /// 书签列表
    bookmarks: Vec<(String, String)>,
    /// 书签编辑对话框的状态
    editing_bookmark: Option<(usize, String)>,
    /// 被固定的标签页 ID 集合
    pinned_tabs: Vec<u64>,
    /// winit 窗口管理器
    gui_manager: Option<rust_browser::gui_window::WindowManager>,
}

#[cfg(feature = "gui")]
impl BrowserApp {
    fn new(initial_url: String, width: u32, height: u32) -> Self {
        add_log_message("=== Rust Browser (Chrome 多进程架构) ===".to_string());
        add_log_message("正在初始化浏览器进程...".to_string());

        // 初始化全局 TaskQueue 调度器
        let _ = &*GLOBAL_SCHEDULER;
        add_log_message("TaskQueue 调度器已启动".to_string());

        // 创建浏览器进程宿主
        let mut browser_host = BrowserProcessHost::new();

        // 启动渲染器进程（通过 IPC）
        add_log_message(format!("创建默认渲染器进程: {}", initial_url));
        let renderer_id = match browser_host.spawn_renderer(&initial_url, width, height) {
            Ok(id) => {
                add_log_message(format!("渲染器进程 #{} 已创建", id));
                Some(id)
            }
            Err(e) => {
                let err = format!("创建渲染器失败: {}", e);
                add_log_message(err);
                None
            }
        };

        add_log_message(format!("窗口尺寸: {}x{}", width, height));

        Self {
            browser_host: Some(browser_host),
            active_renderer_id: renderer_id,
            url_input: initial_url.clone(),
            rgba_pixels: None,
            error_message: None,
            is_loading: false,
            first_frame: true,
            load_start_time: None,
            auto_load_pending: true,
            width,
            height,
            nav_history: vec![initial_url.clone()],
            initial_url,
            nav_history_pos: 0,
            show_bookmarks: true,
            show_log_panel: false,
            show_devtools: false,
            devtools_tab: "console".to_string(),
            bookmarks: vec![
                ("百度".to_string(), "https://www.baidu.com".to_string()),
                ("谷歌".to_string(), "https://www.google.com".to_string()),
                ("GitHub".to_string(), "https://github.com".to_string()),
                ("Rust".to_string(), "https://www.rust-lang.org".to_string()),
                (
                    "Hacker News".to_string(),
                    "https://news.ycombinator.com".to_string(),
                ),
            ],
            editing_bookmark: None,
            pinned_tabs: Vec::new(),
            gui_manager: None,
        }
    }

    /// 从 JS 引擎的 console 日志缓冲区读取消息，加入 GUI 日志面板
    fn flush_console_logs(&self) {
        if let Ok(mut buf) = CONSOLE_LOG_BUFFER.lock() {
            while let Some(msg) = buf.pop() {
                add_log_message(msg);
            }
        }
    }

    /// 通过 IPC 从渲染器进程获取最新帧
    fn refresh_from_renderer(&mut self) {
        // 先消费 JS console 日志
        self.flush_console_logs();

        if let Some(ref mut host) = self.browser_host {
            // 优先接收 RGBA 渲染结果（省掉 PNG 编解码）
            if let Some(result) = host.try_receive_rgba_result() {
                add_log_message(format!(
                    "收到渲染帧: {}x{} (RGBA, {} bytes, is_loading={})",
                    result.width,
                    result.height,
                    result.rgba_data.len(),
                    result.is_loading
                ));

                let w = result.width;
                let h = result.height;
                let pixels = result.rgba_data;

                // 只有非 loading 帧才保存和显示，loading 帧只设置状态
                if !result.is_loading {
                    // 保存 RGBA 像素，通过 winit 主窗口显示
                    self.rgba_pixels = Some((w, h, pixels));
                    self.error_message = None;
                    self.is_loading = false;
                    if let Some(ref title) = result.title {
                        add_log_message(format!("页面标题: {}", title));
                    }

                    // 推送像素到 winit 主窗口
                    if let Some(ref manager) = self.gui_manager {
                        if let Some((ref pw, ref ph, ref ppixels)) = self.rgba_pixels {
                            manager.update_pixels(*pw, *ph, ppixels.clone());
                        }
                    }
                } else {
                    // loading 帧只设置加载状态，不更新显示内容
                    self.is_loading = true;
                }
            }
        }
    }

    fn navigate(&mut self, url: &str) {
        let trimmed = url.trim();
        if trimmed.is_empty() {
            return;
        }

        add_log_message(format!("Mojo IPC 导航到: {}", trimmed));
        self.is_loading = true;
        self.error_message = None;
        self.load_start_time = Some(Instant::now());

        if let Some(ref host) = self.browser_host {
            match host.navigate(trimmed) {
                Ok(_) => {
                    self.url_input = trimmed.to_string();
                    // 更新导航历史
                    if self.nav_history_pos < self.nav_history.len() as i32 - 1 {
                        self.nav_history
                            .truncate((self.nav_history_pos + 1) as usize);
                    }
                    self.nav_history.push(trimmed.to_string());
                    self.nav_history_pos = self.nav_history.len() as i32 - 1;
                    add_log_message("导航消息已通过 IPC 发送到渲染器".to_string());
                }
                Err(e) => {
                    let err = format!("导航 IPC 发送失败: {}", e);
                    add_log_message(err.clone());
                    self.error_message = Some(err);
                    self.is_loading = false;
                }
            }
        } else {
            add_log_message("错误: 浏览器进程未初始化".to_string());
            self.is_loading = false;
        }
    }

    fn go_back(&mut self) {
        if self.nav_history_pos > 0 {
            self.nav_history_pos -= 1;
            let url = self.nav_history[self.nav_history_pos as usize].clone();
            self.navigate(&url);
        }
    }

    fn go_forward(&mut self) {
        if (self.nav_history_pos as usize) < self.nav_history.len() - 1 {
            self.nav_history_pos += 1;
            let url = self.nav_history[self.nav_history_pos as usize].clone();
            self.navigate(&url);
        }
    }

    fn refresh(&mut self) {
        if let Some(current_url) = self.nav_history.last().cloned() {
            self.navigate(&current_url);
        }
    }

    fn add_bookmark(&mut self) {
        let url = self.url_input.clone();
        if url.is_empty() || url == "about:blank" {
            return;
        }
        // 获取页面标题作为书签名称
        let title = self
            .browser_host
            .as_ref()
            .and_then(|h| h.active_renderer())
            .and_then(|r| r.title.clone())
            .unwrap_or_else(|| url.clone());

        // 避免重复
        if !self.bookmarks.iter().any(|(_, u)| u == &url) {
            let title_for_log = title.clone();
            self.bookmarks.push((title, url));
            add_log_message(format!("书签已添加: {}", title_for_log));
        } else {
            add_log_message("书签已存在".to_string());
        }
    }

    fn remove_bookmark(&mut self, index: usize) {
        if index < self.bookmarks.len() {
            let removed = self.bookmarks.remove(index);
            add_log_message(format!("书签已删除: {}", removed.0));
        }
    }

    fn edit_bookmark(&mut self, index: usize, new_name: &str) {
        if index < self.bookmarks.len() {
            self.bookmarks[index].0 = new_name.to_string();
            add_log_message(format!("书签已重命名: {}", new_name));
        }
    }
}

#[cfg(feature = "gui")]
#[allow(deprecated)]
impl eframe::App for BrowserApp {
    fn ui(&mut self, ctx: &mut egui::Ui, _frame: &mut eframe::Frame) {
        // ── 首帧初始化 ──
        if self.first_frame {
            self.first_frame = false;
            add_log_message("窗口已显示，等待渲染器加载首页...".to_string());
            info!("首帧渲染完成，窗口已显示");
            self.is_loading = true;
            self.load_start_time = Some(Instant::now());

            // 获取屏幕工作区尺寸
            let (screen_w, screen_h) = {
                let monitor = ctx.input(|i| i.viewport().monitor_size);
                match monitor {
                    Some(v) => (v.x as u32, v.y as u32),
                    None => (1920, 1080),
                }
            };

            // 创建窗口管理器（共享状态，不创建独立事件循环）
            let gui_manager = rust_browser::gui_window::WindowManager::create(screen_w, screen_h);
            add_log_message(format!(
                "布局: 工具栏=80px, 内容区={}x{}",
                screen_w,
                screen_h.saturating_sub(80).saturating_sub(28)
            ));

            self.gui_manager = Some(gui_manager);
        }

        // ── 检测窗口大小变化并更新渲染器视口 ──
        let current_viewport_size = ctx.input(|i| {
            let rect = i.viewport().outer_rect;
            if let Some(rect) = rect {
                (rect.width() as u32, rect.height() as u32)
            } else {
                (1280, 720) // 默认尺寸
            }
        });
        
        // 简单的大小变化检测（考虑到工具栏占用的高度）
        let estimated_content_width = current_viewport_size.0;
        let estimated_content_height = current_viewport_size.1.saturating_sub(150); // 估计的工具栏高度
        
        if estimated_content_width > 0 && estimated_content_height > 0 {
            // 如果当前存储的宽高与估计的内容区大小差异较大，更新渲染器
            let width_diff = (estimated_content_width as i32 - self.width as i32).abs();
            let height_diff = (estimated_content_height as i32 - self.height as i32).abs();
            
            if width_diff > 50 || height_diff > 50 {
                self.width = estimated_content_width;
                self.height = estimated_content_height;
                
                add_log_message(format!("窗口大小变化: 更新视口为 {}x{}", self.width, self.height));
                
                // 更新当前活跃渲染器的视口大小
                if let Some(ref mut host) = self.browser_host {
                    if let Some(active_id) = self.active_renderer_id {
                        let _ = host.resize_renderer(active_id, self.width, self.height);
                    }
                }
            }
        }

        // 每帧尝试接收渲染结果
        self.refresh_from_renderer();

        // 检查并发送待处理的滚动事件（从 winit 主窗口接收）
        if let Some(ref manager) = self.gui_manager {
            let scroll = {
                let mut s = manager.state.lock().unwrap();
                s.pending_scroll.take()
            };
            if let Some((dx, dy)) = scroll {
                add_log_message(format!("滚动事件: dx={:.0}, dy={:.0}", dx, dy));
                let event = rust_browser::browser_process::interfaces::InputEvent::Scroll {
                    delta_x: dx * 40.0, // LineDelta → 像素转换
                    delta_y: dy * 40.0,
                };
                if let Some(ref mut host) = self.browser_host {
                    let _ = host.send_input(event);
                }
            }
        }

        // 清除首帧自动加载标记（收到渲染帧后设置为false）
        if self.rgba_pixels.is_some() {
            self.auto_load_pending = false;
        }

        // 加载超时处理
        if self.is_loading {
            if let Some(start_time) = self.load_start_time {
                let elapsed = start_time.elapsed().as_secs();
                if elapsed >= LOAD_TIMEOUT_SECS {
                    add_log_message("加载超时！".to_string());
                    self.is_loading = false;
                    self.error_message = Some("网络连接超时，请检查网络后重试".to_string());
                }
            }
        }

        // 在每一帧运行 TaskQueue 的 main thread 任务
        GLOBAL_SCHEDULER.run_main_tasks();

        ctx.set_visuals(egui::Visuals::dark());

        // ═══════════════════════════════════════════
        // 顶部面板 (egui 浏览器 UI 条)
        // ═══════════════════════════════════════════

        let tabs_data: Vec<(u64, String, bool)> = if let Some(ref host) = self.browser_host {
            let active_id = self.active_renderer_id;
            host.renderer_ids()
                .iter()
                .map(|&tab_id| {
                    let is_active = Some(tab_id) == active_id;
                    let label = if let Some(renderer) = host.get_renderer(tab_id) {
                        if let Some(ref r_title) = renderer.title {
                            if r_title.is_empty() {
                                format!("Tab #{}", tab_id)
                            } else {
                                r_title.clone()
                            }
                        } else {
                            let short = renderer
                                .url
                                .trim_start_matches("https://")
                                .trim_start_matches("http://")
                                .trim_end_matches('/');
                            if short.is_empty() {
                                format!("Tab #{}", tab_id)
                            } else {
                                short.to_string()
                            }
                        }
                    } else {
                        format!("Tab #{}", tab_id)
                    };
                    (tab_id, label, is_active)
                })
                .collect()
        } else {
            Vec::new()
        };
        let renderer_count = self
            .browser_host
            .as_ref()
            .map(|h| h.renderer_ids().len())
            .unwrap_or(0);

        // ── 标签页栏 ──
        egui::Panel::top("tab_bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                for (tab_id, label, is_active) in &tabs_data {
                    let is_pinned = self.pinned_tabs.contains(tab_id);

                    // 固定标签页显示 📌 图标
                    let tab_label = if is_pinned {
                        format!("📌 {}", label)
                    } else {
                        label.clone()
                    };

                    let resp = if *is_active {
                        ui.selectable_label(true, &tab_label)
                    } else {
                        ui.selectable_label(false, &tab_label)
                    };
                    if resp.clicked() && !is_active {
                        add_log_message(format!("切换到 Tab #{}", tab_id));
                        if let Some(ref mut host) = self.browser_host {
                            host.switch_to_tab(*tab_id);
                            self.active_renderer_id = Some(*tab_id);
                            self.refresh_from_renderer();
                        }
                    }

                    // Pin/Unpin 按钮
                    let pin_label = if is_pinned { "📍" } else { "📌" };
                    if ui.button(pin_label).clicked() {
                        if is_pinned {
                            self.pinned_tabs.retain(|&id| id != *tab_id);
                            add_log_message(format!("Tab #{} 取消固定", tab_id));
                        } else {
                            self.pinned_tabs.push(*tab_id);
                            add_log_message(format!("Tab #{} 已固定", tab_id));
                        }
                    }

                    if renderer_count > 1 {
                        if ui.button("x").clicked() {
                            add_log_message(format!("关闭 Tab #{}", tab_id));
                            if let Some(ref mut host) = self.browser_host {
                                // 关闭前先移除固定状态
                                self.pinned_tabs.retain(|&id| id != *tab_id);
                                host.close_renderer(*tab_id);
                                self.active_renderer_id = host.active_renderer().map(|r| r.id);
                                self.refresh_from_renderer();
                            }
                            ui.close();
                        }
                    }
                }
                if ui.button("+").clicked() {
                    add_log_message("新建 Tab".to_string());
                    if let Some(ref mut host) = self.browser_host {
                        if let Ok(new_id) =
                            host.spawn_renderer("about:blank", self.width, self.height)
                        {
                            self.active_renderer_id = Some(new_id);
                            self.refresh_from_renderer();
                        }
                    }
                }
            });
        });

        // ── 书签编辑对话框 ──
        if let Some((idx, ref mut name)) = self.editing_bookmark.clone() {
            let mut new_name = name.clone();
            egui::Window::new("编辑书签")
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.label("书签名称:");
                    ui.text_edit_singleline(&mut new_name);
                    ui.horizontal(|ui| {
                        if ui.button("保存").clicked() {
                            self.edit_bookmark(idx, &new_name);
                            self.editing_bookmark = None;
                        }
                        if ui.button("取消").clicked() {
                            self.editing_bookmark = None;
                        }
                    });
                });
        }

        // ── 导航栏 ──
        egui::Panel::top("nav_bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;

                // 后退
                let back_enabled = self.nav_history_pos > 0;
                if ui
                    .add_enabled(back_enabled, egui::Button::new("<"))
                    .clicked()
                {
                    self.go_back();
                }

                // 前进
                let fwd_enabled =
                    (self.nav_history_pos as usize) < self.nav_history.len().saturating_sub(1);
                if ui
                    .add_enabled(fwd_enabled, egui::Button::new(">"))
                    .clicked()
                {
                    self.go_forward();
                }

                // 刷新
                if ui.button("\u{21bb}").clicked() {
                    self.refresh();
                }

                // URL 地址栏（可伸缩占满空间）
                ui.add(
                    egui::TextEdit::singleline(&mut self.url_input)
                        .font(egui::TextStyle::Monospace)
                        .desired_width(f32::INFINITY)
                        .hint_text("URL 输入后 Enter 导航"),
                );

                // 转到按钮
                if ui.button("Go").clicked() && !self.url_input.is_empty() {
                    let url = self.url_input.clone();
                    if !url.starts_with("http://")
                        && !url.starts_with("https://")
                        && !url.starts_with("file://")
                        && url != "about:blank"
                    {
                        self.url_input = format!("https://{}", url);
                    }
                    self.navigate(&self.url_input.clone());
                }

                // 添加书签按钮（星标）
                if ui.button("☆").clicked() {
                    self.add_bookmark();
                }

                // 书签切换按钮
                let bk_label = if self.show_bookmarks {
                    "▼BK"
                } else {
                    "▶BK"
                };
                if ui.button(bk_label).clicked() {
                    self.show_bookmarks = !self.show_bookmarks;
                }

                // DevTools 切换按钮
                let dt_label = if self.show_devtools {
                    "🔍 DevTools ▼"
                } else {
                    "🔍 DevTools ▶"
                };
                if ui.button(dt_label).clicked() {
                    self.show_devtools = !self.show_devtools;
                    if self.show_devtools {
                        self.show_log_panel = false;
                    }
                }

                // 调试日志切换按钮
                let log_label = if self.show_log_panel {
                    "▼LOG"
                } else {
                    "▶LOG"
                };
                if ui.button(log_label).clicked() {
                    self.show_log_panel = !self.show_log_panel;
                    if self.show_log_panel {
                        self.show_devtools = false;
                    }
                }
            });

            // ── 加载进度条 ──
            if self.is_loading {
                let w = ui.available_width();
                let time = ctx.input(|i| i.time);
                let p = ((time * 2.0).sin() * 0.5 + 0.5) as f32;
                let painter = ui.painter();
                let bar_y = ui.cursor().min.y;
                let c = egui::Color32::from_rgb(0x4A, 0x90, 0xD9)
                    .lerp_to_gamma(egui::Color32::from_rgb(0xAA, 0xCC, 0xEE), p);
                let bw = w * 0.3;
                let off = (((time * 60.0) as f32) % (w + bw)) - bw;
                painter.rect_filled(
                    egui::Rect::from_min_size(egui::pos2(off, bar_y), egui::vec2(bw, 3.0)),
                    0.0,
                    c,
                );
                ui.allocate_space(egui::vec2(w, 3.0));
            }
        });

        // ── 书签栏 ──
        if self.show_bookmarks {
            let bookmarks = self.bookmarks.clone();
            egui::Panel::top("bookmark_bar").show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    if bookmarks.is_empty() {
                        ui.label("（无书签）");
                    } else {
                        for (i, (name, url)) in bookmarks.iter().enumerate() {
                            let resp = ui.link(name);
                            if resp.clicked() {
                                self.url_input = url.clone();
                                self.navigate(url);
                                self.show_bookmarks = false;
                            }
                            // 右键删除
                            if resp.secondary_clicked() {
                                self.remove_bookmark(i);
                                ui.close();
                            }
                            // 编辑按钮
                            if ui.button("✎").clicked() {
                                self.editing_bookmark = Some((i, name.clone()));
                            }
                        }
                    }
                });
            });
        }

        // ═══════════════════════════════════════════
        // 日志面板（按快捷键 Ctrl+L 切换，默认隐藏）
        // ═══════════════════════════════════════════
        if self.show_log_panel {
            egui::Panel::bottom("log_panel").show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.heading("IPC 日志");
                    if ui.button("清空").clicked() {
                        if let Ok(mut logs) = LOG_MESSAGES.lock() {
                            logs.clear();
                        }
                    }
                });
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .stick_to_bottom(true)
                    .max_height(150.0)
                    .show(ui, |ui| {
                        let logs = get_log_messages();
                        for log in logs.iter().rev().take(30) {
                            ui.label(log);
                        }
                    });
            });
        }

        // ═══════════════════════════════════════════
        // DevTools 面板
        // ═══════════════════════════════════════════
        if self.show_devtools {
            egui::Panel::bottom("devtools_panel").show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.selectable_value(
                        &mut self.devtools_tab,
                        "console".to_string(),
                        "📋 Console",
                    );
                    ui.selectable_value(&mut self.devtools_tab, "dom".to_string(), "🌳 DOM");
                    ui.selectable_value(
                        &mut self.devtools_tab,
                        "network".to_string(),
                        "🌐 Network",
                    );
                    ui.selectable_value(&mut self.devtools_tab, "info".to_string(), "ℹ️ Info");
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("✕").clicked() {
                            self.show_devtools = false;
                        }
                    });
                });
                ui.separator();

                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .max_height(200.0)
                    .show(ui, |ui| {
                        match self.devtools_tab.as_str() {
                            "console" => {
                                // Console 标签 — 显示 IPC 日志和 JS console 消息
                                ui.horizontal(|ui| {
                                    ui.heading("Console");
                                    if ui.button("清空").clicked() {
                                        if let Ok(mut logs) = LOG_MESSAGES.lock() {
                                            logs.clear();
                                        }
                                        if let Ok(mut buf) =
                                            rust_browser::js_engine::CONSOLE_LOG_BUFFER.lock()
                                        {
                                            buf.clear();
                                        }
                                    }
                                });
                                let logs = get_log_messages();
                                for log in logs.iter().rev().take(100) {
                                    // 根据消息类型着色
                                    let color = if log.contains("[console.error")
                                        || log.contains("错误")
                                        || log.contains("失败")
                                    {
                                        egui::Color32::from_rgb(0xCC, 0x33, 0x33)
                                    } else if log.contains("[console.warn") || log.contains("警告")
                                    {
                                        egui::Color32::from_rgb(0xCC, 0x88, 0x00)
                                    } else if log.contains("[console.") {
                                        egui::Color32::from_rgb(0x33, 0x66, 0xCC)
                                    } else {
                                        egui::Color32::LIGHT_GRAY
                                    };
                                    ui.colored_label(color, log);
                                }
                            }
                            "dom" => {
                                // DOM 检查器 — 显示当前页面的 DOM 结构
                                ui.heading("DOM 树");
                                ui.horizontal(|ui| {
                                    if ui.button("🔄 刷新").clicked() {
                                        // 触发 DOM 刷新
                                        add_log_message("DevTools: DOM 树刷新请求".to_string());
                                    }
                                    ui.label("当前页面 DOM 结构：");
                                });
                                ui.separator();
                                // 尝试获取 DOM 信息
                                if let Some(ref host) = self.browser_host {
                                    if let Some(renderer) = host.active_renderer() {
                                        let url = &renderer.url;
                                        let title = renderer.title.as_deref().unwrap_or("无标题");
                                        ui.label(format!("📄 {} ({})", title, url));

                                        // 显示 DOM 节点统计（从渲染器获取）
                                        // 当前通过日志方式获取——实际渲染器的 DOM 信息
                                        // 需要通过 IPC 获取，这里先显示可获取的信息
                                        ui.label(format!("渲染器 ID: #{}", renderer.id));
                                        ui.label(format!(
                                            "视口: {}x{}",
                                            renderer.width, renderer.height
                                        ));
                                        ui.colored_label(
                                            egui::Color32::YELLOW,
                                            "💡 完整 DOM 树需要通过 IPC 请求渲染器",
                                        );
                                    } else {
                                        ui.label("(没有活跃渲染器)");
                                    }
                                } else {
                                    ui.label("(浏览器进程未启动)");
                                }
                            }
                            "network" => {
                                // 网络请求面板
                                ui.heading("网络请求");
                                ui.label("HTTP 请求记录：");
                                ui.separator();
                                ui.colored_label(
                                    egui::Color32::YELLOW,
                                    "💡 网络请求日志将通过 IPC 捕获（开发中）",
                                );
                                // 显示最近导航记录
                                for (i, url) in self.nav_history.iter().enumerate() {
                                    let marker = if i == self.nav_history_pos as usize {
                                        "→ "
                                    } else {
                                        "  "
                                    };
                                    ui.label(format!("{}{}", marker, url));
                                }
                            }
                            "info" => {
                                // 系统信息面板
                                ui.heading("浏览器信息");
                                ui.separator();
                                ui.label(format!("🧩 版本: {}", env!("CARGO_PKG_VERSION")));
                                ui.label(format!("📏 视口: {}x{}", self.width, self.height));
                                ui.label(format!(
                                    "📑 标签页数: {}",
                                    self.browser_host
                                        .as_ref()
                                        .map(|h| h.renderer_count())
                                        .unwrap_or(0)
                                ));
                                ui.label(format!("📜 导航历史: {} 条", self.nav_history.len()));
                                ui.label(format!("🔖 书签: {} 个", self.bookmarks.len()));
                                ui.separator();
                                ui.label("构建信息：");
                                ui.label(format!(
                                    "  编译配置: {}",
                                    if cfg!(debug_assertions) {
                                        "Debug"
                                    } else {
                                        "Release"
                                    }
                                ));
                                ui.label(format!("  操作系统: {}", std::env::consts::OS));
                                ui.label(format!(
                                    "  Boa JS: {}",
                                    cfg!(feature = "boa").to_string()
                                ));
                                ui.label(format!("  GUI: {}", cfg!(feature = "gui").to_string()));
                            }
                            _ => {
                                ui.label("未知标签页");
                            }
                        }
                    });
            });
        }

        // ═══════════════════════════════════════════
        // 页面内容：在 CentralPanel 中显示渲染帧
        // ═══════════════════════════════════════════

        egui::CentralPanel::default().show(ctx, |ui| {
            // 如果有错误消息，显示错误页面
            if let Some(error_msg) = self.error_message.clone() {
                ui.vertical_centered(|ui| {
                    ui.add_space(ui.available_height() / 3.0);
                    ui.heading("⚠ 加载失败");
                    ui.label(&error_msg);
                    if ui.button("重试").clicked() {
                        let url = self.url_input.clone();
                        self.navigate(&url);
                    }
                });
                return;
            }

            // 如果正在加载，显示加载动画
            if self.is_loading {
                ui.vertical_centered(|ui| {
                    ui.add_space(ui.available_height() / 3.0);
                    ui.spinner();
                    ui.label("正在加载...");
                });
                return;
            }

            // 显示渲染帧
            if let Some((w, h, ref rgba)) = self.rgba_pixels {
                if w > 0 && h > 0 && !rgba.is_empty() {
                    let image = egui::ColorImage::from_rgba_unmultiplied(
                        [w as usize, h as usize],
                        rgba.as_slice(),
                    );
                    let texture = ui.ctx().load_texture(
                        "browser_content",
                        image,
                        egui::TextureOptions::NEAREST,
                    );
                    let avail = ui.available_size();
                    
                    // 计算合适的缩放比例，让内容填充空间但保持比例
                    let scale_x = avail.x / w as f32;
                    let scale_y = avail.y / h as f32;
                    let scale = scale_x.min(scale_y).min(2.0); // 最大放大2倍
                    
                    let img_size = egui::vec2(w as f32 * scale, h as f32 * scale);
                    
                    // 使用 Frame 来创建一个填充背景和内容的区域
                    egui::Frame::canvas(ui.style())
                        .outer_margin(0.0)
                        .inner_margin(0.0)
                        .fill(egui::Color32::BLACK)
                        .show(ui, |ui| {
                            // 让内容居中显示
                            ui.allocate_ui_with_layout(
                                avail,
                                egui::Layout::centered_and_justified(egui::Direction::TopDown),
                                |ui| {
                                    ui.add(
                                        egui::Image::new(&texture)
                                            .fit_to_exact_size(img_size)
                                            .sense(egui::Sense::click())
                                    );
                                }
                            );
                        });
                } else {
                    ui.vertical_centered(|ui| {
                        ui.label("(无内容)");
                    });
                }
            } else {
                ui.vertical_centered(|ui| {
                    ui.add_space(ui.available_height() / 3.0);
                    ui.label("等待渲染...");
                });
            }
        });

        // ── 键盘事件：Enter 导航 ──
        let enter_pressed = ctx.input(|i| {
            i.events.iter().any(|e| {
                matches!(
                    e,
                    egui::Event::Key {
                        key: egui::Key::Enter,
                        pressed: true,
                        ..
                    }
                )
            })
        });
        if enter_pressed && !self.url_input.is_empty() {
            let url = self.url_input.clone();
            if !url.starts_with("http://")
                && !url.starts_with("https://")
                && !url.starts_with("file://")
                && url != "about:blank"
            {
                self.url_input = format!("https://{}", url);
            }
            self.navigate(&self.url_input.clone());
        }
    }
}

use std::io::Cursor;

/// 将 RGBA 像素数据转换为 PNG 格式
fn rgba_to_png(width: u32, height: u32, rgba_data: &[u8]) -> Result<Vec<u8>, String> {
    let image: ImageBuffer<Rgba<u8>, Vec<u8>> = ImageBuffer::from_raw(width, height, rgba_data.to_vec())
        .ok_or_else(|| "无法创建图像缓冲区".to_string())?;
    
    let mut png_data = Vec::new();
    let mut cursor = Cursor::new(&mut png_data);
    image.write_to(&mut cursor, ImageFormat::Png)
        .map_err(|e| format!("PNG 编码失败: {}", e))?;
    
    Ok(png_data)
}

fn run_screenshot_mode(args: &Args) -> Result<(), String> {
    println!("Rust Browser (Chrome 架构) 截图模式");
    println!("目标 URL: {}", args.url);

    let mut host = BrowserProcessHost::new();
    let renderer_id = host.spawn_renderer(&args.url, args.width, args.height)?;
    println!("✓ 渲染器进程 #{} 已启动", renderer_id);

    // 等待渲染结果（跳过 loading 帧）
    let start = Instant::now();
    loop {
        // 优先接收 RGBA 格式（内容帧使用 RGBA）
        if let Some(result) = host.try_receive_rgba_result() {
            println!("✓ 收到渲染帧: {}x{} (RGBA, is_loading={})", result.width, result.height, result.is_loading);

            // 跳过 loading 帧，等待内容帧
            if !result.is_loading {
                if let Some(output_path) = &args.output {
                    println!("正在保存截图到: {:?}", output_path);
                    // 将 RGBA 转换为 PNG
                    let png_data = rgba_to_png(result.width, result.height, &result.rgba_data)
                        .map_err(|e| format!("PNG 编码失败: {}", e))?;
                    std::fs::write(output_path, &png_data)
                        .map_err(|e| format!("保存文件失败: {}", e))?;
                    println!("✓ 截图保存成功");
                }
                return Ok(());
            }
        }

        // 也检查 PNG 格式（loading 帧使用 PNG）
        if let Some(result) = host.try_receive_result() {
            println!("✓ 收到渲染帧: {}x{} (PNG, is_loading={})", result.width, result.height, result.is_loading);
        }

        if start.elapsed().as_secs() > 30 {
            return Err("等待渲染结果超时".to_string());
        }

        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}

/// 无头模式入口：没有 GUI，直接渲染并输出
#[cfg(not(feature = "gui"))]
fn main() -> Result<(), String> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .format_timestamp_secs()
        .init();

    info!("Rust Browser (Headless 模式) 启动");
    println!("Rust Browser (Headless 模式)");

    let args = Args::parse();
    println!("目标 URL: {}", args.url);

    let mut host = BrowserProcessHost::new();
    let renderer_id = host.spawn_renderer(&args.url, args.width, args.height)?;
    println!("✓ 渲染器进程 #{} 已启动", renderer_id);

    // 等待渲染结果（跳过 loading 帧）
    let start = Instant::now();
    loop {
        if let Some(result) = host.try_receive_result() {
            println!("✓ 收到渲染帧: {}x{} (is_loading={})", result.width, result.height, result.is_loading);

            // 跳过 loading 帧，等待内容帧
            if !result.is_loading {
                if let Some(output_path) = &args.output {
                    println!("正在保存截图到: {:?}", output_path);
                    std::fs::write(output_path, &result.png_data)
                        .map_err(|e| format!("保存文件失败: {}", e))?;
                    println!("✓ 截图保存成功");
                }

                return Ok(());
            }
        }

        if start.elapsed().as_secs() > 30 {
            return Err("等待渲染结果超时".to_string());
        }

        std::thread::sleep(Duration::from_millis(100));
    }
}

#[cfg(feature = "gui")]
fn main() -> Result<(), eframe::Error> {
    // 初始化日志系统
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .format_timestamp_secs()
        .init();

    info!("Rust Browser (Chrome 多进程架构) 启动");
    add_log_message("Rust Browser (Chrome 多进程架构) 启动".to_string());

    // 打印架构启动信息
    add_log_message("─".repeat(50));
    add_log_message("进程模型: BrowserProcess + RendererProcess (多线程)".to_string());
    add_log_message("IPC 协议: Mojo 风格 MessagePipe + Interface Binding".to_string());
    add_log_message("任务调度: TaskQueue (Work-Stealing ThreadPool)".to_string());
    add_log_message("─".repeat(50));

    // 解析命令行参数
    let args = Args::parse();

    // 截图模式
    if args.output.is_some() {
        if let Err(e) = run_screenshot_mode(&args) {
            eprintln!("{}", e);
        }
        return Ok(());
    }

    // 窗口模式
    add_log_message("进入窗口模式（winit + eframe 混合架构）".to_string());

    // 获取屏幕尺寸以设置工具栏宽度
    #[allow(unused_variables)]
    let screen_width = 1920.0_f32; // 默认值，首帧时通过 egui 获取实际值

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([800.0, 600.0])
            .with_resizable(true)
            .with_decorations(true)
            .with_title("Rust Browser")
            .with_visible(true),
        ..Default::default()
    };

    add_log_message(format!("目标 URL: {}", args.url));

    let app = BrowserApp::new(args.url, args.width, args.height);

    add_log_message("正在创建 eframe 工具栏窗口...".to_string());
    info!("开始运行 eframe::run_native");

    let result = eframe::run_native(
        "Rust Browser Toolbar",
        options,
        Box::new(|cc| {
            let mut fonts = egui::FontDefinitions::default();

            fonts.font_data.insert(
                "china_font".to_owned(),
                egui::FontData::from_static(include_bytes!("C:/Windows/Fonts/msyh.ttc")).into(),
            );

            fonts
                .families
                .entry(egui::FontFamily::Proportional)
                .or_default()
                .insert(0, "china_font".to_owned());

            fonts
                .families
                .entry(egui::FontFamily::Monospace)
                .or_default()
                .push("china_font".to_owned());

            cc.egui_ctx.set_fonts(fonts);
            info!("中文字体已加载");

            Ok(Box::new(app))
        }),
    );

    if let Err(e) = result {
        error!("窗口运行失败: {}", e);
        eprintln!("窗口运行失败: {}", e);
    }
    info!("程序退出");
    Ok(())
}
