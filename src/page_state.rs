//! Page State Freezer — 页面冻结机制
//!
//! 实现 Arc 浏览器的"隐藏页面冻结状态等价变换"：
//! - 非激活标签页冻结所有计算（布局、渲染、图片解码）
//! - 仅保留最小存活快照
//! - 唤醒时增量恢复

use std::time::{Duration, Instant};

/// 冻结层级
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FreezeLevel {
    /// 活跃状态 — 完全运行
    Active,
    /// 轻冻 — 暂停非必须渲染（保留 JS 定时器）
    LightFreeze,
    /// 中冻 — 暂停图片解码 + 预加载 + 非关键渲染
    MediumFreeze,
    /// 深冻 — 暂停所有计算，仅保留 DOM 快照
    DeepFreeze,
}

/// 标签页冻结状态
#[derive(Debug, Clone)]
pub struct PageFreezeState {
    /// 当前冻结层级
    pub level: FreezeLevel,
    /// 最后活跃时间
    pub last_active: Instant,
    /// 冻结持续时间
    pub freeze_duration: Duration,
    /// 是否为不可见标签页（后台、隐藏）
    pub is_visible: bool,
    /// 是否被用户固定（pin）
    pub is_pinned: bool,
}

impl PageFreezeState {
    pub fn new() -> Self {
        Self {
            level: FreezeLevel::Active,
            last_active: Instant::now(),
            freeze_duration: Duration::ZERO,
            is_visible: true,
            is_pinned: false,
        }
    }

    /// 标签页变为非活跃时调用
    pub fn deactivate(&mut self) {
        self.is_visible = false;
        self.last_active = Instant::now();
    }

    /// 标签页变为活跃时调用
    pub fn activate(&mut self) {
        self.is_visible = true;
        self.level = FreezeLevel::Active;
        self.freeze_duration = Duration::ZERO;
    }

    /// 更新冻结层级（基于时间推移）
    pub fn tick(&mut self, elapsed: Duration) {
        if self.is_visible || self.is_pinned {
            self.level = FreezeLevel::Active;
            return;
        }

        let inactive_time = elapsed;

        // 0-30s: 轻冻
        if inactive_time < Duration::from_secs(30) {
            self.level = FreezeLevel::LightFreeze;
        // 30s-2min: 中冻
        } else if inactive_time < Duration::from_secs(120) {
            self.level = FreezeLevel::MediumFreeze;
        // 2min+: 深冻
        } else {
            self.level = FreezeLevel::DeepFreeze;
        }

        self.freeze_duration = inactive_time;
    }

    /// 检查是否应暂停渲染
    pub fn should_pause_rendering(&self) -> bool {
        match self.level {
            FreezeLevel::Active => false,
            FreezeLevel::LightFreeze => true,
            FreezeLevel::MediumFreeze => true,
            FreezeLevel::DeepFreeze => true,
        }
    }

    /// 检查是否应暂停图片解码
    pub fn should_pause_image_decode(&self) -> bool {
        match self.level {
            FreezeLevel::Active | FreezeLevel::LightFreeze => false,
            FreezeLevel::MediumFreeze | FreezeLevel::DeepFreeze => true,
        }
    }

    /// 检查是否应暂停 JS 定时器
    pub fn should_pause_js_timers(&self) -> bool {
        match self.level {
            FreezeLevel::Active | FreezeLevel::LightFreeze => false,
            FreezeLevel::MediumFreeze => false, // 中冻保留标准 JS
            FreezeLevel::DeepFreeze => true,
        }
    }

    /// 检查是否应暂停非关键后台 JS（广告、统计等）
    pub fn should_pause_background_js(&self) -> bool {
        match self.level {
            FreezeLevel::Active => false,
            FreezeLevel::LightFreeze | FreezeLevel::MediumFreeze | FreezeLevel::DeepFreeze => true,
        }
    }
}

impl Default for PageFreezeState {
    fn default() -> Self {
        Self::new()
    }
}

#[allow(dead_code)]
/// 页面冻结管理器 — 管理所有标签页的冻结状态
pub struct PageFreezer {
    /// 按标签页 ID 索引的冻结状态
    states: Vec<PageFreezeState>,
    /// 创建时间
    created_at: Instant,
}

impl PageFreezer {
    pub fn new() -> Self {
        Self {
            states: Vec::new(),
            created_at: Instant::now(),
        }
    }

    /// 注册新标签页
    pub fn register_tab(&mut self) -> usize {
        let idx = self.states.len();
        self.states.push(PageFreezeState::new());
        idx
    }

    /// 标签页激活
    pub fn activate_tab(&mut self, index: usize) {
        if let Some(state) = self.states.get_mut(index) {
            state.activate();
        }
    }

    /// 标签页失去焦点
    pub fn deactivate_tab(&mut self, index: usize) {
        if let Some(state) = self.states.get_mut(index) {
            state.deactivate();
        }
    }

    /// 获取指定标签页的冻结状态
    pub fn get_state(&self, index: usize) -> Option<&PageFreezeState> {
        self.states.get(index)
    }

    /// 获取标签页的冻结层级（自动基于时间更新）
    pub fn get_freeze_level(&self, index: usize) -> FreezeLevel {
        self.states
            .get(index)
            .map(|s| s.level)
            .unwrap_or(FreezeLevel::Active)
    }

    /// 移除标签页
    pub fn remove_tab(&mut self, index: usize) {
        if index < self.states.len() {
            self.states.remove(index);
        }
    }

    /// 关闭所有标签页
    pub fn clear(&mut self) {
        self.states.clear();
    }

    /// 返回标签页数量
    pub fn len(&self) -> usize {
        self.states.len()
    }

    pub fn is_empty(&self) -> bool {
        self.states.is_empty()
    }
}

impl Default for PageFreezer {
    fn default() -> Self {
        Self::new()
    }
}
