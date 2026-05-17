//! 百度首页渲染测试
//!
//! 直接使用 BrowserEngine 渲染百度首页并保存截图
//! 不经过多进程架构，方便调试

use rust_browser::BrowserEngine;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== 渲染测试 ===\n");

    let mut engine = BrowserEngine::new(1280, 720)?;
    println!("✓ 浏览器引擎创建成功\n");

    // 测试本地 HTML 文件
    println!("正在测试本地 example.html ...");
    engine.load_html(
        include_str!("../examples/example.html"),
        "file:///example.html",
    )?;
    println!("✓ 页面加载成功");
    println!("  标题: {:?}", engine.title());

    let png_data = engine.render_to_image()?;
    println!("✓ 渲染完成 ({} 字节)", png_data.len());

    let output_path = "example_test_output.png";
    let img = image::load_from_memory(&png_data)?;
    img.save(output_path)?;
    println!(
        "✓ 截图已保存: {} ({}x{})\n",
        output_path,
        img.width(),
        img.height()
    );

    // 测试简单的 JS 执行（验证 BrowserEngine 已接入 JsEngine）
    println!("测试 JS 引擎...");
    match engine.execute_js("1 + 2") {
        Ok(result) => println!("✓ JS 执行结果: {}", result),
        Err(e) => println!("  JS 引擎未启用: {} (--features js)", e),
    }

    Ok(())
}
