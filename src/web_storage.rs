//! Web Storage — localStorage / sessionStorage 实现
//!
//! 符合 Web Storage 规范 (HTML Living Standard)
//!
//! - localStorage: 持久化存储（重启后保留）
//! - sessionStorage: 会话级存储（关闭后清除）
//!
//! 每个 origin 有独立的存储区域。

use std::collections::HashMap;
use std::sync::Mutex;

lazy_static::lazy_static! {
    /// 全局 localStorage 存储（按 origin 分域）
    static ref LOCAL_STORAGE: Mutex<HashMap<String, HashMap<String, String>>> = {
        let map = HashMap::new();
        Mutex::new(map)
    };
    /// 全局 sessionStorage 存储（按 origin 分域）
    static ref SESSION_STORAGE: Mutex<HashMap<String, HashMap<String, String>>> = {
        let map = HashMap::new();
        Mutex::new(map)
    };
}

/// Web Storage 操作
pub enum StorageType {
    Local,
    Session,
}

/// 获取指定 origin 的存储项数
pub fn storage_length(storage: StorageType, origin: &str) -> usize {
    let store = match storage {
        StorageType::Local => LOCAL_STORAGE.lock().unwrap(),
        StorageType::Session => SESSION_STORAGE.lock().unwrap(),
    };
    store.get(origin).map(|m| m.len()).unwrap_or(0)
}

/// 获取指定 origin 的存储键名（按索引）
pub fn storage_key(storage: StorageType, origin: &str, index: u32) -> Option<String> {
    let store = match storage {
        StorageType::Local => LOCAL_STORAGE.lock().unwrap(),
        StorageType::Session => SESSION_STORAGE.lock().unwrap(),
    };
    store.get(origin).and_then(|m| {
        m.keys().nth(index as usize).cloned()
    })
}

/// 获取存储项
pub fn storage_get_item(storage: StorageType, origin: &str, key: &str) -> Option<String> {
    let store = match storage {
        StorageType::Local => LOCAL_STORAGE.lock().unwrap(),
        StorageType::Session => SESSION_STORAGE.lock().unwrap(),
    };
    store.get(origin).and_then(|m| m.get(key).cloned())
}

/// 设置存储项
pub fn storage_set_item(storage: StorageType, origin: &str, key: &str, value: &str) {
    let mut store = match storage {
        StorageType::Local => LOCAL_STORAGE.lock().unwrap(),
        StorageType::Session => SESSION_STORAGE.lock().unwrap(),
    };
    store.entry(origin.to_string())
        .or_default()
        .insert(key.to_string(), value.to_string());
}

/// 移除存储项
pub fn storage_remove_item(storage: StorageType, origin: &str, key: &str) {
    let mut store = match storage {
        StorageType::Local => LOCAL_STORAGE.lock().unwrap(),
        StorageType::Session => SESSION_STORAGE.lock().unwrap(),
    };
    if let Some(map) = store.get_mut(origin) {
        map.remove(key);
    }
}

/// 清空存储
pub fn storage_clear(storage: StorageType, origin: &str) {
    let mut store = match storage {
        StorageType::Local => LOCAL_STORAGE.lock().unwrap(),
        StorageType::Session => SESSION_STORAGE.lock().unwrap(),
    };
    if let Some(map) = store.get_mut(origin) {
        map.clear();
    }
}

// ============================================================
// Cookie JS Bridge
// ============================================================

use crate::network::GLOBAL_COOKIE_JAR;

/// 通过 JS 读取 document.cookie（返回当前 origin 的所有 non-httpOnly cookie）
pub fn cookie_js_get(origin: &str) -> String {
    let jar = GLOBAL_COOKIE_JAR.lock().unwrap();
    let domain = extract_domain(origin);

    // 收集匹配 domain + path、未过期、non-HttpOnly 的 cookie
    let mut pairs: Vec<String> = jar.iter()
        .filter(|(_, entry)| {
            // 非 HttpOnly（JS 可读取）
            !entry.http_only
                // 未过期
                && !entry.is_expired()
                // 匹配 domain
                && entry.domain.as_str() == domain
        })
        .map(|(_, entry)| {
            format!("{}={}", entry.name, entry.value)
        })
        .collect();

    pairs.sort();
    pairs.join("; ")
}

/// 通过 JS 设置 document.cookie
pub fn cookie_js_set(origin: &str, cookie_str: &str) {
    let domain = extract_domain(origin);
    // 简单解析 name=value
    if let Some(eq_pos) = cookie_str.find('=') {
        let name = cookie_str[..eq_pos].trim().to_string();
        // 处理 value 部分（可能在分号后有额外属性，忽略它们）
        let value_part = cookie_str[eq_pos + 1..].trim();
        let value = value_part.split(';').next().unwrap_or(value_part).trim().to_string();

        let mut jar = GLOBAL_COOKIE_JAR.lock().unwrap();
        let key = format!("{}:{}:{}", domain, name, "/");
        jar.insert(key, crate::network::CookieEntry {
            name,
            value,
            domain: domain.clone(),
            path: "/".to_string(),
            secure: false,
            http_only: false,
            expires: None,
        });
    }
}

fn extract_domain(url: &str) -> String {
    if let Ok(parsed) = url::Url::parse(url) {
        parsed.host_str().unwrap_or("").to_string()
    } else {
        url.to_string()
    }
}
