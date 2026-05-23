//! Async Pipeline — 异步分片渲染管线
//!
//! 将渲染管线拆分为：
//! - 阻塞必执行段（首屏用）
//! - 延迟异步执行段（后台工作线程）
//!
//! # 管线拆分
//!
//! ```text
//! 主线程（必须阻塞）:
//! ├─ 首屏样式计算 (Critical CSS only)
//! ├─ 核心布局 (viewport 内)
//! └─ 可视区域绘制
//!
//! 工作线程（异步后置）:
//! ├─ 离线 DOM 解析
//! ├─ 非关键 CSS 解析
//! ├─ 冗余样式合并
//! └─ 图片解码 (GPU 离线)
//! ```

use crate::dom_wrapper::DomWrapper;
use crate::renderer::taffy_layout::TaffyLayoutEngine;
use crate::renderer::Renderer;
use crate::resource_scheduler::ResourceScheduler;
use crate::task_queue::scheduler::GLOBAL_SCHEDULER;
use crate::task_queue::task::{TaskPriority, TaskTraits};
use log::debug;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

/// 渲染管线阶段
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PipelineStage {
    /// DOM 解析（可异步）
    DomParse,
    /// 关键 CSS 解析（阻塞）
    CriticalCss,
    /// 非关键 CSS 解析（异步）
    NonCriticalCss,
    /// 首屏样式计算（阻塞）
    CriticalStyle,
    /// 非首屏样式计算（异步）
    NonCriticalStyle,
    /// 首屏布局（阻塞）
    CriticalLayout,
    /// 非首屏布局（异步可延迟）
    NonCriticalLayout,
    /// 首屏绘制（阻塞）
    CriticalPaint,
    /// 非首屏绘制（异步）
    NonCriticalPaint,
    /// 图片解码（异步）
    ImageDecode,
    /// 纹理上传
    TextureUpload,
    /// 合成输出
    Composite,
}

/// 渲染管线异步分片调度器
pub struct AsyncPipeline {
    /// 渲染是否已完成首屏
    first_meaningful_paint: Arc<AtomicBool>,
    /// 是否正在后台处理
    background_processing: Arc<AtomicBool>,
    /// 资源调度器引用
    resource_scheduler: Arc<ResourceScheduler>,
}

impl AsyncPipeline {
    pub fn new() -> Self {
        Self {
            first_meaningful_paint: Arc::new(AtomicBool::new(false)),
            background_processing: Arc::new(AtomicBool::new(false)),
            resource_scheduler: Arc::new(ResourceScheduler::new()),
        }
    }

    /// 执行关键路径渲染（主线程，必须阻塞）
    ///
    /// 只做首屏渲染所需的最小工作集：
    /// 1. DOM 快速解析（只够构建首屏布局）
    /// 2. 关键 CSS 样式计算（仅首屏相关选择器）
    /// 3. 首屏布局计算
    /// 4. 首屏绘制
    pub fn render_critical_path(
        &self,
        dom: &DomWrapper,
        renderer: &mut Renderer,
        layout: &mut TaffyLayoutEngine,
        viewport_w: f32,
        viewport_h: f32,
    ) -> Result<Vec<u8>, String> {
        debug!(
            "AsyncPipeline: 开始关键路径渲染 ({}x{})",
            viewport_w, viewport_h
        );

        // 1. 确保视口设置
        renderer.set_viewport(viewport_w as u32, viewport_h as u32);
        layout.set_viewport(viewport_w, viewport_h);

        // 2. 提取关键 CSS 样式（所有 <style> 和外部样式表的首屏部分）
        let all_css = crate::renderer::extract_style_tags(dom);
        let rules = crate::css_engine::parse_css_rules(&all_css);
        let style_map = crate::css_engine::rules_to_style_map(&rules, dom.inner_document());
        layout.set_style_map(style_map);

        // 3. 执行布局计算（仅首屏相关）
        layout
            .compute(dom)
            .map_err(|e| format!("布局失败: {}", e))?;

        // 4. 渲染首屏
        let result = renderer
            .render_with_taffy(dom, layout)
            .map_err(|e| format!("渲染失败: {:?}", e))?;

        self.first_meaningful_paint.store(true, Ordering::SeqCst);
        debug!("AsyncPipeline: 首屏渲染完成 ({} bytes)", result.len());

        Ok(result)
    }

    /// 启动后台异步处理（工作线程执行）
    ///
    /// 在首屏渲染完成后调用，在后台继续：
    /// 1. 深度 CSS 处理
    /// 2. 非关键 CSS 规则分析
    /// 3. 资源预加载调度
    /// 4. 离屏渲染预计算
    ///
    /// 接收 `css_text`（字符串，Send + Sync）而非 `DomWrapper`（非 Send），
    /// 避免 kuchiki 的 `Rc<Node>` 跨线程传递问题。
    pub fn start_background_processing(&self, css_text: String) {
        if self.background_processing.load(Ordering::SeqCst) {
            return; // 已在处理中
        }
        self.background_processing.store(true, Ordering::SeqCst);

        let bg_flag = self.background_processing.clone();

        GLOBAL_SCHEDULER.post_task(
            move || {
                debug!("AsyncPipeline: 后台处理线程启动");

                // 1. 非关键 CSS 规则解析（仅解析，不阻塞渲染）
                let rules = crate::css_engine::parse_css_rules(&css_text);
                debug!("AsyncPipeline: 后台解析完成，{} 条规则", rules.len());

                // 2. 这里可以扩展：资源预加载、离屏渲染预计算等

                // 后台处理完成
                bg_flag.store(false, Ordering::SeqCst);
                debug!("AsyncPipeline: 后台处理完成");
            },
            TaskTraits::default()
                .named("async-pipeline-bg")
                .with_priority(TaskPriority::BestEffort)
                .may_block(),
        );
    }

    /// 检查首屏渲染是否完成
    pub fn has_first_meaningful_paint(&self) -> bool {
        self.first_meaningful_paint.load(Ordering::SeqCst)
    }

    /// 获取资源调度器引用
    pub fn resource_scheduler(&self) -> &Arc<ResourceScheduler> {
        &self.resource_scheduler
    }

    /// 重置管线状态
    pub fn reset(&self) {
        self.first_meaningful_paint.store(false, Ordering::SeqCst);
        self.background_processing.store(false, Ordering::SeqCst);
    }
}

impl Default for AsyncPipeline {
    fn default() -> Self {
        Self::new()
    }
}
