//! Rust Browser - 主程序入口
//!
//! Chrome 多进程架构浏览器
//! - Browser Process (主线程/UI)
//! - Renderer Processes (每个标签页独立线程)
//! - Mojo IPC 通信
//! - Task Queue 任务调度

#![cfg_attr(not(feature = "gui"), allow(dead_code, unused_imports))]

use clap::Parser;
use log::{error, info};
use rust_browser::browser_process::host::BrowserProcessHost;
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
        if logs.len() > 100 {
            logs.remove(0);
        }
    }
}

fn get_log_messages() -> Vec<String> {
    LOG_MESSAGES
        .lock()
        .map(|logs| logs.clone())
        .unwrap_or_default()
}

const DEFAULT_URL: &str = "https://www.baidu.com";
const LOAD_TIMEOUT_SECS: u64 = 30;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[arg(default_value = DEFAULT_URL)]
    url: String,

    #[arg(short, long)]
    output: Option<PathBuf>,

    #[arg(long, default_value_t = 1280)]
    width: u32,

    #[arg(long, default_value_t = 720)]
    height: u32,

    #[arg(short, long)]
    debug: bool,
}

/// Chrome 风格的多进程浏览器应用
#[cfg(feature = "gui")]
struct BrowserApp {
    /// 浏览器进程宿主（管理渲染器进程）
    browser_host: Option<BrowserProcessHost>,
    /// 当前活跃渲染器 ID
    active_renderer_id: Option<u64>,
    /// URL 输入框内容
    url_input: String,
    /// 渲染的页面图像
    image_data: Option<egui::ColorImage>,
    /// 错误信息
    error_message: Option<String>,
    /// 初始 URL
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
    /// 书签列表
    bookmarks: Vec<(String, String)>,
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
            image_data: None,
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
        }
    }

    /// 通过 IPC 从渲染器进程获取最新帧
    fn refresh_from_renderer(&mut self) {
        if let Some(ref mut host) = self.browser_host {
            // 从 Mojo IPC 管道接收渲染结果
            if let Some(result) = host.try_receive_result() {
                add_log_message(format!(
                    "收到渲染帧: {}x{} ({} bytes)",
                    result.width,
                    result.height,
                    result.png_data.len()
                ));

                match image::load_from_memory(&result.png_data) {
                    Ok(img) => {
                        let rgba = img.to_rgba8();
                        let (w, h) = rgba.dimensions();
                        add_log_message(format!("图片解码成功: {}x{}", w, h));
                        let pixels: Vec<u8> = rgba.into_raw();
                        self.image_data = Some(egui::ColorImage::from_rgba_unmultiplied(
                            [w as usize, h as usize],
                            &pixels,
                        ));
                        self.error_message = None;
                        self.is_loading = false;
                        if let Some(ref title) = result.title {
                            add_log_message(format!("页面标题: {}", title));
                        }
                    }
                    Err(e) => {
                        let err = format!("图像解析失败: {}", e);
                        self.error_message = Some(err.clone());
                        add_log_message(err);
                    }
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
}

#[cfg(feature = "gui")]
impl eframe::App for BrowserApp {
    fn ui(&mut self, ctx: &mut egui::Ui, _frame: &mut eframe::Frame) {
        // ── 首帧初始化 ──
        if self.first_frame {
            self.first_frame = false;
            add_log_message("窗口已显示，等待渲染器加载首页...".to_string());
            info!("首帧渲染完成，窗口已显示");
            self.is_loading = true;
            self.load_start_time = Some(Instant::now());
        }

        // 每帧尝试接收渲染结果
        self.refresh_from_renderer();

        // 清除首帧自动加载标记（收到渲染帧后设置为false）
        if self.image_data.is_some() {
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
        // 顶部面板
        // ═══════════════════════════════════════════

        let tabs_data: Vec<(u64, String, bool)> = if let Some(ref host) = self.browser_host {
            let active_id = self.active_renderer_id;
            host.renderer_ids()
                .iter()
                .map(|&tab_id| {
                    let is_active = Some(tab_id) == active_id;
                    let label = if let Some(renderer) = host.get_renderer(tab_id) {
                        if let Some(ref title) = renderer.title {
                            if title.is_empty() {
                                format!("Tab #{}", tab_id)
                            } else {
                                title.clone()
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
        egui::TopBottomPanel::top("tab_bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                for (tab_id, label, is_active) in &tabs_data {
                    let resp = if *is_active {
                        ui.selectable_label(true, label)
                    } else {
                        ui.selectable_label(false, label)
                    };
                    if resp.clicked() && !is_active {
                        add_log_message(format!("切换到 Tab #{}", tab_id));
                        if let Some(ref mut host) = self.browser_host {
                            host.switch_to_tab(*tab_id);
                            self.active_renderer_id = Some(*tab_id);
                            self.refresh_from_renderer();
                        }
                    }
                    if renderer_count > 1 {
                        if ui.button("x").clicked() {
                            add_log_message(format!("关闭 Tab #{}", tab_id));
                            if let Some(ref mut host) = self.browser_host {
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

        // ── 导航栏 ──
        egui::TopBottomPanel::top("nav_bar").show(ctx, |ui| {
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

                // 书签切换按钮
                let bk_label = if self.show_bookmarks {
                    "▼BK"
                } else {
                    "▶BK"
                };
                if ui.button(bk_label).clicked() {
                    self.show_bookmarks = !self.show_bookmarks;
                }

                // 调试日志切换按钮
                let log_label = if self.show_log_panel {
                    "▼LOG"
                } else {
                    "▶LOG"
                };
                if ui.button(log_label).clicked() {
                    self.show_log_panel = !self.show_log_panel;
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
        if self.show_bookmarks && !self.bookmarks.is_empty() {
            let bookmarks = self.bookmarks.clone();
            egui::TopBottomPanel::top("bookmark_bar").show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    for (name, url) in &bookmarks {
                        if ui.link(name).clicked() {
                            self.url_input = url.clone();
                            self.navigate(url);
                            self.show_bookmarks = false; // 点击书签后自动隐藏书签栏
                        }
                    }
                });
            });
        }

        // ═══════════════════════════════════════════
        // 底部面板：调试日志
        // ═══════════════════════════════════════════
        if self.show_log_panel {
            egui::TopBottomPanel::bottom("log_panel").show(ctx, |ui| {
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
        // 中央面板：页面显示
        // ═══════════════════════════════════════════
        egui::CentralPanel::default().show(ctx, |ui| {
            // 自动撑满可用空间
            let available = ui.available_size();
            ui.set_min_size(available);

            if let Some(error) = &self.error_message {
                ui.centered_and_justified(|ui| {
                    ui.label(
                        egui::RichText::new(error)
                            .color(egui::Color32::RED)
                            .size(18.0),
                    );
                });
            } else if let Some(image_data) = &self.image_data {
                let img_size = egui::vec2(image_data.width() as f32, image_data.height() as f32);
                let texture = ctx.load_texture(
                    "browser_content",
                    image_data.clone(),
                    egui::TextureOptions::default(),
                );

                let available = ui.available_size();
                let (rect, _) = ui.allocate_exact_size(available, egui::Sense::click());

                // 等比例缩放显示，居中
                let scale = (available.x / img_size.x)
                    .min(available.y / img_size.y)
                    .min(1.0);
                let scaled_size = img_size * scale;
                let offset = egui::vec2(
                    (available.x - scaled_size.x).max(0.0) * 0.5,
                    (available.y - scaled_size.y).max(0.0) * 0.5,
                );
                let image_rect = egui::Rect::from_min_size(rect.min + offset, scaled_size);

                let click_resp = ui.interact(rect, ui.next_auto_id(), egui::Sense::click());
                if click_resp.clicked_by(egui::PointerButton::Primary) {
                    if let Some(pos) = ctx.pointer_interact_pos() {
                        // 将点击坐标映射到图像坐标系
                        let img_x =
                            ((pos.x - image_rect.min.x) / scaled_size.x * img_size.x).max(0.0);
                        let img_y =
                            ((pos.y - image_rect.min.y) / scaled_size.y * img_size.y).max(0.0);
                        if let Some(ref host) = self.browser_host {
                            add_log_message(format!("页面点击: ({:.0}, {:.0})", img_x, img_y));
                            let _ = host.send_input(
                                rust_browser::browser_process::interfaces::InputEvent::MouseClick {
                                    x: img_x,
                                    y: img_y,
                                    button: 0,
                                },
                            );
                        }
                    }
                }

                ui.put(image_rect, egui::Image::new(&texture));
            } else if self.auto_load_pending || self.is_loading {
                ui.centered_and_justified(|ui| {
                    ui.label("Rust Browser (Chrome 架构) - 初始化中...");
                });
            } else {
                ui.centered_and_justified(|ui| {
                    ui.heading("Rust Browser");
                    ui.label("输入 URL 并按 Enter 开始浏览");
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

fn run_screenshot_mode(args: &Args) -> Result<(), String> {
    println!("Rust Browser (Chrome 架构) 截图模式");
    println!("目标 URL: {}", args.url);

    let mut host = BrowserProcessHost::new();
    let renderer_id = host.spawn_renderer(&args.url, args.width, args.height)?;
    println!("✓ 渲染器进程 #{} 已启动", renderer_id);

    // 等待渲染结果
    let start = Instant::now();
    loop {
        if let Some(result) = host.try_receive_result() {
            println!("✓ 收到渲染帧: {}x{}", result.width, result.height);

            if let Some(output_path) = &args.output {
                println!("正在保存截图到: {:?}", output_path);
                std::fs::write(output_path, &result.png_data)
                    .map_err(|e| format!("保存文件失败: {}", e))?;
                println!("✓ 截图保存成功");
            }
            return Ok(());
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

    // 等待渲染结果
    let start = Instant::now();
    loop {
        if let Some(result) = host.try_receive_result() {
            println!("✓ 收到渲染帧: {}x{}", result.width, result.height);

            if let Some(output_path) = &args.output {
                println!("正在保存截图到: {:?}", output_path);
                std::fs::write(output_path, &result.png_data)
                    .map_err(|e| format!("保存文件失败: {}", e))?;
                println!("✓ 截图保存成功");
            }

            return Ok(());
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
    add_log_message("进入窗口模式".to_string());

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([args.width as f32, args.height as f32])
            .with_min_inner_size([400.0, 300.0])
            .with_title("Rust Browser (Chrome 架构)")
            .with_resizable(true)
            .with_maximized(false)
            .with_visible(true),
        ..Default::default()
    };

    add_log_message(format!("目标 URL: {}", args.url));
    add_log_message(format!("窗口尺寸: {}x{}", args.width, args.height));

    let app = BrowserApp::new(args.url, args.width, args.height);

    add_log_message("正在创建窗口...".to_string());
    info!("开始运行 eframe::run_native");

    let result = eframe::run_native(
        "Rust Browser (Chrome 架构)",
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
