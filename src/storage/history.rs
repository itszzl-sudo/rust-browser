//! 浏览历史记录 —— 类似 Chrome 的 History
//!
//! 自动记录每次导航的 URL、标题、访问时间。
//! 支持按日期分组、搜索、清除、持久化。

use lazy_static::lazy_static;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// 单条历史记录
#[derive(Debug, Clone)]
pub struct HistoryEntry {
    /// 完整 URL
    pub url: String,
    /// 页面标题
    pub title: String,
    /// 访问时间戳（Unix 毫秒）
    pub timestamp_ms: u64,
    /// 访问次数
    pub visit_count: u32,
}

/// 历史记录管理器
pub struct History {
    entries: Vec<HistoryEntry>,
    /// URL → index 映射（快速去重）
    url_index: HashMap<String, usize>,
    /// 是否已脏（需要持久化）
    dirty: bool,
    /// 最大记录数
    max_entries: usize,
}

impl History {
    /// 创建新的历史记录管理器
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            url_index: HashMap::new(),
            dirty: false,
            max_entries: 10000, // 最多保留 10000 条
        }
    }

    /// 记录一次页面访问
    pub fn record_visit(&mut self, url: &str, title: &str) {
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        // 如果已存在相同 URL，更新时间和增加计数
        if let Some(&idx) = self.url_index.get(url) {
            if idx < self.entries.len() {
                self.entries[idx].timestamp_ms = now_ms;
                self.entries[idx].visit_count += 1;
                if !title.is_empty() {
                    self.entries[idx].title = title.to_string();
                }
                self.dirty = true;
                return;
            }
        }

        // 新增记录
        let entry = HistoryEntry {
            url: url.to_string(),
            title: if title.is_empty() {
                url.to_string()
            } else {
                title.to_string()
            },
            timestamp_ms: now_ms,
            visit_count: 1,
        };

        self.entries.push(entry);
        self.url_index
            .insert(url.to_string(), self.entries.len() - 1);

        // 超过上限时删除最旧的
        if self.entries.len() > self.max_entries {
            let remove_count = self.entries.len() - self.max_entries;
            for _ in 0..remove_count {
                if let Some(oldest) = self.entries.first() {
                    self.url_index.remove(&oldest.url);
                    self.entries.remove(0);
                }
            }
        }

        self.dirty = true;
    }

    /// 获取所有历史记录（按时间倒序）
    pub fn all_entries(&self) -> Vec<&HistoryEntry> {
        let mut result: Vec<&HistoryEntry> = self.entries.iter().collect();
        result.sort_by(|a, b| b.timestamp_ms.cmp(&a.timestamp_ms));
        result
    }

    /// 搜索历史记录（按 URL 和标题模糊匹配）
    pub fn search(&self, query: &str) -> Vec<&HistoryEntry> {
        let query_lower = query.to_lowercase();
        let mut result: Vec<&HistoryEntry> = self
            .entries
            .iter()
            .filter(|e| {
                e.url.to_lowercase().contains(&query_lower)
                    || e.title.to_lowercase().contains(&query_lower)
            })
            .collect();
        result.sort_by(|a, b| b.timestamp_ms.cmp(&a.timestamp_ms));
        result
    }

    /// 获取按日期分组的记录
    pub fn grouped_by_date(&self) -> Vec<(String, Vec<&HistoryEntry>)> {
        let mut groups: HashMap<String, Vec<&HistoryEntry>> = HashMap::new();
        for entry in &self.entries {
            let date = Self::format_date(entry.timestamp_ms);
            groups.entry(date).or_default().push(entry);
        }
        let mut result: Vec<(String, Vec<&HistoryEntry>)> = groups.into_iter().collect();
        // 按日期降序
        result.sort_by(|a, b| b.0.cmp(&a.0));
        result
    }

    /// 删除单条记录
    pub fn remove_entry(&mut self, url: &str) {
        self.url_index.remove(url);
        self.entries.retain(|e| e.url != url);
        self.dirty = true;
    }

    /// 清除所有历史记录
    pub fn clear(&mut self) {
        self.entries.clear();
        self.url_index.clear();
        self.dirty = true;
    }

    /// 返回记录总数
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 将时间戳格式化为日期字符串
    fn format_date(timestamp_ms: u64) -> String {
        let secs = timestamp_ms / 1000;
        let days = secs / 86400;
        // 简单日期计算（从 1970-01-01 开始）
        let mut y = 1970i64;
        let mut remaining = days as i64;
        loop {
            let days_in_year = if Self::is_leap(y) { 366 } else { 365 };
            if remaining < days_in_year {
                break;
            }
            remaining -= days_in_year;
            y += 1;
        }
        let months = [
            31,
            if Self::is_leap(y) { 29 } else { 28 },
            31,
            30,
            31,
            30,
            31,
            31,
            30,
            31,
            30,
            31,
        ];
        let mut m = 0;
        for (i, &days_in_m) in months.iter().enumerate() {
            if remaining < days_in_m {
                m = i;
                break;
            }
            remaining -= days_in_m;
        }
        let d = remaining + 1;
        format!("{:04}-{:02}-{:02}", y, m + 1, d)
    }

    fn is_leap(y: i64) -> bool {
        (y % 4 == 0 && y % 100 != 0) || (y % 400 == 0)
    }

    // ================================================================
    // JSON 持久化
    // ================================================================

    pub fn to_json(&self) -> String {
        let mut json = String::from("{\"entries\":[");
        let mut first = true;
        for entry in &self.entries {
            if !first {
                json.push(',');
            }
            first = false;
            let title_escaped = entry
                .title
                .replace('\\', "\\\\")
                .replace('"', "\\\"")
                .replace('\n', "\\n");
            let url_escaped = entry
                .url
                .replace('\\', "\\\\")
                .replace('"', "\\\"")
                .replace('\n', "\\n");
            json.push_str(&format!(
                r#"{{"u":"{}","t":"{}","ts":{},"vc":{}}}"#,
                url_escaped, title_escaped, entry.timestamp_ms, entry.visit_count
            ));
        }
        json.push_str("]}");
        json
    }

    pub fn from_json(json: &str) -> Self {
        let mut history = History::new();
        let json = json.trim();
        if !json.starts_with('{') || !json.ends_with('}') {
            return history;
        }

        // 查找 entries 数组
        if let Some(arr_start) = json.find("\"entries\":[") {
            let start = arr_start + "\"entries\":[".len();
            if let Some(end) = json.rfind(']') {
                let content = &json[start..end];
                if content.is_empty() {
                    return history;
                }

                // 解析每个对象
                let mut depth = 0i32;
                let mut obj_start = 0;
                let mut in_str = false;
                let mut esc = false;
                for (i, ch) in content.char_indices() {
                    if esc {
                        esc = false;
                        continue;
                    }
                    if ch == '\\' && in_str {
                        esc = true;
                        continue;
                    }
                    if ch == '"' {
                        in_str = !in_str;
                        continue;
                    }
                    if in_str {
                        continue;
                    }
                    match ch {
                        '{' if depth == 0 => {
                            obj_start = i;
                            depth = 1;
                        }
                        '{' => depth += 1,
                        '}' => {
                            depth -= 1;
                            if depth == 0 {
                                let obj = &content[obj_start..=i];
                                if let Some(entry) = Self::parse_entry(obj) {
                                    history
                                        .url_index
                                        .insert(entry.url.clone(), history.entries.len());
                                    history.entries.push(entry);
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
        history
    }

    fn parse_entry(s: &str) -> Option<HistoryEntry> {
        let mut url = String::new();
        let mut title = String::new();
        let mut ts: u64 = 0;
        let mut vc: u32 = 0;

        // 简单字段解析
        for part in s.trim_matches(|c| c == '{' || c == '}').split(',') {
            let part = part.trim();
            if let Some(eq) = part.find(':') {
                let key = part[..eq].trim().trim_matches('"');
                let val = part[eq + 1..].trim().trim_matches('"');
                match key {
                    "u" => url = val.to_string(),
                    "t" => title = val.to_string(),
                    "ts" => ts = val.parse().unwrap_or(0),
                    "vc" => vc = val.parse().unwrap_or(1),
                    _ => {}
                }
            }
        }
        if url.is_empty() {
            None
        } else {
            Some(HistoryEntry {
                url,
                title,
                timestamp_ms: ts,
                visit_count: vc,
            })
        }
    }

    pub fn save_to_file(&self, path: &Path) -> Result<(), String> {
        let json = self.to_json();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("创建目录失败: {}", e))?;
        }
        let tmp = path.with_extension("tmp");
        fs::write(&tmp, &json).map_err(|e| format!("写入失败: {}", e))?;
        fs::rename(&tmp, path).map_err(|e| format!("重命名失败: {}", e))?;
        Ok(())
    }

    pub fn load_from_file(path: &Path) -> Self {
        match fs::read_to_string(path) {
            Ok(json) => Self::from_json(&json),
            Err(_) => History::new(),
        }
    }
}

lazy_static! {
    /// 全局历史记录
    pub static ref HISTORY: Mutex<History> = Mutex::new(History::new());
}

/// 默认历史记录文件路径
pub fn default_history_path(data_dir: &Path) -> PathBuf {
    data_dir.join("history.json")
}
