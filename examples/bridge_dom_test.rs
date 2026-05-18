//! DOM 渲染测试 —— 通过 bridge.rs 直接构造 DOM 并渲染
//!
//! 不加载任何网络页面，不显示菜单栏/地址栏，
//! 直接通过 WebNativeBridge 构造 HTML DOM，渲染为 PNG 截图。
//!
//! 用法:
//!   cargo run --example bridge_dom_test
//!   cargo run --example bridge_dom_test -- --output custom_output.png

use std::path::PathBuf;

use clap::Parser;
use rust_browser::bridge::WebNativeBridge;
use rust_browser::bridge_impl::DefaultWebNativeBridge;

#[derive(Parser)]
#[command(name = "bridge_dom_test")]
struct Args {
    /// 输出图片路径
    #[arg(short, long, default_value = "bridge_dom_test_output.png")]
    output: PathBuf,

    /// 视口宽度
    #[arg(long, default_value_t = 1280)]
    width: u32,

    /// 视口高度
    #[arg(long, default_value_t = 720)]
    height: u32,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    println!("=== bridge.rs DOM 渲染测试 ===\n");

    // 1. 创建桥接器
    let mut bridge = DefaultWebNativeBridge::new(args.width, args.height);
    println!("✓ 桥接器创建成功 ({}x{})", args.width, args.height);

    // ──────────────────────────────────────────────
    // 2. 写入测试 DOM
    // ──────────────────────────────────────────────
    println!("\n--- 注入测试 DOM ---");

    bridge.set_html(
        r#"
<!DOCTYPE html>
<html>
<head>
    <title>Bridge 渲染测试</title>
</head>
<body>
    <div style="background: #1a73e8; padding: 20px; text-align: center;">
        <h1 style="color: white; margin: 0; font-size: 28px;">Bridge DOM 渲染测试</h1>
        <p style="color: #ccc; font-size: 14px; margin: 8px 0 0 0;">通过 WebNativeBridge 直接构造 DOM 并渲染</p>
    </div>

    <div style="padding: 10px;">
        <!-- 卡片一：颜色色块（不依赖flex，用table布局） -->
        <div id="card-colors" style="background: white; border-radius: 6px; padding: 10px; margin-bottom: 6px;">
            <h2 id="title-colors" style="color: #333; margin: 0 0 6px 0; font-size: 15px;">颜色色块</h2>
            <div id="color-container">
                <div id="red-block" style="width:50px; height:50px; background:#ff4444; border-radius:4px; display:inline-block;"></div>
                <div id="green-block" style="width:50px; height:50px; background:#44bb44; border-radius:4px; display:inline-block; margin-left:6px;"></div>
                <div id="blue-block" style="width:50px; height:50px; background:#4488ff; border-radius:4px; display:inline-block; margin-left:6px;"></div>
                <div id="orange-circle" style="width:50px; height:50px; background:orange; border-radius:50%; display:inline-block; margin-left:6px;"></div>
                <div id="purple-circle" style="width:50px; height:50px; background:purple; border-radius:50%; display:inline-block; margin-left:6px;"></div>
            </div>
        </div>

        <!-- 卡片二：文本渲染 -->
        <div id="card-text" style="background: white; border-radius: 6px; padding: 10px; margin-bottom: 6px;">
            <h2 id="title-text" style="color: #333; margin: 0 0 6px 0; font-size: 15px;">文本渲染</h2>
            <p id="p1" style="font-size: 13px; color: #333; margin: 3px 0;">Hello World! 你好，世界！</p>
            <p id="p2" style="font-size: 12px; color: #666; margin: 3px 0;">中文 English 数字 123 混合排版测试</p>
            <p id="p3" style="font-size: 11px; color: #999; border-left: 3px solid #1a73e8; padding-left: 8px; margin: 3px 0;">"The quick brown fox jumps over the lazy dog."</p>
        </div>

        <!-- 卡片三：Flex 三栏布局 -->
        <div id="card-flex" style="background: white; border-radius: 6px; padding: 10px; margin-bottom: 6px;">
            <h2 id="title-flex" style="color: #333; margin: 0 0 6px 0; font-size: 15px;">Flex 三栏布局</h2>
            <div id="flex-container" style="display: flex; gap: 8px; width: 100%;">
                <div id="flex-left" style="flex: 1; background:#e3f2fd; padding:8px; border-radius:4px; text-align:center;">
                    <div id="left-label" style="font-size:18px; color:#1565c0;">左</div>
                    <div id="left-desc" style="font-size:11px; color:#666;">flex: 1</div>
                </div>
                <div id="flex-center" style="flex: 2; background:#fce4ec; padding:8px; border-radius:4px; text-align:center;">
                    <div id="center-label" style="font-size:18px; color:#c62828;">中</div>
                    <div id="center-desc" style="font-size:11px; color:#666;">flex: 2</div>
                </div>
                <div id="flex-right" style="flex: 1; background:#e8f5e9; padding:8px; border-radius:4px; text-align:center;">
                    <div id="right-label" style="font-size:18px; color:#2e7d32;">右</div>
                    <div id="right-desc" style="font-size:11px; color:#666;">flex: 1</div>
                </div>
            </div>
        </div>

        <!-- 卡片四：点击事件测试 -->
        <div id="card-click" style="background: white; border-radius: 6px; padding: 10px;">
            <h2 id="title-click" style="color: #333; margin: 0 0 6px 0; font-size: 15px;">点击事件测试</h2>
            <div id="click-container" style="display: flex; gap: 8px; align-items: center;">
                <button id="btn-clickme" style="background:#1a73e8; color:white; border:none; padding:6px 14px; border-radius:4px; font-size:12px; width:70px; height:30px;">点击我</button>
                <button id="btn-reset" style="background:#f5f5f5; color:#666; border:1px solid #ddd; padding:6px 14px; border-radius:4px; font-size:12px; width:60px; height:30px;">重置</button>
                <a id="link-example" style="color:#1a73e8; font-size:12px;">链接示例</a>
            </div>
        </div>
    </div>

    <div id="footer" style="background: #333; padding: 16px; text-align: center;">
        <p id="footer-text" style="color: #999; font-size: 12px; margin: 0;">Rust Browser &copy; 2026 — 点击事件测试</p>
    </div>
</body>
</html>
"#,
    );
    println!("✓ DOM 注入完成");

    // 3. 添加自定义 CSS
    println!("\n--- 添加自定义 CSS ---");
    bridge.set_css(
        r#"
        body { margin: 0; font-family: sans-serif; background: #f0f2f5; }
        "#,
    );
    println!("✓ CSS 注入完成");

    // 4. 查询 DOM
    println!("\n--- DOM 查询 ---");
    if let Some(id) = bridge.query("#header") {
        println!("✓ #header 元素存在, tag={:?}", bridge.tag_name(id));
    }
    let title_text = bridge.query_text("h1").unwrap_or_default();
    println!("  h1 内容: {}", title_text);

    let all_elements = bridge.query_all("*");
    println!("  页面元素总数: {}", all_elements.len());

    let div_count = bridge.query_all("div").len();
    println!("  div 元素数: {}", div_count);

    let input_count = bridge.query_all("input").len();
    println!("  input 元素数: {}", input_count);

    let button_count = bridge.query_all("button").len();
    println!("  button 元素数: {}", button_count);

    // 5. 修改样式
    println!("\n--- 样式修改 ---");
    bridge.set_style("h1", "font-size", "32px");
    bridge.set_style("h1", "letter-spacing", "2px");
    // 给提交按钮加个边框
    bridge.set_style("button", "border", "2px solid #1a73e8");
    println!("✓ 样式修改完成");

    // 6. 渲染
    println!("\n--- 渲染 ---");

    // 渲染前布局（应该为空）
    let all_rects = bridge.all_rects();
    println!("  渲染前布局节点数: {}", all_rects.len());

    let png_data = bridge.render();
    println!("✓ 渲染完成！PNG 大小: {} 字节", png_data.len());
    let all_rects = bridge.all_rects();
    println!("  渲染后布局节点数: {}", all_rects.len());

    // 打印所有有 id 的元素
    println!("\n  所有带 id 的元素:");
    for n in &all_rects {
        if let Some(attr_id) = bridge.get_attr(n.dom_node, "id") {
            println!(
                "    id='{}' dom={} tag={} pos=({:.0},{:.0})",
                attr_id, n.dom_node, n.tag_name, n.x, n.y
            );
        }
    }

    // 完整布局信息
    println!("\n  完整布局节点:");
    let all_nodes = bridge.all_rects();
    // 先按 y 坐标排序
    let mut sorted: Vec<_> = all_nodes.iter().collect();
    sorted.sort_by(|a, b| a.y.partial_cmp(&b.y).unwrap());

    for n in &sorted {
        let text = bridge.text(n.dom_node).unwrap_or_default();
        let text_preview = if text.len() > 30 {
            format!("{}...", &text[..30])
        } else {
            text
        };
        let id_attr = bridge.get_attr(n.dom_node, "id").unwrap_or_default();
        let has_bg = n.background.is_some();
        println!(
            "    {}[{}] id='{}' pos=({:.0},{:.0}) size=({:.0}x{:.0}) bg={} text=\"{}\"",
            n.tag_name, n.dom_node, id_attr, n.x, n.y, n.width, n.height, has_bg, text_preview
        );
    }

    // 7. 保存截图
    let output_path = &args.output;
    let img = image::load_from_memory(&png_data)?;
    img.save(output_path)?;
    println!(
        "✓ 截图已保存: {} ({}x{})",
        output_path.display(),
        img.width(),
        img.height()
    );

    // 8. 打印布局信息
    println!("\n--- 布局信息（前 20 个元素）---");
    let rects = bridge.all_rects();
    for (i, n) in rects.iter().take(20).enumerate() {
        let text_preview = bridge.text(n.dom_node).unwrap_or_default();
        let preview = if text_preview.len() > 40 {
            format!("{}...", &text_preview[..40])
        } else {
            text_preview
        };
        println!(
            "  [{:>2}] {} (id={})  pos=({:.0},{:.0})  size=({:.0}x{:.0})  text=\"{}\"",
            i + 1,
            n.tag_name,
            n.dom_node,
            n.x,
            n.y,
            n.width,
            n.height,
            preview
        );
    }

    // 9. 测试 JS fetch（头模式下通过 bridge eval_js 调用 fetch）
    println!("\n--- JS fetch 测试 ---");
    let fetch_test_js = r#"
        (async function() {
            try {
                const resp = await fetch('https://httpbin.org/get?test=hello');
                const data = await resp.json();
                return 'fetch OK: args.test = ' + data.args.test;
            } catch(e) {
                return 'fetch error: ' + e.message;
            }
        })()
    "#;
    let fetch_result = bridge.eval_js(fetch_test_js);
    println!("  JS fetch 结果: {}", fetch_result);
    println!("✓ JS fetch 测试完成");

    // 10. 测试点击事件绑定
    println!("\n--- 点击事件测试 ---");

    // 注册事件处理器
    let click_log = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));

    // #card-colors 卡片点击
    let log1 = click_log.clone();
    bridge.on_click(
        "#card-colors",
        Box::new(move |x, y| {
            println!("  ✅ #card-colors 卡片被点击! 坐标: ({:.0}, {:.0})", x, y);
            log1.lock()
                .unwrap()
                .push(format!("card-colors clicked at ({:.0},{:.0})", x, y));
        }),
    );

    // #btn-clickme 按钮点击
    let log2 = click_log.clone();
    bridge.on_click(
        "#btn-clickme",
        Box::new(move |x, y| {
            println!("  ✅ #btn-clickme 按钮被点击! 坐标: ({:.0}, {:.0})", x, y);
            log2.lock()
                .unwrap()
                .push(format!("btn-clickme clicked at ({:.0},{:.0})", x, y));
        }),
    );

    // #flex-left 区域点击
    let log3 = click_log.clone();
    bridge.on_click(
        "#flex-left",
        Box::new(move |x, y| {
            println!("  ✅ #flex-left flex左侧被点击! 坐标: ({:.0}, {:.0})", x, y);
            log3.lock()
                .unwrap()
                .push(format!("flex-left clicked at ({:.0},{:.0})", x, y));
        }),
    );

    // #footer 页脚点击
    let log4 = click_log.clone();
    bridge.on_click(
        "#footer",
        Box::new(move |x, y| {
            println!("  ✅ #footer 页脚被点击! 坐标: ({:.0}, {:.0})", x, y);
            log4.lock()
                .unwrap()
                .push(format!("footer clicked at ({:.0},{:.0})", x, y));
        }),
    );

    // 模拟多个点击位置
    let test_points = vec![
        // (x, y, 描述)
        (30.0, 30.0, "蓝色头栏"),
        (60.0, 180.0, "颜色卡片区域"),
        (120.0, 200.0, "红色方块"),
        (60.0, 990.0, "页脚区域"),
        (90.0, 780.0, "Flex 容器"),
    ];

    for (x, y, desc) in &test_points {
        println!("\n  模拟点击: {} ({:.0}, {:.0})", desc, x, y);

        // 先查看 hit_test 结果
        if let Some(node) = bridge.hit_test(*x, *y) {
            println!(
                "    hit_test 命中: {} dom={} pos=({:.0},{:.0}) size=({:.0}x{:.0})",
                node.tag_name, node.dom_node, node.x, node.y, node.width, node.height
            );

            // 打印祖先链
            print!("    祖先链: ");
            let mut cur = Some(node.dom_node);
            while let Some(id) = cur {
                let tag = bridge.tag_name(id).unwrap_or_default();
                print!("{}[{}] ", tag, id);
                cur = bridge.parent_node(id);
            }
            println!();

            // 检查这个 dom_node 能被哪些 selector 匹配
            let selectors = ["#card-colors", "#btn-clickme", "#flex-left", "#footer"];
            for sel in &selectors {
                if let Some(sid) = bridge.query(sel) {
                    let stag = bridge.tag_name(sid).unwrap_or_default();
                    println!("    selector '{}' → dom_id={}, tag={}", sel, sid, stag);
                } else {
                    println!("    selector '{}' → 查询失败", sel);
                }
            }
        } else {
            println!("    hit_test 未命中任何布局节点");
        }

        let hit = bridge.handle_click(*x, *y);
        println!("    handle_click 返回: consumed={}", hit);
    }

    // 打印所有被触发的事件
    let events = click_log.lock().unwrap();
    println!("\n  触发了 {} 个事件:", events.len());
    for ev in events.iter() {
        println!("    - {}", ev);
    }

    println!("\n✓ 点击事件测试完成");

    println!("\n=== 测试完成 ===");
    Ok(())
}
