//! Task Queue System — Chrome-like task scheduling infrastructure.
//!
//! This module provides a multi-threaded, priority-based task execution system
//! inspired by Chromium's `base/task` machinery. It includes:
//!
//! - **Task types** with priorities (`BestEffort`, `UserVisible`, `UserBlocking`)
//! - A **work-stealing thread pool** for efficient multi-core utilization
//! - A **global task scheduler** that dispatches work to the appropriate pool
//!   based on priority
//! - A **main thread runner** for tasks that must execute on the primary thread
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────┐
//! │                  TaskScheduler                       │
//! │  ┌────────────────────┐  ┌──────────────────────┐   │
//! │  │  Foreground Pool   │  │  Background Pool     │   │
//! │  │  (UserBlocking +   │  │  (BestEffort tasks)  │   │
//! │  │   UserVisible)     │  │                      │   │
//! │  └────────┬───────────┘  └──────────┬───────────┘   │
//! │           │                          │               │
//! │  ┌────────▼──────────────────────────▼───────────┐  │
//! │  │         Work-Stealing Thread Pools            │  │
//! │  │  (crossbeam-deque injector + per-thread queues)│  │
//! │  └───────────────────────────────────────────────┘  │
//! │                                                     │
//! │  ┌──────────────────────────────────────────────┐   │
//! │  │           MainThreadRunner                   │   │
//! │  │  (single-threaded, for UI / main-thread ops) │   │
//! │  └──────────────────────────────────────────────┘   │
//! └─────────────────────────────────────────────────────┘
//! ```

pub mod task;
pub mod thread_pool;
pub mod scheduler;

pub use task::{Task, TaskPriority, TaskTraits, RepeatingTask, TaskHandle};
pub use thread_pool::{ThreadPool, default_thread_pool};
pub use scheduler::{TaskScheduler, MainThreadRunner, GLOBAL_SCHEDULER};
