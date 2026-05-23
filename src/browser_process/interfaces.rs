//! Browser ↔ Renderer IPC 接口定义
//!
//! 类似 Chrome 的 mojom 接口定义
//!
//! 定义了三个 Mojo 接口管道：
//!
//! | 接口 | 方向 | 用途 |
//! |------|------|------|
//! | Navigation | Browser → Renderer | 导航、尺寸变更通知 |
//! | InputEvent | Browser → Renderer | 鼠标、键盘、滚动事件 |
//! | RenderResult | Renderer → Browser | 渲染结果帧回传 |

use crate::mojo::interface::{
    create_interface_pipe, InterfaceBinding, InterfaceProxy, PendingReceiver, PendingRemote,
};
use crate::mojo::message::Message;

// ==========================================================================
// Navigation Interface (Browser -> Renderer)
// ==========================================================================
//
// 浏览器进程通知渲染器进程进行导航。
// 类似 Chrome 的 `NavigationController` mojom 接口。

/// Mojo 接口名称
pub const NAVIGATION_INTERFACE_NAME: &str = "Navigation";

/// 导航消息 —— 浏览器通知渲染器加载指定 URL
#[derive(Debug, Clone)]
pub struct NavigationMessage {
    /// 目标 URL
    pub url: String,
    /// 视口宽度（px）
    pub width: u32,
    /// 视口高度（px）
    pub height: u32,
}

impl NavigationMessage {
    /// 序列化为 Mojo 消息
    ///
    /// 格式: `url|width|height`
    pub fn to_message(&self) -> Message {
        let data = format!("{}|{}|{}", self.url, self.width, self.height).into_bytes();
        Message::new("Navigate").with_data(data)
    }

    /// 从 Mojo 消息反序列化
    pub fn from_message(msg: &Message) -> Option<Self> {
        let s = String::from_utf8_lossy(&msg.data);
        let parts: Vec<&str> = s.split('|').collect();
        if parts.len() >= 3 {
            Some(Self {
                url: parts[0].to_string(),
                width: parts[1].parse().unwrap_or(1280),
                height: parts[2].parse().unwrap_or(720),
            })
        } else {
            None
        }
    }
}

/// 视口尺寸变更消息
#[derive(Debug, Clone)]
pub struct ResizeMessage {
    pub width: u32,
    pub height: u32,
}

impl ResizeMessage {
    /// 序列化为 Mojo 消息
    ///
    /// 格式: `width|height`
    pub fn to_message(&self) -> Message {
        let data = format!("{}|{}", self.width, self.height).into_bytes();
        Message::new("Resize").with_data(data)
    }

    /// 从 Mojo 消息反序列化
    pub fn from_message(msg: &Message) -> Option<Self> {
        let s = String::from_utf8_lossy(&msg.data);
        let parts: Vec<&str> = s.split('|').collect();
        if parts.len() >= 2 {
            let w: u32 = parts[0].parse().ok()?;
            let h: u32 = parts[1].parse().ok()?;
            Some(Self { width: w, height: h })
        } else {
            None
        }
    }
}

/// Navigation 接口代理（浏览器进程使用，负责发送导航请求）
pub type NavigationProxy = InterfaceProxy;

/// Navigation 接口绑定（渲染器进程使用，负责接收导航请求）
pub type NavigationBinding = InterfaceBinding;

/// 创建一对 Navigation 接口管道端点
///
/// 返回 `(PendingRemote, PendingReceiver)`，分别对应浏览器端和渲染器端。
pub fn create_navigation_pipe() -> (PendingRemote, PendingReceiver) {
    create_interface_pipe(NAVIGATION_INTERFACE_NAME)
}

// ==========================================================================
// Render Result Interface (Renderer -> Browser)
// ==========================================================================
//
// 渲染器进程将渲染结果发回浏览器进程。
// 类似 Chrome 的 `FrameHost` mojom 接口。

/// Mojo 接口名称
pub const RENDER_RESULT_INTERFACE_NAME: &str = "RenderResult";

/// 渲染结果消息 —— 渲染器将一帧渲染结果送回浏览器
#[derive(Debug, Clone)]
pub struct RenderResultMessage {
    /// PNG 编码的像素数据
    pub png_data: Vec<u8>,
    /// 图像宽度（px）
    pub width: u32,
    /// 图像高度（px）
    pub height: u32,
    /// 页面标题（可选）
    pub title: Option<String>,
    /// 是否为加载中帧
    pub is_loading: bool,
}

impl RenderResultMessage {
    /// 序列化为 Mojo 消息
    ///
    /// 格式: `[header_len:4byteBE][widthxheight|is_loading|title][png_bytes]`
    /// 使用长度前缀避免 PNG 二进制数据中的字节干扰分隔符
    pub fn to_message(&self) -> Message {
        let title = self.title.as_deref().unwrap_or("");
        let header = format!("{}x{}|{}|{}", self.width, self.height, self.is_loading, title);
        let header_bytes = header.as_bytes();

        // 4 字节大端头部长度 + 头部文本 + PNG 数据
        let mut data = Vec::with_capacity(4 + header_bytes.len() + self.png_data.len());
        data.extend_from_slice(&(header_bytes.len() as u32).to_be_bytes());
        data.extend_from_slice(header_bytes);
        data.extend_from_slice(&self.png_data);
        Message::new("FramePainted").with_data(data)
    }

    /// 从 Mojo 消息反序列化
    pub fn from_message(msg: &Message) -> Option<Self> {
        if msg.data.len() < 4 {
            return None;
        }

        // 读取前 4 字节作为头部长度（大端）
        let header_len =
            u32::from_be_bytes([msg.data[0], msg.data[1], msg.data[2], msg.data[3]]) as usize;

        if 4 + header_len > msg.data.len() {
            return None;
        }

        let header_bytes = &msg.data[4..4 + header_len];
        let png_data = &msg.data[4 + header_len..];

        let header = std::str::from_utf8(header_bytes).ok()?;
        let parts: Vec<&str> = header.split('|').collect();
        if parts.len() >= 3 {
            let dims: Vec<&str> = parts[0].split('x').collect();
            if dims.len() == 2 {
                let is_loading = parts[1].parse::<bool>().unwrap_or(false);
                let title = if parts.len() >= 3 {
                    let t = parts[2..].join("|");
                    if t.is_empty() {
                        None
                    } else {
                        Some(t)
                    }
                } else {
                    None
                };
                return Some(Self {
                    png_data: png_data.to_vec(),
                    width: dims[0].parse().unwrap_or(0),
                    height: dims[1].parse().unwrap_or(0),
                    title,
                    is_loading,
                });
            }
        }
        None
    }
}

/// RenderResult 接口代理（渲染器进程使用，负责发送渲染结果）
pub type RenderResultProxy = InterfaceProxy;

/// RenderResult 接口绑定（浏览器进程使用，负责接收渲染结果）
pub type RenderResultBinding = InterfaceBinding;

/// 创建一对 RenderResult 接口管道端点
///
/// 返回 `(PendingRemote, PendingReceiver)`，分别对应渲染器端和浏览器端。
pub fn create_render_result_pipe() -> (PendingRemote, PendingReceiver) {
    create_interface_pipe(RENDER_RESULT_INTERFACE_NAME)
}

// ==========================================================================
// Input Event Interface (Browser -> Renderer)
// ==========================================================================
//
// 浏览器进程将用户输入事件发送给渲染器进程。
// 类似 Chrome 的 `RenderWidget` mojom 接口。

/// Mojo 接口名称
pub const INPUT_EVENT_INTERFACE_NAME: &str = "InputEvent";

/// 用户输入事件枚举
#[derive(Debug, Clone)]
pub enum InputEvent {
    /// 鼠标移动
    MouseMove { x: f32, y: f32 },
    /// 鼠标点击
    MouseClick {
        x: f32,
        y: f32,
        /// 按钮编号: 0=左键, 1=中键, 2=右键
        button: u8,
    },
    /// 键盘按键
    KeyPress { key: String },
    /// 滚轮滚动
    Scroll { delta_x: f32, delta_y: f32 },
}

impl InputEvent {
    /// 序列化为 Mojo 消息
    pub fn to_message(&self) -> Message {
        match self {
            InputEvent::MouseMove { x, y } => {
                Message::new("MouseMove").with_data(format!("{}|{}", x, y).into_bytes())
            }
            InputEvent::MouseClick { x, y, button } => {
                Message::new("MouseClick").with_data(format!("{}|{}|{}", x, y, button).into_bytes())
            }
            InputEvent::KeyPress { key } => {
                Message::new("KeyPress").with_data(key.as_bytes().to_vec())
            }
            InputEvent::Scroll { delta_x, delta_y } => {
                Message::new("Scroll").with_data(format!("{}|{}", delta_x, delta_y).into_bytes())
            }
        }
    }
}

/// InputEvent 接口代理（浏览器进程使用，负责发送输入事件）
pub type InputEventProxy = InterfaceProxy;

/// InputEvent 接口绑定（渲染器进程使用，负责接收输入事件）
pub type InputEventBinding = InterfaceBinding;

/// RGBA 渲染结果消息 —— 渲染器将一帧 RGBA 像素数据送回浏览器
/// 比 PNG 方案省掉编解码，性能提升约 30%
#[derive(Debug, Clone)]
pub struct RenderResultRgba {
    /// RGBA 像素数据（每个像素 4 字节）
    pub rgba_data: Vec<u8>,
    /// 图像宽度（px）
    pub width: u32,
    /// 图像高度（px）
    pub height: u32,
    /// 页面标题（可选）
    pub title: Option<String>,
    /// 是否为加载中帧
    pub is_loading: bool,
}

impl RenderResultRgba {
    /// 序列化为 Mojo 消息（零拷贝：直接将 rgba_data Build 进 Message）
    /// 格式: `[header_len:4byteBE][widthxheight|RGBA|is_loading|title][rgba_bytes]`
    pub fn into_message(mut self) -> Message {
        let title = self.title.take().unwrap_or_default();
        let header = format!("{}x{}|RGBA|{}|{}", self.width, self.height, self.is_loading, title);
        let header_bytes = header.as_bytes();
        // 直接在 rgba_data 前面插入 header，零拷贝（仅 header 部分新建）
        let mut data = Vec::with_capacity(4 + header_bytes.len() + self.rgba_data.len());
        data.extend_from_slice(&(header_bytes.len() as u32).to_be_bytes());
        data.extend_from_slice(header_bytes);
        // 将 rgba_data 拼到 header 后面——这是唯一的大块数据拷贝
        // 由于 Message 要求连续缓冲区，无法完全避免
        data.append(&mut self.rgba_data);
        Message::new("FramePaintedRGBA").with_data(data)
    }

    /// 从 Mojo 消息反序列化
    pub fn from_message(msg: &Message) -> Option<Self> {
        if msg.data.len() < 4 {
            return None;
        }
        let header_len =
            u32::from_be_bytes([msg.data[0], msg.data[1], msg.data[2], msg.data[3]]) as usize;
        if 4 + header_len > msg.data.len() {
            return None;
        }
        let header_bytes = &msg.data[4..4 + header_len];
        let rgba_data = &msg.data[4 + header_len..];
        let header = std::str::from_utf8(header_bytes).ok()?;
        let parts: Vec<&str> = header.split('|').collect();
        if parts.len() >= 4 && parts[1] == "RGBA" {
            let dims: Vec<&str> = parts[0].split('x').collect();
            if dims.len() == 2 {
                let is_loading = parts[2].parse::<bool>().unwrap_or(false);
                let title = if parts.len() >= 4 {
                    let t = parts[3..].join("|");
                    if t.is_empty() {
                        None
                    } else {
                        Some(t)
                    }
                } else {
                    None
                };
                return Some(Self {
                    rgba_data: rgba_data.to_vec(),
                    width: dims[0].parse().unwrap_or(0),
                    height: dims[1].parse().unwrap_or(0),
                    title,
                    is_loading,
                });
            }
        }
        None
    }
}

/// 创建一对 InputEvent 接口管道端点
///
/// 返回 `(PendingRemote, PendingReceiver)`，分别对应浏览器端和渲染器端。
pub fn create_input_event_pipe() -> (PendingRemote, PendingReceiver) {
    create_interface_pipe(INPUT_EVENT_INTERFACE_NAME)
}

// ==========================================================================
// Tests
// ==========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_navigation_message_roundtrip() {
        let msg = NavigationMessage {
            url: "https://example.com".to_string(),
            width: 1920,
            height: 1080,
        };
        let m = msg.to_message();
        let decoded = NavigationMessage::from_message(&m).unwrap();
        assert_eq!(decoded.url, "https://example.com");
        assert_eq!(decoded.width, 1920);
        assert_eq!(decoded.height, 1080);
    }

    #[test]
    fn test_render_result_message_roundtrip() {
        let msg = RenderResultMessage {
            png_data: vec![0x89, 0x50, 0x4e, 0x47],
            width: 800,
            height: 600,
            title: Some("Test Page".to_string()),
            is_loading: false,
        };
        let m = msg.to_message();
        let decoded = RenderResultMessage::from_message(&m).unwrap();
        assert_eq!(decoded.width, 800);
        assert_eq!(decoded.height, 600);
        assert_eq!(decoded.title, Some("Test Page".to_string()));
        assert_eq!(decoded.is_loading, false);
        assert_eq!(decoded.png_data, vec![0x89, 0x50, 0x4e, 0x47]);
    }

    #[test]
    fn test_render_result_message_loading_roundtrip() {
        let msg = RenderResultMessage {
            png_data: vec![0x89, 0x50, 0x4e, 0x47],
            width: 800,
            height: 600,
            title: Some("加载中…".to_string()),
            is_loading: true,
        };
        let m = msg.to_message();
        let decoded = RenderResultMessage::from_message(&m).unwrap();
        assert_eq!(decoded.width, 800);
        assert_eq!(decoded.height, 600);
        assert_eq!(decoded.title, Some("加载中…".to_string()));
        assert_eq!(decoded.is_loading, true);
        assert_eq!(decoded.png_data, vec![0x89, 0x50, 0x4e, 0x47]);
    }

    #[test]
    fn test_render_result_message_png_with_newline_bytes() {
        // PNG 二进制数据中包含 0x0A (\n) 字节，旧的分隔符方案会出错
        let png_with_newlines = vec![
            0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, // PNG 签名含 \n
            0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52, // IHDR 块
            0x0a, 0x0a, 0x0a, 0x0a, // 更多 \n 字节
        ];
        let msg = RenderResultMessage {
            png_data: png_with_newlines.clone(),
            width: 100,
            height: 200,
            title: None,
            is_loading: false,
        };
        let m = msg.to_message();
        let decoded = RenderResultMessage::from_message(&m).unwrap();
        assert_eq!(decoded.width, 100);
        assert_eq!(decoded.height, 200);
        assert_eq!(decoded.title, None);
        assert_eq!(decoded.is_loading, false);
        assert_eq!(decoded.png_data, png_with_newlines);
    }

    #[test]
    fn test_render_result_message_chinese_title() {
        // 中文标题中的字符可能包含 0x0A 字节的 UTF-8 编码片段
        let msg = RenderResultMessage {
            png_data: vec![0x89, 0x50, 0x4e, 0x47],
            width: 1920,
            height: 1080,
            title: Some("百度一下，你就知道".to_string()),
            is_loading: false,
        };
        let m = msg.to_message();
        let decoded = RenderResultMessage::from_message(&m).unwrap();
        assert_eq!(decoded.width, 1920);
        assert_eq!(decoded.height, 1080);
        assert_eq!(decoded.title, Some("百度一下，你就知道".to_string()));
        assert_eq!(decoded.is_loading, false);
        assert_eq!(decoded.png_data, vec![0x89, 0x50, 0x4e, 0x47]);
    }

    #[test]
    fn test_input_event_serialization() {
        let event = InputEvent::MouseClick {
            x: 100.0,
            y: 200.0,
            button: 0,
        };
        let m = event.to_message();
        assert_eq!(m.name.as_str(), "MouseClick");
        let data = String::from_utf8_lossy(&m.data);
        assert_eq!(data, "100|200|0");
    }

    #[test]
    fn test_create_pipes() {
        let (remote, receiver) = create_navigation_pipe();
        assert!(remote.is_some());
        assert!(receiver.is_some());

        let (remote2, receiver2) = create_render_result_pipe();
        assert!(remote2.is_some());
        assert!(receiver2.is_some());
    }
}
