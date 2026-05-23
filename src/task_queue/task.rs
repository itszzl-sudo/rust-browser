//! Task types and priority levels.
//!
//! This module defines the fundamental building blocks of the task system:
//!
//! - [`TaskPriority`] — mirrors Chromium's `base::TaskPriority`
//! - [`TaskTraits`] — metadata attached to every task (name, priority, etc.)
//! - [`Task`] — a single unit of work (like Chrome's `base::OnceClosure`)
//! - [`RepeatingTask`] — a task that fires periodically
//! - [`TaskHandle`] — a cancellation token for pending tasks

use log::trace;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

/// Global counter for assigning unique IDs to every task.
static NEXT_TASK_ID: AtomicU64 = AtomicU64::new(1);

// ---------------------------------------------------------------------------
// Priority
// ---------------------------------------------------------------------------

/// Task priority levels, mirroring Chromium's `base::TaskPriority`.
///
/// The ordering determines scheduling preference: higher-priority tasks are
/// dispatched and executed before lower-priority ones.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum TaskPriority {
    /// Background / best-effort work (e.g., indexing, cleaning, analytics).
    /// These tasks must never block the user's experience.
    BestEffort = 0,

    /// User-visible but non-blocking work (e.g., decoding images, loading
    /// secondary resources).  Users are aware of this work but it does not
    /// directly block interaction.
    UserVisible = 1,

    /// User-blocking work with the highest priority (e.g., input events,
    /// rendering, layout).  Users are directly waiting for these tasks.
    UserBlocking = 2,
}

impl TaskPriority {
    /// Returns the highest possible priority ([`TaskPriority::UserBlocking`]).
    pub fn highest() -> Self {
        TaskPriority::UserBlocking
    }

    /// Returns the lowest possible priority ([`TaskPriority::BestEffort`]).
    pub fn lowest() -> Self {
        TaskPriority::BestEffort
    }
}

// ---------------------------------------------------------------------------
// Task traits
// ---------------------------------------------------------------------------

/// Metadata describing a task's properties, mirroring Chromium's
/// `base::TaskTraits`.
///
/// Every task carries a [`TaskTraits`] value that the scheduler uses to decide
/// which thread pool to use and how to prioritise the work.
#[derive(Debug, Clone)]
pub struct TaskTraits {
    /// Human-readable name for debugging / tracing.
    pub name: &'static str,

    /// Scheduling priority.
    pub priority: TaskPriority,

    /// Whether this task may perform blocking I/O.
    pub may_block: bool,

    /// Whether this task may run for an extended period of time.
    pub may_run_long: bool,
}

impl TaskTraits {
    /// Returns a default [`TaskTraits`] with `UserVisible` priority.
    pub fn default() -> Self {
        Self {
            name: "unnamed",
            priority: TaskPriority::UserVisible,
            may_block: false,
            may_run_long: false,
        }
    }

    /// Sets the priority and returns self (builder pattern).
    pub fn with_priority(mut self, p: TaskPriority) -> Self {
        self.priority = p;
        self
    }

    /// Sets the task name and returns self.
    pub fn named(mut self, name: &'static str) -> Self {
        self.name = name;
        self
    }

    /// Marks the task as potentially blocking and returns self.
    pub fn may_block(mut self) -> Self {
        self.may_block = true;
        self
    }

    /// Marks the task as potentially long-running and returns self.
    pub fn may_run_long(mut self) -> Self {
        self.may_run_long = true;
        self
    }
}

// ---------------------------------------------------------------------------
// Task
// ---------------------------------------------------------------------------

/// A single unit of work, analogous to Chrome's `base::OnceClosure`.
///
/// A [`Task`] wraps a `Box<dyn FnOnce() + Send>` together with its
/// [`TaskTraits`] and a unique ID for tracing.
pub struct Task {
    /// Globally-unique identifier (assigned at creation).
    pub id: u64,

    /// Priority / metadata for the scheduler.
    pub traits: TaskTraits,

    /// The actual closure to execute.
    inner: Box<dyn FnOnce() + Send>,
}

impl Task {
    /// Creates a new [`Task`] from a closure and its traits.
    ///
    /// The task is assigned a unique, monotonically-increasing ID.
    pub fn new<F>(f: F, traits: TaskTraits) -> Self
    where
        F: FnOnce() + Send + 'static,
    {
        let id = NEXT_TASK_ID.fetch_add(1, Ordering::SeqCst);
        trace!("Task[{}] created: {}", id, traits.name);
        Self {
            id,
            traits,
            inner: Box::new(f),
        }
    }

    /// Consumes the task and executes its inner closure.
    pub fn run(self) {
        trace!("Task[{}] running: {}", self.id, self.traits.name);
        (self.inner)();
    }
}

// ---------------------------------------------------------------------------
// RepeatingTask
// ---------------------------------------------------------------------------

/// A task that executes repeatedly at a fixed interval.
///
/// This is a simplified version — a full implementation would integrate with
/// the scheduler's timer queue (like Chrome's `base::Timer`).
#[allow(dead_code)]
pub struct RepeatingTask {
    /// The underlying task (wrapped for optional cancellation).
    task: Arc<Mutex<Option<Task>>>,

    /// Interval between firings.
    interval: std::time::Duration,

    /// Cancellation flag shared with the background thread.
    cancelled: Arc<std::sync::atomic::AtomicBool>,
}

impl RepeatingTask {
    /// Creates a new [`RepeatingTask`] that runs `f` every `interval`.
    ///
    /// The closure is called at (approximately) the given interval until the
    /// task is dropped or cancelled.
    ///
    /// ## Implementation note
    ///
    /// The current implementation spawns a dedicated `std::thread` that loops
    /// with `thread::sleep(interval)` between executions. This is a simple
    /// approach suitable for periodic housekeeping tasks (e.g. garbage
    /// collection, metrics reporting).
    ///
    /// For a production-quality periodic timer, consider integrating with:
    ///   - a hierarchical timer wheel (e.g. `tokio`-style timing wheels)
    ///   - the scheduler's delayed task queue (for unified priority scheduling)
    ///
    /// The `TaskHandle` returned (via the `Arc<Mutex<Option<Task>>>`) can be
    /// used for cancellation: drop the handle or set its `cancelled` flag.
    pub fn new<F>(interval: std::time::Duration, _traits: TaskTraits, f: F) -> Self
    where
        F: Fn() + Send + 'static,
    {
        let task = Arc::new(Mutex::new(None));
        let cancelled = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let cancelled_clone = cancelled.clone();

        // 启动后台线程按 interval 重复执行回调
        std::thread::Builder::new()
            .name(format!("RepeatingTask-{}", _traits.name))
            .spawn(move || {
                while !cancelled_clone.load(std::sync::atomic::Ordering::SeqCst) {
                    f();
                    std::thread::sleep(interval);
                }
            })
            .ok();

        Self {
            task,
            interval,
            cancelled,
        }
    }

    /// Returns the interval between firings.
    pub fn interval(&self) -> std::time::Duration {
        self.interval
    }

    /// Stops the repeating task.
    pub fn cancel(&self) {
        self.cancelled
            .store(true, std::sync::atomic::Ordering::SeqCst);
    }
}

impl Drop for RepeatingTask {
    fn drop(&mut self) {
        self.cancelled
            .store(true, std::sync::atomic::Ordering::SeqCst);
    }
}

// ---------------------------------------------------------------------------
// TaskHandle (cancellation token)
// ---------------------------------------------------------------------------

/// A handle that allows cancelling a pending task.
///
/// Works similarly to a cancellation token: check [`is_cancelled`](TaskHandle::is_cancelled)
/// inside the task body, or use the shared [`AtomicBool`] directly.
#[derive(Clone)]
pub struct TaskHandle {
    cancelled: Arc<AtomicBool>,
}

impl TaskHandle {
    /// Creates a new [`TaskHandle`] in the non-cancelled state.
    pub fn new() -> Self {
        Self {
            cancelled: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Requests cancellation of the associated task.
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
    }

    /// Returns `true` if cancellation has been requested.
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }

    /// Returns a reference to the shared [`AtomicBool`] flag so external code
    /// can poll it efficiently.
    pub fn shared_flag(&self) -> Arc<AtomicBool> {
        self.cancelled.clone()
    }
}

impl Default for TaskHandle {
    fn default() -> Self {
        Self::new()
    }
}
