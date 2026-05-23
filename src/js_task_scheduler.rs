//! JS Task Scheduler — JS 任务分级调度
//!
//! 实现 Arc 浏览器的"JS 任务分级管控等价变换"：
//!
//! 分级策略:
//! - 高优先级: DOM 几何修改、布局相关样式、页面交互 JS → 即时执行
//! - 中优先级: 页面渲染辅助逻辑 → 分片拆分执行
//! - 低优先级: 广告、统计、上报、后台心跳 JS → 节流延迟执行
//!
//! 核心机制:
//! 1. 引擎自动隐式合并批量 DOM 读写 → 消除布局抖动
//! 2. 读写分离调度 → 统一批量读取布局数据、统一批量写入样式数据
//! 3. 代理转换: JS 修改 left/top/width/height → 自动转为 transform 位移缩放

use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// JS 任务分类
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JsTaskCategory {
    /// 交互响应（点击、键盘） — 最高优先级
    Interaction,
    /// 布局相关（DOM 几何变更、样式修改） — 高优先级
    LayoutAffecting,
    /// 渲染辅助（动画帧回调、requestAnimationFrame） — 中优先级
    RenderAuxiliary,
    /// 网络请求（fetch/XHR） — 中优先级
    Network,
    /// 后台任务（广告、统计、上报、埋点） — 低优先级
    Background,
}

/// JS 任务调度项
#[derive(Debug)]
pub struct JsTaskItem {
    /// 分类
    pub category: JsTaskCategory,
    /// 原始 JS 代码
    pub code: String,
    /// 提交时间
    pub submitted_at: Instant,
    /// 是否已执行
    pub executed: bool,
}

/// JS 任务调度器
pub struct JsTaskScheduler {
    /// 高优先级队列（交互 + 布局相关）
    high_priority: VecDeque<JsTaskItem>,
    /// 中优先级队列（渲染辅助 + 网络）
    medium_priority: VecDeque<JsTaskItem>,
    /// 低优先级队列（后台任务）
    low_priority: VecDeque<JsTaskItem>,
    /// 上次执行低优先级任务的时间（用于节流）
    last_low_priority_run: Instant,
    /// 低优先级任务的节流间隔
    throttle_interval: Duration,
    /// 批量 DOM 写操作缓冲区（用于合并）
    pending_dom_writes: Vec<(String, String, String)>, // (selector, property, value)
    /// 批量 DOM 读操作缓冲区（用于读写分离）
    pending_dom_reads: Vec<String>, // selectors to read
}

impl JsTaskScheduler {
    pub fn new() -> Self {
        Self {
            high_priority: VecDeque::new(),
            medium_priority: VecDeque::new(),
            low_priority: VecDeque::new(),
            last_low_priority_run: Instant::now(),
            throttle_interval: Duration::from_millis(500), // 500ms 节流
            pending_dom_writes: Vec::new(),
            pending_dom_reads: Vec::new(),
        }
    }

    /// 提交 JS 任务到调度器
    pub fn submit(&mut self, category: JsTaskCategory, code: String) {
        let item = JsTaskItem {
            category,
            code,
            submitted_at: Instant::now(),
            executed: false,
        };

        match category {
            JsTaskCategory::Interaction | JsTaskCategory::LayoutAffecting => {
                self.high_priority.push_back(item);
            }
            JsTaskCategory::RenderAuxiliary | JsTaskCategory::Network => {
                self.medium_priority.push_back(item);
            }
            JsTaskCategory::Background => {
                self.low_priority.push_back(item);
            }
        }
    }

    /// 获取下一个应执行的任务（按优先级）
    pub fn next_task(&mut self) -> Option<JsTaskItem> {
        // 高优先级：总是立即返回
        if let Some(item) = self.high_priority.pop_front() {
            return Some(item);
        }

        // 中优先级：如果有空间则执行
        if let Some(item) = self.medium_priority.pop_front() {
            return Some(item);
        }

        // 低优先级：需要节流
        if Instant::now().duration_since(self.last_low_priority_run) >= self.throttle_interval {
            if let Some(item) = self.low_priority.pop_front() {
                self.last_low_priority_run = Instant::now();
                return Some(item);
            }
        }

        None
    }

    /// 批量提交 DOM 写操作（引擎内部自动合并）
    pub fn submit_dom_write(&mut self, selector: &str, property: &str, value: &str) {
        // 查找是否有对同一 selector + property 的待处理写入
        let existing = self.pending_dom_writes.iter_mut().find(|(s, p, _)| {
            s == selector && p == property
        });
        if let Some(entry) = existing {
            entry.2 = value.to_string(); // 更新值（合并）
        } else {
            self.pending_dom_writes
                .push((selector.to_string(), property.to_string(), value.to_string()));
        }
    }

    /// 批量提交 DOM 读操作
    pub fn submit_dom_read(&mut self, selector: &str) {
        self.pending_dom_reads.push(selector.to_string());
    }

    /// 刷新所有待处理的 DOM 写操作（批量执行）
    pub fn flush_dom_writes(&mut self) -> Vec<(String, String, String)> {
        std::mem::take(&mut self.pending_dom_writes)
    }

    /// 刷新所有待处理的 DOM 读操作
    pub fn flush_dom_reads(&mut self) -> Vec<String> {
        std::mem::take(&mut self.pending_dom_reads)
    }

    /// 判断是否可以将高成本布局属性映射为合成属性
    ///
    /// 例如：`left: X` → `transform: translateX(X)` 的等价映射
    pub fn should_map_to_composite(property: &str) -> bool {
        matches!(
            property,
            "left" | "top" | "width" | "height" | "margin" | "padding"
        )
    }

    /// 将高成本属性映射为合成属性值
    ///
    /// 引擎内部等价转换：JS 开发者无需修改代码
    pub fn map_to_composite_property(property: &str, value: &str) -> (String, String) {
        match property {
            "left" => ("transform".to_string(), format!("translateX({})", value)),
            "top" => ("transform".to_string(), format!("translateY({})", value)),
            "width" => ("transform".to_string(), format!("scaleX({})", value)),
            "height" => ("transform".to_string(), format!("scaleY({})", value)),
            _ => (property.to_string(), value.to_string()),
        }
    }

    /// 获取队列统计
    pub fn queue_sizes(&self) -> (usize, usize, usize) {
        (
            self.high_priority.len(),
            self.medium_priority.len(),
            self.low_priority.len(),
        )
    }

    /// 清空所有队列
    pub fn clear(&mut self) {
        self.high_priority.clear();
        self.medium_priority.clear();
        self.low_priority.clear();
        self.pending_dom_writes.clear();
        self.pending_dom_reads.clear();
    }
}

impl Default for JsTaskScheduler {
    fn default() -> Self {
        Self::new()
    }
}
