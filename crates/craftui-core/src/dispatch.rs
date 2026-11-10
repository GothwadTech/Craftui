use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

type Task = Box<dyn FnOnce() + Send + 'static>;

struct DispatcherInner {
    queue: Mutex<VecDeque<Task>>,
    wake: Mutex<Option<Arc<dyn Fn() + Send + Sync>>>,
}

/// Thread-safe handle allowing worker threads (`rayon`, `tokio`, `std::thread`)
/// to schedule closures onto the UI thread event loop.
#[derive(Clone)]
pub struct UiDispatcher {
    inner: Arc<DispatcherInner>,
}

impl Default for UiDispatcher {
    fn default() -> Self {
        Self::new()
    }
}

impl UiDispatcher {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(DispatcherInner {
                queue: Mutex::new(VecDeque::new()),
                wake: Mutex::new(None),
            }),
        }
    }

    pub fn with_waker<F>(wake: F) -> Self
    where
        F: Fn() + Send + Sync + 'static,
    {
        let dispatcher = Self::new();
        dispatcher.set_waker(wake);
        dispatcher
    }

    pub fn set_waker<F>(&self, wake: F)
    where
        F: Fn() + Send + Sync + 'static,
    {
        if let Ok(mut guard) = self.inner.wake.lock() {
            *guard = Some(Arc::new(wake));
        }
    }

    /// Post a task onto the UI thread and wake the platform event loop.
    pub fn post<F>(&self, task: F)
    where
        F: FnOnce() + Send + 'static,
    {
        if let Ok(mut q) = self.inner.queue.lock() {
            q.push_back(Box::new(task));
        }
        if let Ok(guard) = self.inner.wake.lock()
            && let Some(wake) = guard.as_ref()
        {
            wake();
        }
    }

    /// Drain and run all pending UI tasks. Should only be called from the UI thread.
    pub fn drain(&self) -> usize {
        let tasks: Vec<Task> = match self.inner.queue.lock() {
            Ok(mut q) => q.drain(..).collect(),
            Err(_) => return 0,
        };
        let count = tasks.len();
        for task in tasks {
            task();
        }
        count
    }

    pub fn pending_count(&self) -> usize {
        self.inner.queue.lock().map(|q| q.len()).unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    #[test]
    fn dispatcher_wakes_and_drains() {
        let woken = Arc::new(AtomicBool::new(false));
        let woken_clone = woken.clone();
        let dispatcher = UiDispatcher::with_waker(move || {
            woken_clone.store(true, Ordering::SeqCst);
        });

        let counter = Arc::new(AtomicUsize::new(0));
        let c = counter.clone();
        std::thread::spawn(move || {
            dispatcher.post(move || {
                c.fetch_add(42, Ordering::SeqCst);
            });
        })
        .join()
        .unwrap();
    }

    #[test]
    fn dispatcher_post_and_drain() {
        let woken = Arc::new(AtomicUsize::new(0));
        let w = woken.clone();
        let dispatcher = UiDispatcher::with_waker(move || {
            w.fetch_add(1, Ordering::SeqCst);
        });
        let value = Arc::new(AtomicUsize::new(0));
        let v = value.clone();
        dispatcher.post(move || {
            v.store(99, Ordering::SeqCst);
        });
        assert_eq!(woken.load(Ordering::SeqCst), 1);
        assert_eq!(dispatcher.drain(), 1);
        assert_eq!(value.load(Ordering::SeqCst), 99);
    }
}
