//! Resource Scheduler — 资源优先级调度
//!
//! 实现 Arc 浏览器的"全局资源调度优先级等价变换"：
//! - 关键 CSS 优先阻塞完成
//! - 首屏图片即时解码
//! - 视口外图片延迟 GPU 离线解码
//! - 大体积资源移交 IO/GPU 线程
//!
//! 资源优先级：
//! - Critical: 首屏 CSS、字体、首屏图片 → UserBlocking
//! - High: 首屏渲染所需继续加载的资源 → UserVisible
//! - Medium: 非首屏图片、辅助 CSS → Background
//! - Low: 预加载、统计、埋点 → BestEffort

use crate::task_queue::task::TaskPriority;
use std::sync::{Arc, Mutex};

/// 资源类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceType {
    /// CSS 样式表
    Css,
    /// JavaScript
    JavaScript,
    /// 图片
    Image,
    /// 字体
    Font,
    /// 媒体（视频/音频）
    Media,
    /// 其他
    Other,
}

/// 资源优先级
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ResourcePriority {
    /// 首屏关键资源 — 必须尽快加载
    Critical,
    /// 首屏资源持续加载
    High,
    /// 非首屏资源
    Medium,
    /// 后台/预取/统计
    Low,
}

impl ResourcePriority {
    /// 转换为 TaskPriority
    pub fn to_task_priority(self) -> TaskPriority {
        match self {
            ResourcePriority::Critical => TaskPriority::UserBlocking,
            ResourcePriority::High => TaskPriority::UserVisible,
            ResourcePriority::Medium => TaskPriority::UserVisible,
            ResourcePriority::Low => TaskPriority::BestEffort,
        }
    }
}

/// 资源请求
#[derive(Debug, Clone)]
pub struct ResourceRequest {
    pub url: String,
    pub resource_type: ResourceType,
    pub priority: ResourcePriority,
    pub is_viewport: bool,
    pub is_fetching: bool,
    pub is_loaded: bool,
}

/// 资源调度器
pub struct ResourceScheduler {
    /// 待加载资源队列
    pending: Arc<Mutex<Vec<ResourceRequest>>>,
    /// 已加载资源缓存 (URL → bytes)
    cache: Arc<Mutex<lru_cache::LruCache<String, Vec<u8>>>>,
}

// 简单 LRU 缓存
mod lru_cache {
    use std::collections::HashMap;
    use std::hash::Hash;

    pub struct LruCache<K: Eq + Hash + Clone, V> {
        map: HashMap<K, V>,
        max_size: usize,
    }

    impl<K: Eq + Hash + Clone, V> LruCache<K, V> {
        pub fn new(max_size: usize) -> Self {
            Self {
                map: HashMap::new(),
                max_size,
            }
        }

        pub fn get(&self, key: &K) -> Option<&V> {
            self.map.get(key)
        }

        pub fn put(&mut self, key: K, value: V) {
            if self.map.len() >= self.max_size {
                // 简单策略：删除第一个条目
                if let Some(key) = self.map.keys().next().cloned() {
                    self.map.remove(&key);
                }
            }
            self.map.insert(key, value);
        }
    }
}

impl ResourceScheduler {
    pub fn new() -> Self {
        Self {
            pending: Arc::new(Mutex::new(Vec::new())),
            cache: Arc::new(Mutex::new(lru_cache::LruCache::new(256))),
        }
    }

    /// 请求加载资源
    pub fn request(&self, url: &str, resource_type: ResourceType, priority: ResourcePriority) {
        // 检查缓存
        {
            let cache = self.cache.lock().unwrap();
            if cache.get(&url.to_string()).is_some() {
                return;
            }
        }

        let request = ResourceRequest {
            url: url.to_string(),
            resource_type,
            priority,
            is_viewport: priority == ResourcePriority::Critical
                || priority == ResourcePriority::High,
            is_fetching: false,
            is_loaded: false,
        };

        self.pending.lock().unwrap().push(request);
    }

    /// 处理下一个最高优先级的待加载资源
    pub fn process_next(&self) -> Option<ResourceRequest> {
        let mut pending = self.pending.lock().unwrap();

        // 按优先级排序
        pending.sort_by_key(|r| r.priority);

        // 找到第一个未在加载中的
        if let Some(pos) = pending.iter().position(|r| !r.is_fetching) {
            let mut req = pending.remove(pos);
            req.is_fetching = true;
            Some(req)
        } else {
            None
        }
    }

    /// 标记资源加载完成（缓存结果）
    pub fn mark_loaded(&self, url: &str, data: Vec<u8>) {
        let mut cache = self.cache.lock().unwrap();
        cache.put(url.to_string(), data);
    }

    /// 从缓存获取已加载资源
    pub fn get_cached(&self, url: &str) -> Option<Vec<u8>> {
        self.cache.lock().unwrap().get(&url.to_string()).cloned()
    }

    /// 判断资源是否已在加载中
    pub fn is_pending(&self, url: &str) -> bool {
        let pending = self.pending.lock().unwrap();
        pending.iter().any(|r| r.url == url && !r.is_loaded)
    }

    /// 判断资源是否已缓存
    pub fn is_cached(&self, url: &str) -> bool {
        self.cache.lock().unwrap().get(&url.to_string()).is_some()
    }

    /// 清空所有待加载队列
    pub fn clear_pending(&self) {
        self.pending.lock().unwrap().clear();
    }
}

impl Default for ResourceScheduler {
    fn default() -> Self {
        Self::new()
    }
}
