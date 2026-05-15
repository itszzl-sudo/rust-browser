//! 标签页管理模块

use crate::browser::engine::BrowserError;
use log::{debug, info};
use std::collections::HashMap;

/// 标签页
#[derive(Debug, Clone)]
pub struct Tab {
    /// 标签页 ID
    pub id: usize,
    /// 标签页标题
    pub title: Option<String>,
    /// 当前 URL
    pub url: String,
    /// 页面图标 URL
    pub favicon: Option<String>,
    /// 是否正在加载
    pub is_loading: bool,
    /// 是否是活跃标签页
    pub is_active: bool,
}

impl Tab {
    /// 创建新标签页
    pub fn new(id: usize) -> Self {
        Self {
            id,
            title: Some("新标签页".to_string()),
            url: "about:blank".to_string(),
            favicon: None,
            is_loading: false,
            is_active: true,
        }
    }
}

/// 历史记录条目
#[derive(Debug, Clone)]
pub struct HistoryEntry {
    /// URL
    pub url: String,
    /// 标题
    pub title: Option<String>,
}

/// 标签页管理器
pub struct TabManager {
    /// 标签页列表
    tabs: Vec<Tab>,
    /// 当前活跃标签页索引
    active_index: usize,
    /// 每个标签页的历史记录
    histories: HashMap<usize, Vec<HistoryEntry>>,
    /// 每个标签页的历史位置
    history_positions: HashMap<usize, usize>,
    /// 下一个可用标签页 ID
    next_tab_id: usize,
}

impl TabManager {
    /// 创建新的标签页管理器
    pub fn new() -> Self {
        info!("初始化标签页管理器");
        Self {
            tabs: vec![Tab::new(0)],
            active_index: 0,
            histories: HashMap::new(),
            history_positions: HashMap::new(),
            next_tab_id: 1,
        }
    }

    /// 获取活跃标签页
    pub fn active_tab(&self) -> Option<&Tab> {
        self.tabs.get(self.active_index)
    }

    /// 获取活跃标签页（可变）
    pub fn active_tab_mut(&mut self) -> Option<&mut Tab> {
        self.tabs.get_mut(self.active_index)
    }

    /// 获取活跃标签页索引
    pub fn active_index(&self) -> usize {
        self.active_index
    }

    /// 获取所有标签页
    pub fn tabs(&self) -> &[Tab] {
        &self.tabs
    }

    /// 获取所有标签页（可变）
    pub fn tabs_mut(&mut self) -> &mut [Tab] {
        &mut self.tabs
    }

    /// 创建新标签页
    pub fn new_tab(&mut self) -> &mut Tab {
        let id = self.next_tab_id;
        self.next_tab_id += 1;
        
        // 取消所有标签页的活跃状态
        for tab in &mut self.tabs {
            tab.is_active = false;
        }
        
        let mut tab = Tab::new(id);
        tab.is_active = true;
        
        self.histories.insert(id, Vec::new());
        self.history_positions.insert(id, 0);
        
        self.tabs.push(tab);
        self.active_index = self.tabs.len() - 1;
        
        debug!("创建新标签页: {}", id);
        self.tabs.last_mut().unwrap()
    }

    /// 关闭标签页
    pub fn close_tab(&mut self, index: usize) -> Result<(), BrowserError> {
        if self.tabs.len() <= 1 {
            return Err(BrowserError::NavigationError("无法关闭最后一个标签页".to_string()));
        }
        
        let tab_id = self.tabs[index].id;
        self.tabs.remove(index);
        self.histories.remove(&tab_id);
        self.history_positions.remove(&tab_id);
        
        // 调整活跃索引
        if self.active_index >= self.tabs.len() {
            self.active_index = self.tabs.len() - 1;
        }
        
        // 更新活跃状态
        for (i, tab) in self.tabs.iter_mut().enumerate() {
            tab.is_active = i == self.active_index;
        }
        
        debug!("关闭标签页，剩余: {}", self.tabs.len());
        Ok(())
    }

    /// 切换到指定标签页
    pub fn switch_to(&mut self, index: usize) {
        if index < self.tabs.len() {
            for (i, tab) in self.tabs.iter_mut().enumerate() {
                tab.is_active = i == index;
            }
            self.active_index = index;
            debug!("切换到标签页: {}", index);
        }
    }

    /// 更新活跃标签页信息
    pub fn update_active_tab(&mut self, url: &str, title: Option<String>) {
        if let Some(tab) = self.tabs.get_mut(self.active_index) {
            tab.url = url.to_string();
            tab.title = title.clone();
            
            // 添加到历史记录
            let entry = HistoryEntry {
                url: url.to_string(),
                title: title.clone(),
            };
            
            let pos = self.history_positions.entry(tab.id).or_insert(0);
            
            // 如果当前位置不是历史记录的末尾，清除后面的记录
            if let Some(history) = self.histories.get_mut(&tab.id) {
                let pos_usize = *pos;
                if pos_usize < history.len() {
                    history.truncate(pos_usize);
                }
                history.push(entry);
                *pos = history.len() - 1;
            }
        }
    }

    /// 设置活跃标签页的加载状态
    pub fn set_loading(&mut self, loading: bool) {
        if let Some(tab) = self.tabs.get_mut(self.active_index) {
            tab.is_loading = loading;
        }
    }

    /// 检查是否可以后退
    pub fn can_go_back(&self) -> bool {
        if let Some(tab) = self.active_tab() {
            if let Some(pos) = self.history_positions.get(&tab.id) {
                return *pos > 0;
            }
        }
        false
    }

    /// 检查是否可以前进
    pub fn can_go_forward(&self) -> bool {
        if let Some(tab) = self.active_tab() {
            if let Some(pos) = self.history_positions.get(&tab.id) {
                if let Some(history) = self.histories.get(&tab.id) {
                    return *pos < history.len() - 1;
                }
            }
        }
        false
    }

    /// 获取后退的 URL
    pub fn go_back_url(&self) -> Option<String> {
        if let Some(tab) = self.active_tab() {
            if let Some(pos) = self.history_positions.get(&tab.id) {
                if *pos > 0 {
                    if let Some(history) = self.histories.get(&tab.id) {
                        return Some(history[*pos - 1].url.clone());
                    }
                }
            }
        }
        None
    }

    /// 获取前进的 URL
    pub fn go_forward_url(&self) -> Option<String> {
        if let Some(tab) = self.active_tab() {
            if let Some(pos) = self.history_positions.get(&tab.id) {
                if let Some(history) = self.histories.get(&tab.id) {
                    if *pos < history.len() - 1 {
                        return Some(history[*pos + 1].url.clone());
                    }
                }
            }
        }
        None
    }

    /// 后退
    pub fn go_back(&mut self) -> Option<String> {
        if self.can_go_back() {
            if let Some(tab) = self.tabs.get_mut(self.active_index) {
                if let Some(pos) = self.history_positions.get_mut(&tab.id) {
                    *pos -= 1;
                    if let Some(history) = self.histories.get(&tab.id) {
                        let entry = &history[*pos];
                        tab.url = entry.url.clone();
                        tab.title = entry.title.clone();
                        return Some(entry.url.clone());
                    }
                }
            }
        }
        None
    }

    /// 前进
    pub fn go_forward(&mut self) -> Option<String> {
        if self.can_go_forward() {
            if let Some(tab) = self.tabs.get_mut(self.active_index) {
                if let Some(pos) = self.history_positions.get_mut(&tab.id) {
                    *pos += 1;
                    if let Some(history) = self.histories.get(&tab.id) {
                        let entry = &history[*pos];
                        tab.url = entry.url.clone();
                        tab.title = entry.title.clone();
                        return Some(entry.url.clone());
                    }
                }
            }
        }
        None
    }

    /// 重新加载当前页面
    pub fn reload_url(&self) -> Option<String> {
        self.active_tab().map(|tab| tab.url.clone())
    }
}

impl Default for TabManager {
    fn default() -> Self {
        Self::new()
    }
}
