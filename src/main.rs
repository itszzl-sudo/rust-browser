//! Rust Browser - 主程序入口
//!
//! 基于 egui 的图形化浏览器

use clap::Parser;
use eframe::egui;
use rust_browser::Browser;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Instant;
use log::{info, error};

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
    LOG_MESSAGES.lock().map(|logs| logs.clone()).unwrap_or_default()
}

fn clear_log_messages() {
    if let Ok(mut logs) = LOG_MESSAGES.lock() {
        logs.clear();
    }
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

struct BrowserApp {
    browser: Option<Browser>,
    url_input: String,
    image_data: Option<egui::ColorImage>,
    error_message: Option<String>,
    viewport_size: (u32, u32),
    initial_url: String,
    is_loading: bool,
    first_frame: bool,
    load_start_time: Option<Instant>,
    auto_load_pending: bool,
}

impl BrowserApp {
    fn new(initial_url: String, width: u32, height: u32) -> Self {
        let viewport_size = (width, height);
        add_log_message("正在初始化浏览器...".to_string());

        let browser = match Browser::new() {
            Ok(b) => {
                add_log_message("浏览器创建成功".to_string());
                Some(b.with_viewport(width, height))
            }
            Err(e) => {
                let err_msg = format!("创建浏览器失败: {}", e);
                add_log_message(err_msg.clone());
                eprintln!("{}", err_msg);
                return Self {
                    browser: None,
                    url_input: initial_url.clone(),
                    image_data: None,
                    error_message: Some(err_msg),
                    viewport_size,
                    initial_url,
                    is_loading: false,
                    first_frame: true,
                    load_start_time: None,
                    auto_load_pending: false,
                };
            }
        };

        add_log_message(format!("窗口尺寸: {}x{}", width, height));

        Self {
            browser,
            url_input: initial_url.clone(),
            image_data: None,
            error_message: None,
            viewport_size,
            initial_url,
            is_loading: false,
            first_frame: true,
            load_start_time: None,
            auto_load_pending: true,
        }
    }

    fn refresh_image(&mut self) {
        if let Some(browser) = self.browser.as_mut() {
            add_log_message("正在渲染页面...".to_string());
            match browser.render_full() {
                Ok(png_data) => {
                    add_log_message("渲染成功，正在加载图像...".to_string());
                    match image::load_from_memory(&png_data) {
                        Ok(img) => {
                            let rgba = img.to_rgba8();
                            let (w, h) = rgba.dimensions();
                            let pixels: Vec<u8> = rgba.into_raw();
                            self.image_data = Some(egui::ColorImage::from_rgba_unmultiplied(
                                [w as usize, h as usize],
                                &pixels,
                            ));
                            self.error_message = None;
                            add_log_message(format!("图像加载成功: {}x{}", w, h));
                        }
                        Err(e) => {
                            let err = format!("图像解析失败: {}", e);
                            add_log_message(err.clone());
                            self.error_message = Some(err);
                        }
                    }
                }
                Err(e) => {
                    let err = format!("渲染失败: {}", e);
                    add_log_message(err.clone());
                    self.error_message = Some(err);
                }
            }
        }
    }

    fn navigate(&mut self, url: &str) {
        add_log_message(format!("正在导航到: {}", url));
        self.is_loading = true;

        if let Some(browser) = self.browser.as_mut() {
            match browser.navigate(url) {
                Ok(_) => {
                    self.url_input = url.to_string();
                    add_log_message("导航成功，正在刷新图像...".to_string());
                    self.refresh_image();
                    self.is_loading = false;
                }
                Err(e) => {
                    let err = format!("导航失败: {}", e);
                    add_log_message(err.clone());
                    self.error_message = Some(err);
                    self.is_loading = false;
                }
            }
        } else {
            add_log_message("错误: 浏览器未初始化".to_string());
            self.is_loading = false;
        }
    }

    fn go_back(&mut self) {
        if let Some(browser) = self.browser.as_mut() {
            if browser.go_back().is_ok() {
                self.url_input = browser.url().to_string();
                self.refresh_image();
            }
        }
    }

    fn go_forward(&mut self) {
        if let Some(browser) = self.browser.as_mut() {
            if browser.go_forward().is_ok() {
                self.url_input = browser.url().to_string();
                self.refresh_image();
            }
        }
    }

    fn reload(&mut self) {
        if let Some(browser) = self.browser.as_mut() {
            if browser.reload().is_ok() {
                self.refresh_image();
            }
        }
    }
}

impl eframe::App for BrowserApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if self.first_frame {
            self.first_frame = false;
            add_log_message("窗口已显示".to_string());
            add_log_message(format!("将在 {} 秒后自动加载: {}", LOAD_TIMEOUT_SECS, self.initial_url));
            self.load_start_time = Some(Instant::now());
            info!("首帧渲染完成，窗口应该已显示");
        }

        if self.auto_load_pending {
            if let Some(start_time) = self.load_start_time {
                let elapsed = start_time.elapsed().as_secs();
                if elapsed >= LOAD_TIMEOUT_SECS {
                    add_log_message("开始加载网页...".to_string());
                    self.auto_load_pending = false;
                    let url_to_load = self.initial_url.clone();
                    self.navigate(&url_to_load);
                }
            }
        }

        if self.is_loading {
            if let Some(start_time) = self.load_start_time {
                let elapsed = start_time.elapsed().as_secs();
                if elapsed >= LOAD_TIMEOUT_SECS {
                    add_log_message("加载超时！网络请求超过5秒".to_string());
                    self.is_loading = false;
                    self.error_message = Some("网络连接超时，请检查网络后重试".to_string());
                }
            }
        }

        ctx.set_visuals(egui::Visuals::dark());

        egui::TopBottomPanel::top("address_bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                if ui.button("◀").clicked() {
                    self.go_back();
                }

                if ui.button("▶").clicked() {
                    self.go_forward();
                }

                if ui.button("↻").clicked() {
                    self.reload();
                }

                if ui.button("🏠").clicked() {
                    self.navigate(DEFAULT_URL);
                }

                let response = ui.text_edit_singleline(&mut self.url_input);

                let url_to_navigate = self.url_input.clone();
                let enter_pressed = response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                if ui.button("Go").clicked() || enter_pressed {
                    self.navigate(&url_to_navigate);
                }

                if self.is_loading {
                    ui.label(format!("Loading... ({:.0}s)", self.load_start_time.map(|t| t.elapsed().as_secs_f64()).unwrap_or(0.0)));
                }
            });
        });

        egui::TopBottomPanel::bottom("log_panel").show(ctx, |ui| {
            ui.heading("Logs");
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
                    ui.image(&texture);
                });
            } else if self.auto_load_pending || self.is_loading {
                ui.centered_and_justified(|ui| {
                    ui.label("Initializing...");
                });
            } else {
                ui.centered_and_justified(|ui| {
                    ui.label("Enter URL and click Go");
                });
            }
        });
    }
}

fn run_screenshot_mode(args: &Args) -> Result<(), String> {
    println!("Rust Browser 截图模式");
    println!("目标 URL: {}", args.url);

    let mut browser = Browser::new().map_err(|e| format!("创建浏览器失败: {}", e))?;
    browser = browser.with_viewport(args.width, args.height);

    match browser.navigate(&args.url) {
        Ok(_) => println!("✓ 页面加载成功"),
        Err(e) => {
            eprintln!("✗ 页面加载失败: {}", e);
            return Err(format!("页面加载失败: {}", e));
        }
    }

    if let Some(output_path) = &args.output {
        println!("正在保存截图到: {:?}", output_path);
        match browser.screenshot(output_path) {
            Ok(_) => {
                println!("✓ 截图保存成功");
                Ok(())
            }
            Err(e) => {
                eprintln!("✗ 截图保存失败: {}", e);
                Err(format!("截图保存失败: {}", e))
            }
        }
    } else {
        Ok(())
    }
}

fn main() -> Result<(), eframe::Error> {
    // 初始化日志系统
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .format_timestamp_secs()
        .init();

    info!("Rust Browser 启动");
    add_log_message("Rust Browser 启动".to_string());

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
    println!("Rust Browser 窗口模式");

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([args.width as f32, args.height as f32])
            .with_min_inner_size([400.0, 300.0])
            .with_title("Rust Browser")
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
        "Rust Browser",
        options,
        Box::new(|cc| {
            let mut fonts = egui::FontDefinitions::default();
            
            fonts.font_data.insert(
                "china_font".to_owned(),
                egui::FontData::from_static(include_bytes!("C:/Windows/Fonts/msyh.ttc"))
                    .into(),
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
