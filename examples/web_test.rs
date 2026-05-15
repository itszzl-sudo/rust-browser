//! 真实网页支持能力测试
//!
//! 测试浏览器引擎对真实网页的支持

use rust_browser::{NetworkClient, DomWrapper};

#[tokio::main]
async fn main() {
    println!("=== 真实网页支持能力测试 ===\n");

    test_network_request().await;
    test_html_parsing().await;

    println!("\n=== 测试完成 ===");
}

async fn test_network_request() {
    println!("\n[测试 1] 网络请求能力测试");
    println!("----------------------------------------");

    let client = NetworkClient::new();

    // 测试 1: 简单网页请求
    println!("\n1.1 请求 https://example.com ...");
    match client.fetch_html("https://example.com").await {
        Ok(html) => {
            println!("✓ 请求成功!");
            println!("  - HTML 长度: {} 字符", html.len());
            println!("  - 前 100 字符: {}...", &html[..html.len().min(100)]);
        }
        Err(e) => {
            println!("✗ 请求失败: {}", e);
        }
    }

    // 测试 2: 百度首页
    println!("\n1.2 请求 https://www.baidu.com ...");
    match client.fetch_html("https://www.baidu.com").await {
        Ok(html) => {
            println!("✓ 请求成功!");
            println!("  - HTML 长度: {} 字符", html.len());
            println!("  - 前 150 字符: {}...", &html[..html.len().min(150)]);
        }
        Err(e) => {
            println!("✗ 请求失败: {}", e);
        }
    }

    // 测试 3: 检查响应状态
    println!("\n1.3 检查 HTTP 状态码...");
    match client.fetch("https://www.baidu.com").await {
        Ok(response) => {
            println!("✓ 响应状态: {}", response.status);
            println!("  - 内容长度: {} 字节", response.body.len());
            println!("  - URL: {}", response.url);
        }
        Err(e) => {
            println!("✗ 请求失败: {}", e);
        }
    }
}

async fn test_html_parsing() {
    println!("\n\n[测试 2] HTML 解析能力测试");
    println!("----------------------------------------");

    let client = NetworkClient::new();

    // 获取示例页面
    match client.fetch_html("https://example.com").await {
        Ok(html) => {
            println!("\n2.1 解析 HTML 文档 ...");

            let dom = DomWrapper::from_html(&html, Some("https://example.com"));

            println!("✓ DOM 树创建成功!");
            println!("  - 文档节点: {:?}", dom.document());

            // 遍历所有元素
            let elements = dom.traverse_elements();
            println!("  - 元素数量: {}", elements.len());

            // 显示前 10 个元素
            println!("\n2.2 前 10 个元素标签:");
            for (i, (id, tag)) in elements.iter().take(10).enumerate() {
                println!("  {}. {:?}: {}", i + 1, id, tag);
            }

            // 获取标题
            println!("\n2.3 提取页面标题:");
            if let Some(title) = dom.title() {
                println!("  - 标题: {}", title);
            } else {
                println!("  - 未找到 <title> 标签");
            }

            // 获取 body 内容
            println!("\n2.4 Body 元素信息:");
            let body_id = dom.body();
            println!("  - Body ID: {:?}", body_id);

            let body_children = dom.children(body_id);
            println!("  - Body 子元素数量: {}", body_children.len());

            // 显示 body 的直接子元素
            let mut direct_children = Vec::new();
            for child_id in body_children.iter().take(5) {
                if let Some(tag) = dom.tag_name(*child_id) {
                    direct_children.push(tag);
                }
            }
            println!("  - Body 直接子元素: {:?}", direct_children);

            // 测试文本提取
            println!("\n2.5 文本内容提取:");
            let text = dom.text_content_recursive(body_id);
            println!("  - Body 文本长度: {} 字符", text.len());
            if text.len() > 100 {
                println!("  - 前 100 字符: {}...", &text[..100]);
            } else {
                println!("  - 全部文本: {}", text);
            }
        }
        Err(e) => {
            println!("✗ 获取 HTML 失败: {}", e);
        }
    }
}
