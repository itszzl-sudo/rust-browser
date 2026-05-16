//! Global task scheduler.
//!
//! This module provides a Chrome-style [`TaskScheduler`] that manages two
//! thread pools — a **foreground** pool for user-blocking and user-visible
//! work, and a **background** pool for best-effort work — together with a
//! [`MainThreadRunner`] for single-threaded tasks that must run on the
//! browser's primary thread.
//!
//! # Priority routing
//!
//! | Priority         | Pool        | Notes                         |
//! |------------------|-------------|-------------------------------|
//! | `UserBlocking`   | Foreground  | Input, rendering, layout      |
//! | `UserVisible`    | Foreground  | Image decoding, sub-resources |
//! | `BestEffort`     | Background  | Indexing, cleaning, analytics |
//!
//! A global singleton [`GLOBAL_SCHEDULER`] is provided via `lazy_static` for
//! convenient access throughout the application.

use crate::task_queue::task::{Task, TaskPriority, TaskTraits};
use crate::task_queue::thread_pool::ThreadPool;
use log::{debug, info};
use std::sync::Arc;

// ---------------------------------------------------------------------------
// MainThreadRunner
// ---------------------------------------------------------------------------

/// A single-threaded task queue intended for work that must run on the
/// browser's main / UI thread.
///
/// Callers post tasks via [`post_task`](MainThreadRunner::post_task) and the
/// main thread's event loop should periodically invoke
/// [`run_pending_tasks`](MainThreadRunner::run_pending_tasks) to drain the
/// queue.
///
/// This mirrors Chrome's `SequencedTaskRunner` for the main thread.
pub struct MainThreadRunner {
    /// Pending tasks, stored in FIFO order.
    pending: Arc<std::sync::Mutex<Vec<Task>>>,
}

impl MainThreadRunner {
    /// Creates an empty [`MainThreadRunner`].
    pub fn new() -> Self {
        Self {
            pending: Arc::new(std::sync::Mutex::new(Vec::new())),
        }
    }

    /// Enqueues a task for later execution on the main thread.
    pub fn post_task(&self, task: Task) {
        self.pending.lock().unwrap().push(task);
    }

    /// Runs all currently-pending tasks and returns the number executed.
    ///
    /// **Must be called from the main thread.**  Tasks are drained atomically
    /// so new tasks posted during iteration will be picked up on the next
    /// call.
    pub fn run_pending_tasks(&self) -> usize {
        let tasks = std::mem::take(&mut *self.pending.lock().unwrap());
        let count = tasks.len();
        for task in tasks {
            task.run();
        }
        count
    }

    /// Returns `true` if there are unprocessed tasks in the queue.
    pub fn has_pending(&self) -> bool {
        !self.pending.lock().unwrap().is_empty()
    }
}

impl Default for MainThreadRunner {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// TaskScheduler
// ---------------------------------------------------------------------------

/// Chrome-style [`TaskScheduler`] that manages foreground and background
/// thread pools, plus a main-thread runner.
///
/// # Example
///
/// ```ignore
/// use rust_browser::task_queue::scheduler::{TaskScheduler, GLOBAL_SCHEDULER};
/// use rust_browser::task_queue::task::{TaskTraits, TaskPriority};
///
/// // Use the global scheduler:
/// GLOBAL_SCHEDULER.post_task(
///     || println!("Hello from the thread pool!"),
///     TaskTraits::default().named("hello").with_priority(TaskPriority::UserBlocking),
/// );
///
/// // Or create a custom one:
/// let scheduler = TaskScheduler::new("Renderer");
/// scheduler.post_to_main(
///     || println!("On the main thread"),
///     TaskTraits::default().named("ui-update"),
/// );
/// scheduler.main_thread().run_pending_tasks();
/// ```
pub struct TaskScheduler {
    /// Human-readable name for logging.
    name: String,

    /// Foreground thread pool — handles `UserBlocking` and `UserVisible`
    /// tasks.  Sized to ~75% of available cores (min 2).
    foreground: ThreadPool,

    /// Background thread pool — handles `BestEffort` tasks.
    /// Sized to ~25% of available cores (min 1).
    background: ThreadPool,

    /// Single-threaded runner for main-thread-only operations.
    main_thread_runner: Arc<MainThreadRunner>,
}

impl TaskScheduler {
    /// Creates a new [`TaskScheduler`].
    ///
    /// The thread pool sizes are computed automatically based on the number
    /// of available CPU cores.
    pub fn new(name: &str) -> Self {
        info!("Creating TaskScheduler '{}'", name);

        let num_cores = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4);

        // Foreground pool: 75 % of cores, minimum 2.
        let fg_threads = ((num_cores as f32 * 0.75).ceil() as usize).max(2);
        // Background pool: 25 % of cores, minimum 1.
        let bg_threads = (num_cores / 4).max(1);

        let foreground = ThreadPool::new(&format!("{}-fg", name), fg_threads);
        let background = ThreadPool::new(&format!("{}-bg", name), bg_threads);
        let main_thread_runner = Arc::new(MainThreadRunner::new());

        debug!(
            "TaskScheduler '{}': {} foreground + {} background workers, {} cores available",
            name, fg_threads, bg_threads, num_cores
        );

        Self {
            name: name.to_string(),
            foreground,
            background,
            main_thread_runner,
        }
    }

    /// Posts a task to the appropriate thread pool based on its priority.
    ///
    /// - [`TaskPriority::BestEffort`] -> background pool
    /// - [`TaskPriority::UserVisible`] / [`TaskPriority::UserBlocking`] -> foreground pool
    pub fn post_task<F>(&self, f: F, traits: TaskTraits)
    where
        F: FnOnce() + Send + 'static,
    {
        match traits.priority {
            TaskPriority::BestEffort => {
                self.background.post_task(f, traits);
            }
            TaskPriority::UserVisible | TaskPriority::UserBlocking => {
                self.foreground.post_task(f, traits);
            }
        }
    }

    /// Posts a task to the main thread runner.
    ///
    /// The task will be queued and executed when
    /// [`run_main_tasks`](TaskScheduler::run_main_tasks) is called from the
    /// main thread's event loop.
    pub fn post_to_main<F>(&self, f: F, traits: TaskTraits)
    where
        F: FnOnce() + Send + 'static,
    {
        let task = Task::new(f, traits);
        self.main_thread_runner.post_task(task);
    }

    /// Returns a reference to the [`MainThreadRunner`] so external code can
    /// poll or drain tasks.
    pub fn main_thread(&self) -> Arc<MainThreadRunner> {
        self.main_thread_runner.clone()
    }

    /// Runs all pending main-thread tasks.
    ///
    /// Returns the number of tasks that were executed.
    ///
    /// **Must be called from the main thread** (typically inside the GUI
    /// event loop).
    pub fn run_main_tasks(&self) -> usize {
        self.main_thread_runner.run_pending_tasks()
    }

    /// Returns a reference to the foreground [`ThreadPool`].
    pub fn foreground_pool(&self) -> &ThreadPool {
        &self.foreground
    }

    /// Returns a reference to the background [`ThreadPool`].
    pub fn background_pool(&self) -> &ThreadPool {
        &self.background
    }
}

impl Drop for TaskScheduler {
    fn drop(&mut self) {
        info!("Shutting down TaskScheduler '{}'", self.name);
    }
}

// ---------------------------------------------------------------------------
// Global scheduler
// ---------------------------------------------------------------------------

lazy_static::lazy_static! {
    /// The default global [`TaskScheduler`] instance.
    ///
    /// This is the easiest way to use the task system throughout the browser:
    ///
    /// ```ignore
    /// use rust_browser::task_queue::scheduler::GLOBAL_SCHEDULER;
    /// use rust_browser::task_queue::task::TaskTraits;
    ///
    /// GLOBAL_SCHEDULER.post_task(
    ///     || { /* background work */ },
    ///     TaskTraits::default().named("cleanup"),
    /// );
    /// ```
    pub static ref GLOBAL_SCHEDULER: TaskScheduler = {
        TaskScheduler::new("Global")
    };
}
