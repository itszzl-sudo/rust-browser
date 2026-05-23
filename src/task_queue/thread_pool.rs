//! Work-stealing thread pool.
//!
//! This module provides a multi-threaded, work-stealing executor inspired by
//! Chromium's `ThreadPool`.  Each worker thread has a local double-ended queue
//! (deque); tasks are first pushed into a global [`Injector`] and then stolen
//! by idle workers to minimise contention and balance load automatically.
//!
//! # Stealing order
//!
//! 1. Pop from the worker's **local** queue (LIFO — cache-friendly).
//! 2. Steal a batch from the **global injector** (FIFO — fair).
//! 3. Steal a batch from a **random other worker** (LIFO — load balancing).
//!
//! When no work is available the thread yields for a brief interval.

use crate::task_queue::task::{Task, TaskTraits};
use crossbeam_deque::{Injector, Steal, Worker};
use log::{debug, trace};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

/// A single worker thread together with its local deque.
#[allow(dead_code)]
struct WorkerThread {
    /// Index in the pool (0 .. num_workers-1).
    id: usize,

    /// Local work-stealing deque.
    worker: Worker<Task>,

    /// Join handle (populated after the thread is spawned).
    handle: Option<thread::JoinHandle<()>>,
}

// ---------------------------------------------------------------------------
// ThreadPool
// ---------------------------------------------------------------------------

/// A multi-threaded, work-stealing thread pool.
///
/// Tasks are submitted via [`post_task`](ThreadPool::post_task) or the
/// convenience method [`post`](ThreadPool::post).  Internally they are pushed
/// onto a global [`Injector`] queue from which idle worker threads steal.
///
/// # Shutdown
///
/// The pool shuts down automatically when dropped.  In-flight tasks are *not*
/// awaited — only the shutdown flag is set, and threads exit when they notice
/// it after completing their current work.
pub struct ThreadPool {
    /// Human-readable name (used for thread names and logging).
    name: String,

    /// Global injector queue — the primary entry point for external tasks.
    injector: Arc<Injector<Task>>,

    /// Per-worker deques and handles, each behind a [`Mutex`] so that peers
    /// can steal from one another.
    workers: Arc<Vec<Mutex<WorkerThread>>>,

    /// When `true`, all worker threads should exit as soon as possible.
    shutdown: Arc<AtomicBool>,

    /// Number of threads that are still alive (used during shutdown).
    _active_threads: Arc<AtomicUsize>,
}

impl ThreadPool {
    /// Creates a new [`ThreadPool`] with `num_threads` workers (minimum 1).
    ///
    /// Each worker thread is named `{name}-{index}` for easy identification
    /// in logs and debuggers.
    pub fn new(name: &str, num_threads: usize) -> Self {
        let num_threads = num_threads.max(1);
        let injector = Arc::new(Injector::new());
        let shutdown = Arc::new(AtomicBool::new(false));
        let active = Arc::new(AtomicUsize::new(num_threads));

        let mut workers: Vec<Mutex<WorkerThread>> = Vec::with_capacity(num_threads);

        for i in 0..num_threads {
            let worker = Worker::new_fifo();
            let worker_thread = WorkerThread {
                id: i,
                worker,
                handle: None,
            };
            workers.push(Mutex::new(worker_thread));
        }

        let workers = Arc::new(workers);
        let pool_name = name.to_string();

        // Spawn each worker thread.
        for i in 0..num_threads {
            let injector_clone = injector.clone();
            let workers_clone = workers.clone();
            let shutdown_clone = shutdown.clone();
            let active_clone = active.clone();
            let pool_name_for_thread = pool_name.clone();
            let thread_name = format!("{}-{}", pool_name, i);

            let handle = thread::Builder::new()
                .name(thread_name)
                .spawn(move || {
                    debug!(
                        "Thread pool '{}' worker {} started",
                        pool_name_for_thread, i
                    );

                    loop {
                        // Honour shutdown request.
                        if shutdown_clone.load(Ordering::SeqCst) {
                            active_clone.fetch_sub(1, Ordering::SeqCst);
                            debug!(
                                "Thread pool '{}' worker {} shutting down",
                                pool_name_for_thread, i
                            );
                            return;
                        }

                        let had_work = Self::try_steal_one(i, &workers_clone, &injector_clone);

                        if !had_work {
                            // No work available — yield to avoid busy-spinning.
                            thread::sleep(Duration::from_micros(100));
                        }
                    }
                })
                .expect("Failed to spawn worker thread");

            workers[i].lock().unwrap().handle = Some(handle);
        }

        debug!("ThreadPool '{}' created with {} workers", name, num_threads);

        Self {
            name: name.to_string(),
            injector,
            workers,
            shutdown,
            _active_threads: active,
        }
    }

    /// Attempt to pop or steal a single task for worker `i`.
    ///
    /// Returns `true` if a task was found and executed.
    fn try_steal_one(
        i: usize,
        workers: &Arc<Vec<Mutex<WorkerThread>>>,
        injector: &Injector<Task>,
    ) -> bool {
        // 1. Local queue (LIFO — most cache-friendly).
        if let Ok(w) = workers[i].lock() {
            if let Some(task) = w.worker.pop() {
                drop(w);
                task.run();
                return true;
            }
        }

        // 2. Global injector (FIFO — fair scheduling across sources).
        {
            let local = workers[i].lock().unwrap();
            match injector.steal_batch_and_pop(&local.worker) {
                Steal::Success(task) => {
                    drop(local);
                    task.run();
                    return true;
                }
                Steal::Empty | Steal::Retry => {}
            }
        }

        // 3. Steal from a random other worker (load balancing).
        let num_workers = workers.len();
        for offset in 1..num_workers {
            let j = (i + offset) % num_workers;
            if j == i {
                continue;
            }
            if let Ok(victim) = workers[j].lock() {
                let stealer = victim.worker.stealer();
                // We need a *second* lock on our own worker to receive the
                // stolen batch — release the victim lock first to avoid a
                // deadlock (the lock order is always increasing index).
                drop(victim);

                let local = workers[i].lock().unwrap();
                match stealer.steal_batch_and_pop(&local.worker) {
                    Steal::Success(task) => {
                        drop(local);
                        task.run();
                        return true;
                    }
                    Steal::Empty | Steal::Retry => {}
                }
            }
        }

        false
    }

    /// Posts a task to the global injector queue.
    ///
    /// The task will be picked up by the next available worker thread.
    pub fn post_task<F>(&self, f: F, traits: TaskTraits)
    where
        F: FnOnce() + Send + 'static,
    {
        let task = Task::new(f, traits);
        trace!("ThreadPool '{}' posting Task[{}]", self.name, task.id);
        self.injector.push(task);
    }

    /// Posts a task with default traits.
    pub fn post<F>(&self, f: F)
    where
        F: FnOnce() + Send + 'static,
    {
        self.post_task(f, TaskTraits::default());
    }

    /// Returns the number of worker threads in this pool.
    pub fn num_workers(&self) -> usize {
        self.workers.len()
    }

    /// Initiates an orderly shutdown.
    ///
    /// All worker threads will exit after noticing the shutdown flag.
    /// In-flight tasks already running will complete; queued tasks remain
    /// unprocessed.
    pub fn shutdown(&self) {
        debug!("Shutting down ThreadPool '{}'", self.name);
        self.shutdown.store(true, Ordering::SeqCst);
    }
}

impl Drop for ThreadPool {
    fn drop(&mut self) {
        self.shutdown();
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Creates a [`ThreadPool`] sized to the number of available CPU cores.
pub fn default_thread_pool(name: &str) -> ThreadPool {
    let num = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4);
    ThreadPool::new(name, num)
}
