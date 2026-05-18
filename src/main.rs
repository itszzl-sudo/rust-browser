//! Rust Browser - 主程序入口
//!
//! Chrome 多进程架构浏览器
//! - Browser Process (主线程/UI)
//! - Renderer Processes (每个标签页独立线程)
//! - Mojo IPC 通信
//! - Task Queue 任务调度

use clap::Parser;
use eframe::egui;
use log::{error, info};
use rust_browser::browser_process::host::BrowserProcessHost;
use rust_browser::browser_process::interfaces::{InputEvent, RenderResultMessage};
use rust_browser::task_queue::task::TaskTraits;
use rust_browser::task_queue::GLOBAL_SCHEDULER;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Instant;

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
const LOAD_TIMEOUT_SECS: u64 = 5;

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
}

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
            initial_url,
            is_loading: false,
            first_frame: true,
            load_start_time: None,
            auto_load_pending: true,
            width,
            height,
        }
    }

    /// 通过 IPC 从渲染器进程获取最新帧
    fn refresh_from_renderer(&mut self) {
        if let Some(ref mut host) = self.browser_host {
            // 从 Mojo IPC 管道接收渲染结果
            if let Some(result) = host.try_receive_result() {
                add_log_message(format!("收到渲染帧: {}x{}", result.width, result.height));

                match image::load_from_memory(&result.png_data) {
                    Ok(img) => {
                        let rgba = img.to_rgba8();
                        let (w, h) = rgba.dimensions();
                        let pixels: Vec<u8> = rgba.into_raw();
                        self.image_data = Some(egui::ColorImage::from_rgba_unmultiplied(
                            [w as usize, h as usize],
                            &pixels,
                        ));
                        self.error_message = None;
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
        add_log_message(format!("Mojo IPC 导航到: {}", url));
        self.is_loading = true;
        self.error_message = None;
        self.load_start_time = Some(Instant::now());

        if let Some(ref host) = self.browser_host {
            match host.navigate(url) {
                Ok(_) => {
                    self.url_input = url.to_string();
                    add_log_message("导航消息已通过 IPC 发送到渲染器".to_string());

                    // 通过 TaskQueue 延迟等待渲染结果
                    let scheduler = &*GLOBAL_SCHEDULER;
                    let poll_interval = std::time::Duration::from_millis(100);
                    let start = Instant::now();

                    // 轮询等待渲染结果（最多 10 秒）
                    while start.elapsed().as_secs() < LOAD_TIMEOUT_SECS {
                        // 在浏览器进程中运行 main thread 任务
                        scheduler.run_main_tasks();

                        // 使用 host 的可变引用接收结果
                        // （通过内部可变性处理）
                        if let Some(ref mut host) = self.browser_host {
                            if let Some(result) = host.try_receive_result() {
                                add_log_message("通过 IPC 接收到渲染结果".to_string());
                                match image::load_from_memory(&result.png_data) {
                                    Ok(img) => {
                                        let rgba = img.to_rgba8();
                                        let (w, h) = rgba.dimensions();
                                        let pixels: Vec<u8> = rgba.into_raw();
                                        self.image_data =
                                            Some(egui::ColorImage::from_rgba_unmultiplied(
                                                [w as usize, h as usize],
                                                &pixels,
                                            ));
                                        self.error_message = None;
                                        if let Some(ref title) = result.title {
                                            add_log_message(format!("页面标题: {}", title));
                                        }
                                    }
                                    Err(e) => {
                                        self.error_message = Some(format!("图像解析失败: {}", e));
                                    }
                                }
                                self.is_loading = false;
                                return;
                            }
                        }

                        std::thread::sleep(poll_interval);
                    }

                    add_log_message("导航超时：渲染器未返回结果".to_string());
                    self.error_message = Some("页面加载超时".to_string());
                    self.is_loading = false;
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
}

impl eframe::App for BrowserApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // 首帧初始化
        if self.first_frame {
            self.first_frame = false;
            add_log_message("窗口已显示".to_string());
            add_log_message(format!(
                "将在 {} 秒后自动加载: {}",
                LOAD_TIMEOUT_SECS, self.initial_url
            ));
            self.load_start_time = Some(Instant::now());
            info!("首帧渲染完成，窗口已显示");

            // 立即尝试接收首次渲染结果
            self.refresh_from_renderer();
        }

        // 自动加载
        if self.auto_load_pending {
            if let Some(start_time) = self.load_start_time {
                let elapsed = start_time.elapsed().as_secs();
                if elapsed >= LOAD_TIMEOUT_SECS {
                    add_log_message("自动加载触发...".to_string());
                    self.auto_load_pending = false;
                    let url_to_load = self.initial_url.clone();
                    self.navigate(&url_to_load);
                }
            }
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

            // 加载中仍然尝试接收渲染结果
            self.refresh_from_renderer();
        }

        // 在每一帧运行 TaskQueue 的 main thread 任务
        GLOBAL_SCHEDULER.run_main_tasks();

        ctx.set_visuals(egui::Visuals::dark());

        // 顶部标签页栏 + URL 输入
        egui::TopBottomPanel::top("tab_bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                // 显示所有标签页
                if let Some(ref host) = self.browser_host {
                    let renderer_ids = host.renderer_ids();
                    for &tab_id in &renderer_ids {
                        let is_active = Some(tab_id) == self.active_renderer_id;
                        let label = if let Some(renderer) = host.get_renderer(tab_id) {
                            if let Some(ref title) = renderer.title {
                                format!("{}", title)
                            } else {
                                let url = &renderer.url;
                                let short_url = url
                                    .trim_start_matches("https://")
                                    .trim_start_matches("http://")
                                    .trim_end_matches('/');
                                if short_url.is_empty() {
                                    format!("标签 #{}", tab_id)
                                } else {
                                    format!("{} #{}", short_url, tab_id)
                                }
                            }
                        } else {
                            format!("标签 #{}", tab_id)
                        };

                        let btn = if is_active {
                            ui.selectable_label(true, &label)
                        } else {
                            ui.selectable_label(false, &label)
                        };

                        if btn.clicked() && !is_active {
                            add_log_message(format!("切换到标签页 #{}", tab_id));
                            if let Some(ref mut host) = self.browser_host {
                                host.switch_to_tab(tab_id);
                                self.active_renderer_id = Some(tab_id);
                                self.refresh_from_renderer();
                            }
                        }

                        // 关闭标签页按钮（保留至少一个）
                        if renderer_ids.len() > 1 {
                            if ui.button("✕").clicked() {
                                add_log_message(format!("关闭标签页 #{}", tab_id));
                                if let Some(ref mut host) = self.browser_host {
                                    host.close_renderer(tab_id);
                                    self.active_renderer_id = host.active_renderer().map(|r| r.id);
                                    self.refresh_from_renderer();
                                }
                                ui.close_menu();
                            }
                        }
                    }

                    // 新建标签页按钮
                    if ui.button("+").clicked() {
                        add_log_message("新建空白标签页".to_string());
                        if let Some(ref mut host) = self.browser_host {
                            match host.spawn_renderer("about:blank", self.width, self.height) {
                                Ok(new_id) => {
                                    self.active_renderer_id = Some(new_id);
                                    self.refresh_from_renderer();
                                    add_log_message(format!("新标签页 #{} 已创建", new_id));
                                }
                                Err(e) => {
                                    add_log_message(format!("创建标签页失败: {}", e));
                                }
                            }
                        }
                    }
                }
            });
        });

        // 日志面板
        egui::TopBottomPanel::bottom("log_panel").show(ctx, |ui| {
            ui.heading("Chrome 多进程 IPC 日志");
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .stick_to_bottom(true)
                .show(ui, |ui| {
                    let logs = get_log_messages();
                    for log in logs.iter().rev().take(20) {
                        ui.label(log.clone());
                    }
                });
        });

        // 中央面板 - 页面显示区域
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.set_min_size(egui::vec2(800.0, 600.0));

            if let Some(error) = &self.error_message {
                ui.centered_and_justified(|ui| {
                    ui.label(egui::RichText::new(error).color(egui::Color32::RED).size(18.0));
                });
            } else if let Some(image_data) = &self.image_data {
                let texture = ctx.load_texture(
                    "browser_content",
                    image_data.clone(),
                    egui::TextureOptions::default(),
                );

                ui.centered_and_justified(|ui| {
                    let (rect, _) = ui.allocate_exact_size(
                        egui::vec2(ui.available_width(), ui.available_height()),
                        egui::Sense::click(),
                    );

                    // 检测鼠标点击并通过 IPC 发送到渲染器
                    let click_resp = ui.interact(rect, ui.next_auto_id(), egui::Sense::click());
                    if click_resp.clicked_by(egui::PointerButton::Primary) {
                        if let Some(pos) = ctx.pointer_interact_pos() {
                            if let Some(ref host) = self.browser_host {
                                add_log_message(format!("页面点击: ({:.0}, {:.0})", pos.x, pos.y));
                                let _ = host.send_input(
                                    rust_browser::browser_process::interfaces::InputEvent::MouseClick {
                                        x: pos.x as f32,
                                        y: pos.y as f32,
                                        button: 0,
                                    },
                                );
                            }
                        }
                    }

                    // 显示图像
                    ui.put_image(rect, &texture);
                });
            } else if self.auto_load_pending || self.is_loading {
                ui.centered_and_justified(|ui| {
                    ui.label("Rust Browser (Chrome 架构) - 初始化中...");
                });
            } else {
                ui.centered_and_justified(|ui| {
                    ui.label("Rust Browser (Chrome 架构)");
                });
            }

            // 显示架构信息
            ui.horizontal(|ui| {
                ui.label("架构:");
                ui.colored_label(
                    egui::Color32::GREEN,
                    format!(
                        "BrowserProcess(1) ↔ RendererProcess({}) via Mojo IPC | TaskQueue({} threads)",
                        self.browser_host
                            .as_ref()
                            .map(|h| h.renderer_count())
                            .unwrap_or(0),
                        std::thread::available_parallelism()
                            .map(|n| n.get())
                            .unwrap_or(4),
                    ),
                );
            });
        });
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
