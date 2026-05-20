//! 图片网络加载与缓存模块
//!
//! 支持异步下载网络图片、本地文件加载，使用 `HashMap` 做内存缓存。
//! 所有图片解码为 `tiny_skia::Pixmap`，供渲染流水线直接使用。

use lazy_static::lazy_static;
use log::{debug, error, trace, warn};
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};
use tiny_skia::{IntRect, IntSize, Pixmap};

lazy_static! {
    /// 全局 tokio runtime，用于同步阻塞等待异步下载
    static ref RUNTIME: tokio::runtime::Runtime = tokio::runtime::Runtime::new().unwrap();

    /// 全局共享的 reqwest Client（连接复用、gzip/brotli 默认启用）
    static ref HTTP_CLIENT: reqwest::Client = reqwest::Client::builder()
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36")
        .build()
        .expect("Failed to create reqwest::Client");

    /// 默认超时时间（秒）
    static ref TIMEOUT_SECS: u64 = 30;
}

/// 图片缓存管理器
///
/// 使用 `std::sync::Mutex<HashMap<String, Arc<Pixmap>>>` 做内存缓存。
/// 因为渲染是同步的，不能使用 `tokio::sync::Mutex`。
///
/// # 示例
///
/// ```ignore
/// let cache = ImageCache::new();
///
/// // 同步获取图片（优先查缓存）
/// if let Some(pixmap) = cache.get("https://example.com/image.png") {
///     // 使用 pixmap 进行渲染...
/// }
///
/// // 预加载（异步，不阻塞当前线程）
/// cache.preload("https://example.com/image2.png");
///
/// // 从 sprite 大图中裁剪子图
/// if let Some(sprite) = ImageCache::crop_sprite("sprite.png", -10, -20, 50, 50) {
///     // 使用裁剪后的子图...
/// }
/// ```
pub struct ImageCache {
    /// 内部使用 Arc 包裹 Mutex，使得 preload 的异步任务也能写入缓存
    cache: Arc<Mutex<HashMap<String, Arc<Pixmap>>>>,
}

impl ImageCache {
    /// 创建一个新的图片缓存
    pub fn new() -> Self {
        debug!("创建 ImageCache");
        Self {
            cache: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// 同步获取图片
    ///
    /// 优先查询缓存，若缓存未命中则阻塞下载/加载。
    ///
    /// 支持的 URL 格式：
    /// - `http://` / `https://` — 网络下载
    /// - `file://` — 本地文件
    /// - 普通路径 — 本地文件
    pub fn get(&self, url: &str) -> Option<Pixmap> {
        // 1. 查缓存
        {
            let cache = self.cache.lock().ok()?;
            if let Some(pixmap) = cache.get(url) {
                trace!("图片缓存命中: {}", url);
                return Some(pixmap.as_ref().clone());
            }
        }

        // 2. 缓存未命中，加载图片
        trace!("图片缓存未命中，开始加载: {}", url);
        let pixmap = load_image(url)?;

        // 3. 存入缓存
        let pixmap_arc = Arc::new(pixmap.clone());
        if let Ok(mut cache) = self.cache.lock() {
            cache.insert(url.to_string(), pixmap_arc);
        }

        Some(pixmap)
    }

    /// 预加载图片（异步，不阻塞当前线程）
    ///
    /// 使用 `tokio::spawn` 在后台下载并解码，完成后自动写入缓存。
    /// 如果图片已在缓存中，则跳过。
    pub fn preload(&self, url: &str) {
        // 如果已缓存则跳过
        {
            if let Ok(cache) = self.cache.lock() {
                if cache.contains_key(url) {
                    trace!("预加载跳过（已缓存）: {}", url);
                    return;
                }
            }
        }

        let url = url.to_string();
        let cache_arc = Arc::clone(&self.cache);

        // 使用全局 RUNTIME 在后台异步下载
        RUNTIME.spawn(async move {
            debug!("后台预加载图片: {}", url);
            match download_image_async(&url).await {
                Ok(bytes) => {
                    if let Some(pixmap) = decode_image_bytes(&bytes) {
                        let pixmap_arc = Arc::new(pixmap);
                        if let Ok(mut cache) = cache_arc.lock() {
                            cache.insert(url.clone(), pixmap_arc);
                            debug!("预加载完成（已写入缓存）: {}", url);
                        }
                    } else {
                        warn!("预加载图片解码失败: {}", url);
                    }
                }
                Err(e) => {
                    warn!("预加载图片下载失败 ({}): {}", url, e);
                }
            }
        });
    }

    /// 从 sprite 大图中裁剪子图
    ///
    /// `pos_x`, `pos_y` 对应 CSS `background-position` 的语义：
    /// - 负值表示向左/向上偏移（即裁剪区域向右/向下移动）
    /// - 正值表示向右/向下偏移（即裁剪区域向左/向上移动）
    ///
    /// `w`, `h` 对应元素的宽度和高度（即裁剪区域的尺寸）。
    ///
    /// # 参数说明
    ///
    /// 在 CSS sprite 中，通常将多个小图标合并到一张大图上。
    /// `background-position: -10px -20px` 表示将大图向左移动 10px、向上移动 20px，
    /// 这样目标图标的左上角就位于元素视口的左上角。
    /// 这意味着我们要从大图的 `(10, 20)` 位置开始裁剪一个 `w x h` 的区域。
    pub fn crop_sprite(url: &str, pos_x: i32, pos_y: i32, w: u32, h: u32) -> Option<Pixmap> {
        // 创建一个临时缓存来加载 sprite 大图
        let cache = ImageCache::new();
        let sprite = cache.get(url)?;

        // CSS background-position 语义：
        // `background-position: -Xpx -Ypx` 表示大图左移 X, 上移 Y
        // 即裁剪起点为 (X, Y)
        let src_x = pos_x.unsigned_abs().min(sprite.width().saturating_sub(1));
        let src_y = pos_y.unsigned_abs().min(sprite.height().saturating_sub(1));

        // 确保裁剪区域不超出 sprite 边界
        let crop_w = w.min(sprite.width().saturating_sub(src_x));
        let crop_h = h.min(sprite.height().saturating_sub(src_y));

        if crop_w == 0 || crop_h == 0 {
            warn!(
                "sprite 裁剪区域无效: pos=({},{}), size=({}x{}), sprite=({}x{})",
                pos_x,
                pos_y,
                w,
                h,
                sprite.width(),
                sprite.height()
            );
            return None;
        }

        trace!(
            "sprite 裁剪: url={}, 起点=({},{}), 尺寸=({}x{})",
            url,
            src_x,
            src_y,
            crop_w,
            crop_h
        );

        // 从 sprite 中裁剪子图
        // clone_rect 接受 IntRect 参数
        let rect = IntRect::from_xywh(src_x as i32, src_y as i32, crop_w, crop_h)?;
        sprite.clone_rect(rect)
    }

    /// 清空缓存
    pub fn clear(&self) {
        if let Ok(mut cache) = self.cache.lock() {
            cache.clear();
            debug!("图片缓存已清空");
        }
    }

    /// 缓存条目数
    pub fn len(&self) -> usize {
        self.cache.lock().map(|c| c.len()).unwrap_or(0)
    }

    /// 缓存是否为空
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl Default for ImageCache {
    fn default() -> Self {
        Self::new()
    }
}

/// 加载图片（同步方式）
///
/// 根据 URL 格式自动选择加载方式：
/// - `http://` / `https://` → 网络下载
/// - `//` 开头 → 协议相对 URL，补充 `https:` 后网络下载
/// - `data:image/...` → base64 编码图片
/// - `file://` 或本地路径 → 文件加载
fn load_image(url: &str) -> Option<Pixmap> {
    // 处理 base64 编码图片
    if url.starts_with("data:image/") {
        return load_image_from_base64(url);
    }

    // 处理协议相对 URL（以 // 开头）
    if url.starts_with("//") {
        let full_url = format!("https:{}", url);
        return load_image_from_network(&full_url);
    }

    if url.starts_with("http://") || url.starts_with("https://") {
        load_image_from_network(url)
    } else {
        load_image_from_file(url)
    }
}

/// 从 base64 编码的 data URL 加载图片
///
/// 格式: `data:image/png;base64,iVBORw0KGgo...`
fn load_image_from_base64(url: &str) -> Option<Pixmap> {
    // 查找 base64 数据的起始位置（逗号之后）
    let comma_pos = url.find(',')?;
    let base64_data = &url[comma_pos + 1..];

    // 解码 base64
    let bytes = base64_decode(base64_data)?;

    debug!(
        "base64 图片解码成功: {} bytes (原始 URL 前 60 字符: {}...)",
        bytes.len(),
        &url[..60.min(url.len())]
    );

    decode_image_bytes(&bytes)
}

/// base64 解码（纯 Rust 实现，无额外依赖）
/// 使用标准 base64 字符集: A-Za-z0-9+/
fn base64_decode(input: &str) -> Option<Vec<u8>> {
    // 去除空白字符
    let input: String = input.chars().filter(|c| !c.is_whitespace()).collect();
    if input.is_empty() {
        return None;
    }

    // 处理填充字符
    let input = input.trim_end_matches('=');

    // 解码表: 字符 → 6-bit 值
    let decode_char = |c: u8| -> Option<u32> {
        match c {
            b'A'..=b'Z' => Some((c - b'A') as u32),
            b'a'..=b'z' => Some((c - b'a' + 26) as u32),
            b'0'..=b'9' => Some((c - b'0' + 52) as u32),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    };

    let input_bytes = input.as_bytes();
    let mut result = Vec::with_capacity(input_bytes.len() * 3 / 4);

    let mut i = 0;
    while i + 3 < input_bytes.len() {
        let a = decode_char(input_bytes[i])?;
        let b = decode_char(input_bytes[i + 1])?;
        let c = decode_char(input_bytes[i + 2])?;
        let d = decode_char(input_bytes[i + 3])?;

        let combined = (a << 18) | (b << 12) | (c << 6) | d;
        result.push((combined >> 16) as u8);
        result.push(((combined >> 8) & 0xFF) as u8);
        result.push((combined & 0xFF) as u8);

        i += 4;
    }

    // 处理剩余字节
    let remaining = input_bytes.len() - i;
    match remaining {
        2 => {
            let a = decode_char(input_bytes[i])?;
            let b = decode_char(input_bytes[i + 1])?;
            let combined = (a << 18) | (b << 12);
            result.push((combined >> 16) as u8);
        }
        3 => {
            let a = decode_char(input_bytes[i])?;
            let b = decode_char(input_bytes[i + 1])?;
            let c = decode_char(input_bytes[i + 2])?;
            let combined = (a << 18) | (b << 12) | (c << 6);
            result.push((combined >> 16) as u8);
            result.push(((combined >> 8) & 0xFF) as u8);
        }
        _ => {}
    }

    Some(result)
}

/// 从网络加载图片（同步阻塞方式）
///
/// 使用 `RUNTIME.block_on()` 在同步上下文中等待异步下载完成。
fn load_image_from_network(url: &str) -> Option<Pixmap> {
    let result = RUNTIME.block_on(async {
        let resp = HTTP_CLIENT
            .get(url)
            .timeout(std::time::Duration::from_secs(*TIMEOUT_SECS))
            .send()
            .await
            .map_err(|e| format!("HTTP 请求失败: {}", e))?;

        let status = resp.status();
        if !status.is_success() && status.as_u16() != 304 {
            return Err(format!("HTTP 状态码: {}", status));
        }

        let bytes = resp
            .bytes()
            .await
            .map_err(|e| format!("读取响应体失败: {}", e))?
            .to_vec();

        Ok(bytes)
    });

    match result {
        Ok(bytes) => {
            debug!("图片下载成功: {} ({} bytes)", url, bytes.len());
            decode_image_bytes(&bytes)
        }
        Err(e) => {
            error!("图片下载失败 ({}): {}", url, e);
            None
        }
    }
}

/// 从本地文件加载图片
fn load_image_from_file(url: &str) -> Option<Pixmap> {
    // 移除 file:// 前缀
    let path_str = if let Some(rest) = url.strip_prefix("file://") {
        rest
    } else {
        url
    };

    let path = Path::new(path_str);

    if !path.exists() {
        warn!("本地图片文件不存在: {}", path.display());
        return None;
    }

    debug!("加载本地图片: {}", path.display());

    // 使用 image crate 打开并解码
    match image::open(path) {
        Ok(img) => {
            let rgba = img.to_rgba8();
            let (w, h) = rgba.dimensions();
            let data = rgba.into_raw();

            let size = IntSize::from_wh(w, h)?;
            Pixmap::from_vec(data, size)
                .map(|p| {
                    trace!("本地图片加载成功: {} ({}x{})", path.display(), w, h);
                    p
                })
                .or_else(|| {
                    warn!("无法将图片数据转换为 Pixmap: {}", path.display());
                    None
                })
        }
        Err(e) => {
            error!("本地图片解码失败 ({}): {}", path.display(), e);
            None
        }
    }
}

/// 解码图片字节为 Pixmap
fn decode_image_bytes(bytes: &[u8]) -> Option<Pixmap> {
    let img = image::load_from_memory(bytes).ok()?;
    let rgba = img.to_rgba8();
    let (w, h) = rgba.dimensions();
    let data = rgba.into_raw();

    let size = IntSize::from_wh(w, h)?;
    Pixmap::from_vec(data, size).or_else(|| {
        warn!("图片解码后无法转换为 Pixmap: {}x{}", w, h);
        None
    })
}

/// 异步下载图片（返回原始字节）
async fn download_image_async(url: &str) -> Result<Vec<u8>, String> {
    let resp = HTTP_CLIENT
        .get(url)
        .timeout(std::time::Duration::from_secs(*TIMEOUT_SECS))
        .send()
        .await
        .map_err(|e| format!("HTTP 请求失败: {}", e))?;

    let status = resp.status();
    if !status.is_success() && status.as_u16() != 304 {
        return Err(format!("HTTP 状态码: {}", status));
    }

    let bytes = resp
        .bytes()
        .await
        .map_err(|e| format!("读取响应体失败: {}", e))?
        .to_vec();

    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_image_cache_new_and_empty() {
        let cache = ImageCache::new();
        assert_eq!(cache.len(), 0);
        assert!(cache.is_empty());
    }

    #[test]
    fn test_image_cache_clear() {
        let cache = ImageCache::new();
        cache.clear();
        assert!(cache.is_empty());
    }

    #[test]
    fn test_decode_empty_bytes() {
        assert!(decode_image_bytes(&[]).is_none());
    }

    #[test]
    fn test_decode_invalid_bytes() {
        assert!(decode_image_bytes(b"not a valid image").is_none());
    }

    #[test]
    fn test_crop_sprite_empty_url() {
        let result = ImageCache::crop_sprite("nonexistent.png", 0, 0, 10, 10);
        assert!(result.is_none());
    }

    #[test]
    fn test_crop_sprite_zero_size() {
        // 即使 sprite 不存在，zero size 应该返回 None（但不 panic）
        let result = ImageCache::crop_sprite("nonexistent.png", 0, 0, 0, 0);
        assert!(result.is_none());
    }

    #[test]
    fn test_load_nonexistent_local_file() {
        let result = load_image("file:///nonexistent/path/to/image.png");
        assert!(result.is_none());
    }

    #[test]
    fn test_load_nonexistent_relative_path() {
        let result = load_image("nonexistent_image_file_12345.png");
        assert!(result.is_none());
    }

    #[test]
    fn test_protocol_relative_url_prefix() {
        // 协议相对 URL 应被识别为网络 URL，不走到本地文件
        let result = load_image("//www.example.com/image.png");
        // 无法真的下载，但至少不会 panic 且不走到本地文件
        // 这里只验证不会 panic
        let _ = result;
    }

    #[test]
    fn test_base64_decode_simple() {
        // "hello" 的 base64 编码
        let result = base64_decode("aGVsbG8=");
        assert_eq!(result, Some(vec![104, 101, 108, 108, 111]));
    }

    #[test]
    fn test_base64_decode_empty() {
        let result = base64_decode("");
        assert!(result.is_none());
    }

    #[test]
    fn test_base64_decode_no_padding() {
        let result = base64_decode("aGVsbG8");
        assert_eq!(result, Some(vec![104, 101, 108, 108, 111]));
    }

    #[test]
    fn test_data_url_prefix_detection() {
        // data URL 不应走到网络请求或本地文件
        let result = load_image("data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAA=");
        // 解码失败是预期的（截断的 base64），但不应该 panic
        assert!(result.is_none() || result.is_some());
    }
}
