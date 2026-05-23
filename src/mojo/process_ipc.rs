//! 基于 stdio 管道的进程间 IPC 传输层
//!
//! 替代内存 MessagePipe，通过 stdin/stdout 在真实进程间传递消息。
//! 消息格式：4字节长度前缀（大端）+ JSON 序列化的消息体。
//!
//! # 架构
//!
//! 浏览器进程（父进程）：
//!   1. 创建子进程（`--renderer-process`）
//!   2. 向子进程的 stdin 写入 Navigation/InputEvent 消息
//!   3. 从子进程的 stdout 读取 RenderResult 消息
//!
//! 渲染器进程（子进程）：
//!   1. 从 stdin 读取 Navigation/InputEvent 消息
//!   2. 处理渲染
//!   3. 向 stdout 写入 RenderResult 消息

use crate::mojo::message::Message;
use std::io::{self, Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// 进程 IPC 通道 — 封装 stdin/stdout 管道
pub struct ProcessIpcChannel {
    running: Arc<AtomicBool>,
}

impl ProcessIpcChannel {
    pub fn new() -> Self {
        Self {
            running: Arc::new(AtomicBool::new(true)),
        }
    }

    /// 获取运行状态标志
    pub fn running_flag(&self) -> Arc<AtomicBool> {
        self.running.clone()
    }

    /// 发送一条消息到 stdout（渲染进程 → 浏览器进程）
    pub fn send_to_stdout(msg: &Message) -> Result<(), String> {
        let encoded = encode_message(msg);
        let mut stdout = io::stdout().lock();
        stdout
            .write_all(&encoded)
            .map_err(|e| format!("写入 stdout 失败: {}", e))?;
        stdout
            .flush()
            .map_err(|e| format!("刷新 stdout 失败: {}", e))?;
        Ok(())
    }

    /// 从 stdin 读取一条消息（浏览器进程 → 渲染进程）
    pub fn read_from_stdin(&self) -> Result<Message, String> {
        let mut stdin = io::stdin().lock();
        read_message(&mut stdin)
    }

    /// 从 stdin 非阻塞尝试读取一条消息
    /// 简化实现：使用阻塞读取（真实环境应使用 select/poll）
    pub fn try_read_from_stdin(&self) -> Option<Message> {
        match self.read_from_stdin() {
            Ok(msg) => Some(msg),
            Err(_) => None,
        }
    }

    /// 向子进程的 stdin 写入消息（浏览器进程使用）
    pub fn write_to_stdin(child_stdin: &mut impl Write, msg: &Message) -> Result<(), String> {
        let encoded = encode_message(msg);
        child_stdin
            .write_all(&encoded)
            .map_err(|e| format!("写入子进程 stdin 失败: {}", e))?;
        child_stdin
            .flush()
            .map_err(|e| format!("刷新子进程 stdin 失败: {}", e))?;
        Ok(())
    }

    /// 从子进程的 stdout 读取消息（浏览器进程使用）
    pub fn read_from_child_stdout(child_stdout: &mut impl Read) -> Result<Message, String> {
        read_message(child_stdout)
    }

    /// 停止运行
    pub fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
    }
}

// ═══════════════════════════════════════════════════════════════
// 消息序列化/反序列化
// ═══════════════════════════════════════════════════════════════

/// 消息格式：
/// [4字节大端长度][UTF-8 JSON: {"name":"...","data":[..]}]
fn encode_message(msg: &Message) -> Vec<u8> {
    // 将 data 做 base64 编码以确保 JSON 安全
    let data_b64 = base64_encode(&msg.data);
    let name_str = msg.name.as_str();
    let json_str = format!(
        r#"{{"name":"{}","data":"{}"}}"#,
        name_str.replace('\\', "\\\\").replace('"', "\\\""),
        data_b64
    );
    let json_bytes = json_str.as_bytes();
    let len = json_bytes.len() as u32;

    let mut result = Vec::with_capacity(4 + json_bytes.len());
    result.extend_from_slice(&len.to_be_bytes());
    result.extend_from_slice(json_bytes);
    result
}

/// 从 reader 读取一条消息
fn read_message(reader: &mut impl Read) -> Result<Message, String> {
    // 读取 4 字节长度前缀
    let mut len_buf = [0u8; 4];
    reader
        .read_exact(&mut len_buf)
        .map_err(|e| format!("读取消息长度失败: {}", e))?;
    let json_len = u32::from_be_bytes(len_buf) as usize;

    if json_len == 0 || json_len > 10 * 1024 * 1024 {
        return Err(format!("无效的消息长度: {}", json_len));
    }

    // 读取 JSON 体
    let mut json_buf = vec![0u8; json_len];
    reader
        .read_exact(&mut json_buf)
        .map_err(|e| format!("读取消息体失败: {}", e))?;

    let json_str = String::from_utf8(json_buf).map_err(|e| format!("消息不是有效 UTF-8: {}", e))?;

    // 解析 JSON
    let parsed: serde_json::Value =
        serde_json::from_str(&json_str).map_err(|e| format!("JSON 解析失败: {}", e))?;

    let name = parsed
        .get("name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "消息缺少 name 字段".to_string())?;

    let data_str = parsed
        .get("data")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "消息缺少 data 字段".to_string())?;

    let data = base64_decode(data_str).map_err(|e| format!("base64 解码失败: {}", e))?;

    Ok(Message::new_with_owned_name(name.to_string()).with_data(data))
}

// ═══════════════════════════════════════════════════════════════
// Base64 编解码（无额外依赖，纯 Rust 实现）
// ═══════════════════════════════════════════════════════════════

const BASE64_CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn base64_encode(input: &[u8]) -> String {
    let mut result = String::new();
    for chunk in input.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = chunk.get(1).copied().unwrap_or(0) as u32;
        let b2 = chunk.get(2).copied().unwrap_or(0) as u32;
        let triple = (b0 << 16) | (b1 << 8) | b2;

        result.push(BASE64_CHARS[((triple >> 18) & 0x3F) as usize] as char);
        result.push(BASE64_CHARS[((triple >> 12) & 0x3F) as usize] as char);
        if chunk.len() > 1 {
            result.push(BASE64_CHARS[((triple >> 6) & 0x3F) as usize] as char);
        } else {
            result.push('=');
        }
        if chunk.len() > 2 {
            result.push(BASE64_CHARS[(triple & 0x3F) as usize] as char);
        } else {
            result.push('=');
        }
    }
    result
}

fn base64_decode(input: &str) -> Result<Vec<u8>, String> {
    // 构建反向查找表
    let mut rev = [0xFFu8; 256];
    for (i, &c) in BASE64_CHARS.iter().enumerate() {
        rev[c as usize] = i as u8;
    }
    // '=' 填充用
    rev[b'=' as usize] = 0;

    let input = input.trim();
    let mut result = Vec::with_capacity(input.len() * 3 / 4);
    let bytes = input.as_bytes();

    for chunk in bytes.chunks(4) {
        if chunk.len() < 4 {
            break;
        }
        let mut vals = [0u8; 4];
        for (i, &b) in chunk.iter().enumerate() {
            if b == b'=' {
                vals[i] = 0;
            } else {
                vals[i] = rev[b as usize];
                if vals[i] == 0xFF {
                    return Err(format!("无效的 base64 字符: {}", b as char));
                }
            }
        }
        let triple = ((vals[0] as u32) << 18)
            | ((vals[1] as u32) << 12)
            | ((vals[2] as u32) << 6)
            | (vals[3] as u32);

        result.push((triple >> 16) as u8);
        if chunk.len() > 2 && chunk[2] != b'=' {
            result.push((triple >> 8) as u8);
        }
        if chunk.len() > 3 && chunk[3] != b'=' {
            result.push(triple as u8);
        }
    }
    Ok(result)
}

// ═══════════════════════════════════════════════════════════════
// 测试
// ═══════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_base64_roundtrip() {
        let data = b"Hello, World!";
        let encoded = base64_encode(data);
        let decoded = base64_decode(&encoded).unwrap();
        assert_eq!(decoded, data);
    }

    #[test]
    fn test_base64_binary() {
        let data = vec![0x00, 0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0xFF];
        let encoded = base64_encode(&data);
        let decoded = base64_decode(&encoded).unwrap();
        assert_eq!(decoded, data);
    }

    #[test]
    fn test_message_roundtrip() {
        let msg =
            Message::new_with_owned_name("TestMethod".to_string()).with_data(vec![1, 2, 3, 4, 5]);
        let encoded = encode_message(&msg);
        let mut cursor = io::Cursor::new(&encoded);
        let decoded = read_message(&mut cursor).unwrap();
        assert_eq!(decoded.name.as_str(), "TestMethod");
        assert_eq!(decoded.data, vec![1, 2, 3, 4, 5]);
    }

    #[test]
    fn test_message_with_binary_data() {
        let msg = Message::new_with_owned_name("FramePaintedRGBA".to_string())
            .with_data(vec![0x89, 0x50, 0x4E, 0x47, 0x00, 0xFF, 0xFE]);
        let encoded = encode_message(&msg);
        let mut cursor = io::Cursor::new(&encoded);
        let decoded = read_message(&mut cursor).unwrap();
        assert_eq!(decoded.name.as_str(), "FramePaintedRGBA");
        assert_eq!(decoded.data, vec![0x89, 0x50, 0x4E, 0x47, 0x00, 0xFF, 0xFE]);
    }
}
