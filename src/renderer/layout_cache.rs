//! Layout Cache — 布局缓存与增量计算
//!
//! 实现 Arc 的"布局树静态不变量缓存 + 动态增量局部计算"机制。
//!
//! # 核心策略
//!
//! 1. 提取静态布局不变量（盒模型、嵌套结构、固定排版样式）→ 一次性缓存永久复用
//! 2. 动态属性仅留存 transform、opacity、滚动偏移 → GPU 合成处理，不进入 CPU 布局
//! 3. 布局指纹哈希比对 → 指纹无变更直接跳过整棵子树布局计算
//! 4. 隔离规则自动限制元素布局影响范围，阻断布局向上冒泡传播

use std::collections::HashMap;
use std::hash::{Hash, Hasher};

/// 布局节点指纹 — 用于快速判断布局是否变化
#[derive(Debug, Clone)]
pub struct LayoutFingerprint {
    /// 节点 DOM 索引
    pub dom_index: usize,
    /// 标签名
    pub tag_name: String,
    /// 哈希值（基于所有影响布局的属性计算）
    pub hash: u64,
    /// 子节点指纹列表（用于递归比较）
    pub children: Vec<LayoutFingerprint>,
    /// 该子树是否已缓存
    pub is_cached: bool,
}

impl LayoutFingerprint {
    pub fn new(dom_index: usize, tag_name: &str) -> Self {
        Self {
            dom_index,
            tag_name: tag_name.to_string(),
            hash: 0,
            children: Vec::new(),
            is_cached: false,
        }
    }

    /// 计算指纹哈希
    pub fn compute_hash(&mut self, style_hash: u64, child_hashes: &[u64]) {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        self.dom_index.hash(&mut hasher);
        self.tag_name.hash(&mut hasher);
        style_hash.hash(&mut hasher);
        for ch in child_hashes {
            ch.hash(&mut hasher);
        }
        self.hash = hasher.finish();
    }
}

/// 已缓存的布局子树
#[allow(dead_code)]
struct CachedLayoutSubtree {
    /// 根节点指纹
    fingerprint: LayoutFingerprint,
    /// 缓存的布局节点
    nodes: Vec<crate::renderer::taffy_layout::TaffyLayoutNode>,
    /// 缓存生成时的时间戳
    cached_at: std::time::Instant,
}

/// 布局缓存引擎
pub struct LayoutCache {
    /// 按根 DOM 索引缓存的子树
    subtrees: HashMap<usize, CachedLayoutSubtree>,
    /// 缓存命中统计
    hits: u64,
    misses: u64,
    /// 全局布局指纹（用于全树比较）
    global_fingerprint: Option<LayoutFingerprint>,
    /// 上次计算的 DOM 版本号（用于判断 DOM 是否变更）
    last_dom_version: usize,
}

impl LayoutCache {
    pub fn new() -> Self {
        Self {
            subtrees: HashMap::new(),
            hits: 0,
            misses: 0,
            global_fingerprint: None,
            last_dom_version: 0,
        }
    }

    /// 检查布局缓存是否可用
    ///
    /// 返回 `true` 表示缓存命中，可直接使用缓存结果
    pub fn try_use_cache(
        &mut self,
        dom_version: usize,
        new_fingerprint: &LayoutFingerprint,
    ) -> bool {
        // DOM 版本不同 → 缓存无效
        if dom_version != self.last_dom_version {
            self.misses += 1;
            return false;
        }

        // 对比全局指纹
        if let Some(ref old) = self.global_fingerprint {
            if old.hash == new_fingerprint.hash {
                self.hits += 1;
                return true;
            }
        }

        self.misses += 1;
        false
    }

    /// 保存布局计算结果到缓存
    pub fn save_cache(
        &mut self,
        dom_version: usize,
        fingerprint: LayoutFingerprint,
        nodes: &[crate::renderer::taffy_layout::TaffyLayoutNode],
    ) {
        self.last_dom_version = dom_version;
        self.global_fingerprint = Some(fingerprint.clone());

        // 按子树分组缓存
        for node in nodes {
            if !self.subtrees.contains_key(&node.dom_node) {
                self.subtrees.insert(node.dom_node, CachedLayoutSubtree {
                    fingerprint: fingerprint.clone(),
                    nodes: vec![node.clone()],
                    cached_at: std::time::Instant::now(),
                });
            }
        }
    }

    /// 清空缓存
    pub fn clear(&mut self) {
        self.subtrees.clear();
        self.hits = 0;
        self.misses = 0;
        self.global_fingerprint = None;
        self.last_dom_version = 0;
    }

    /// 获取缓存统计
    pub fn stats(&self) -> (u64, u64) {
        (self.hits, self.misses)
    }

    /// 缓存命中率
    pub fn hit_rate(&self) -> f64 {
        let total = self.hits + self.misses;
        if total == 0 {
            0.0
        } else {
            self.hits as f64 / total as f64
        }
    }
}

impl Default for LayoutCache {
    fn default() -> Self {
        Self::new()
    }
}
