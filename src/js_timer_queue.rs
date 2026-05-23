//! JS Timer Queue — 真实的 JS 定时器调度系统
//!
//! 用 Rust 的 `std::time::Instant` 实现 `setTimeout` / `setInterval` /
//! `requestAnimationFrame` 的真实异步版本。
//!
//! 每个定时器项记录：
//! - 到期时间
//! - 回调 ID（对应 JS 函数引用）
//! - 是否是 interval（循环）
//! - interval 间隔
//!
//! 本模块不依赖 boa_engine 或 deno_core，可在任何纯 Rust 环境中使用。

use std::collections::VecDeque;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// 定时器类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimerType {
    /// setTimeout — 一次性
    Timeout,
    /// setInterval — 循环
    Interval,
    /// requestAnimationFrame — 帧同步
    AnimationFrame,
}

/// 定时器条目
#[derive(Debug, Clone)]
pub struct TimerEntry {
    /// 定时器 ID（由 JS setTimeout/setInterval/requestAnimationFrame 返回的值）
    pub id: u32,
    /// 定时器类型
    pub timer_type: TimerType,
    /// 到期时间
    pub deadline: Instant,
    /// 如果是 interval，重复间隔
    pub interval_ms: u64,
    /// 回调函数 ID（JS 引擎内部用于查找回调）
    pub callback_id: u32,
    /// 是否已取消
    pub cancelled: bool,
}

/// 定时器管理器
pub struct TimerQueue {
    /// 按到期时间排序的定时器
    timers: Vec<TimerEntry>,
    /// 下次分配的 ID
    next_id: u32,
    /// 已到期待触发的回调（按顺序）
    pending_callbacks: VecDeque<(u32, TimerType, u32)>, // (timer_id, type, callback_id)
    /// 最后一次检查时间
    #[allow(dead_code)]
    last_check: Instant,
}

impl TimerQueue {
    pub fn new() -> Self {
        Self {
            timers: Vec::new(),
            next_id: 1,
            pending_callbacks: VecDeque::new(),
            last_check: Instant::now(),
        }
    }

    /// 注册 setTimeout — 返回 timer ID
    pub fn set_timeout(&mut self, callback_id: u32, ms: u64) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        let deadline = Instant::now() + Duration::from_millis(ms);

        self.timers.push(TimerEntry {
            id,
            timer_type: TimerType::Timeout,
            deadline,
            interval_ms: 0,
            callback_id,
            cancelled: false,
        });

        // 按到期时间排序
        self.timers.sort_by_key(|t| t.deadline);

        id
    }

    /// 注册 setInterval — 返回 timer ID
    pub fn set_interval(&mut self, callback_id: u32, ms: u64) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        let deadline = Instant::now() + Duration::from_millis(ms);

        self.timers.push(TimerEntry {
            id,
            timer_type: TimerType::Interval,
            deadline,
            interval_ms: ms,
            callback_id,
            cancelled: false,
        });

        self.timers.sort_by_key(|t| t.deadline);

        id
    }

    /// 注册 requestAnimationFrame — 返回 timer ID
    pub fn request_animation_frame(&mut self, callback_id: u32) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        // 动画帧在 16ms 后触发（~60fps）
        let deadline = Instant::now() + Duration::from_millis(16);

        self.timers.push(TimerEntry {
            id,
            timer_type: TimerType::AnimationFrame,
            deadline,
            interval_ms: 0,
            callback_id,
            cancelled: false,
        });

        id
    }

    /// 取消定时器
    pub fn clear_timer(&mut self, id: u32) {
        if let Some(timer) = self.timers.iter_mut().find(|t| t.id == id) {
            timer.cancelled = true;
        }
    }

    /// 检查到期的定时器，将它们的回调加入待触发队列
    ///
    /// 应在每帧渲染循环中调用
    pub fn tick(&mut self) {
        let now = Instant::now();
        self.last_check = now;

        let mut i = 0;
        while i < self.timers.len() {
            if self.timers[i].cancelled {
                self.timers.remove(i);
                continue;
            }

            if now >= self.timers[i].deadline {
                let entry = &self.timers[i];
                self.pending_callbacks
                    .push_back((entry.id, entry.timer_type, entry.callback_id));

                if entry.timer_type == TimerType::Interval {
                    // 循环：更新 deadline
                    self.timers[i].deadline = now + Duration::from_millis(entry.interval_ms);
                    i += 1;
                } else {
                    // 一次性的移除
                    self.timers.remove(i);
                }
            } else {
                i += 1;
            }
        }
    }

    /// 取出所有待触发的回调
    pub fn drain_pending(&mut self) -> Vec<(u32, TimerType, u32)> {
        self.pending_callbacks.drain(..).collect()
    }

    /// 返回下一个定时器的剩余时间（用于休眠优化）
    pub fn next_deadline(&self) -> Option<Duration> {
        self.timers
            .iter()
            .filter(|t| !t.cancelled)
            .min_by_key(|t| t.deadline)
            .map(|t| t.deadline.saturating_duration_since(Instant::now()))
    }

    /// 是否有待触发的回调
    pub fn has_pending(&self) -> bool {
        !self.pending_callbacks.is_empty()
    }

    /// 获取当前分配的最后一个 ID（用于在 JS 端生成回调键）
    pub fn next_id(&self) -> u32 {
        self.next_id
    }

    /// 定时器数量
    pub fn len(&self) -> usize {
        self.timers.len()
    }

    pub fn is_empty(&self) -> bool {
        self.timers.is_empty()
    }
}

impl Default for TimerQueue {
    fn default() -> Self {
        Self::new()
    }
}

// ═══════════════════════════════════════════════════════════════
// 全局定时器队列
//
// JS 引擎的 polyfill 原生函数通过此全局队列与 Rust 交互。
// 渲染循环每帧调用 GLOBAL_TIMER_QUEUE.lock().tick() 和
// drain_pending() 来触发 JS 回调。
// ═══════════════════════════════════════════════════════════════

use lazy_static::lazy_static;

lazy_static! {
    /// 全局定时器队列实例
    ///
    /// 通过 Mutex 实现线程安全，可在任何线程访问。
    /// JS 原生函数（nativeSetTimeout 等）通过此队列注册定时器，
    /// 渲染循环在每帧检查并触发到期回调。
    pub static ref GLOBAL_TIMER_QUEUE: Mutex<TimerQueue> = Mutex::new(TimerQueue::new());
}
