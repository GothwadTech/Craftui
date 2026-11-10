use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

type Task = Box<dyn FnOnce() + Send + 'static>;

/// Cancellation handle returned when scheduling a timeout or repeating interval.
#[derive(Clone, Debug)]
pub struct TimerHandle {
    cancelled: Arc<AtomicBool>,
}

impl TimerHandle {
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }
}

struct ScheduledTimer {
    due_ms: u64,
    interval_ms: Option<u64>,
    cancelled: Arc<AtomicBool>,
    callback: Box<dyn FnMut() + Send + 'static>,
}

struct DispatcherInner {
    queue: Mutex<VecDeque<Task>>,
    timers: Mutex<Vec<ScheduledTimer>>,
    wake: Mutex<Option<Arc<dyn Fn() + Send + Sync>>>,
}

static CUSTOM_MONOTONIC_MS: std::sync::OnceLock<fn() -> u64> = std::sync::OnceLock::new();

/// Register a platform-specific monotonic clock source (e.g. `performance.now()` on `wasm32`).
pub fn set_monotonic_ms_provider(provider: fn() -> u64) {
    let _ = CUSTOM_MONOTONIC_MS.set(provider);
}

/// Platform-safe monotonic millisecond clock helper (falls back to 0 on targets without monotonic `Instant` unless registered via [`set_monotonic_ms_provider`]).
pub fn monotonic_ms() -> u64 {
    if let Some(provider) = CUSTOM_MONOTONIC_MS.get() {
        return provider();
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        static EPOCH: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
        let start = EPOCH.get_or_init(std::time::Instant::now);
        start.elapsed().as_millis() as u64
    }
    #[cfg(target_arch = "wasm32")]
    {
        0
    }
}

/// Thread-safe handle allowing worker threads (`rayon`, `tokio`, `std::thread`)
/// and scheduled timers to post closures onto the UI thread event loop.
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
                timers: Mutex::new(Vec::new()),
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

    fn wake(&self) {
        if let Ok(guard) = self.inner.wake.lock()
            && let Some(wake) = guard.as_ref()
        {
            wake();
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
        self.wake();
    }

    /// Schedule a one-shot callback relative to `now_ms`.
    pub fn schedule_after_at<F>(&self, now_ms: u64, delay_ms: u64, task: F) -> TimerHandle
    where
        F: FnOnce() + Send + 'static,
    {
        let cancelled = Arc::new(AtomicBool::new(false));
        let mut opt_task = Some(task);
        let timer = ScheduledTimer {
            due_ms: now_ms.saturating_add(delay_ms),
            interval_ms: None,
            cancelled: cancelled.clone(),
            callback: Box::new(move || {
                if let Some(f) = opt_task.take() {
                    f();
                }
            }),
        };
        if let Ok(mut timers) = self.inner.timers.lock() {
            timers.push(timer);
        }
        self.wake();
        TimerHandle { cancelled }
    }

    /// Schedule a one-shot callback using [`monotonic_ms`] as reference timestamp.
    pub fn schedule_after<F>(&self, delay_ms: u64, task: F) -> TimerHandle
    where
        F: FnOnce() + Send + 'static,
    {
        self.schedule_after_at(monotonic_ms(), delay_ms, task)
    }

    /// Schedule a repeating interval callback relative to `now_ms`.
    pub fn schedule_interval_at<F>(&self, now_ms: u64, interval_ms: u64, task: F) -> TimerHandle
    where
        F: FnMut() + Send + 'static,
    {
        let interval = interval_ms.max(1);
        let cancelled = Arc::new(AtomicBool::new(false));
        let timer = ScheduledTimer {
            due_ms: now_ms.saturating_add(interval),
            interval_ms: Some(interval),
            cancelled: cancelled.clone(),
            callback: Box::new(task),
        };
        if let Ok(mut timers) = self.inner.timers.lock() {
            timers.push(timer);
        }
        self.wake();
        TimerHandle { cancelled }
    }

    /// Schedule a repeating interval callback using [`monotonic_ms`].
    pub fn schedule_interval<F>(&self, interval_ms: u64, task: F) -> TimerHandle
    where
        F: FnMut() + Send + 'static,
    {
        self.schedule_interval_at(monotonic_ms(), interval_ms, task)
    }

    /// Fire all timers whose deadline `<= now_ms`. Returns the number of timer firings.
    pub fn drain_due_timers(&self, now_ms: u64) -> usize {
        let mut fired_timers = Vec::new();
        {
            let Ok(mut timers) = self.inner.timers.lock() else {
                return 0;
            };
            let mut remaining = Vec::with_capacity(timers.len());
            for timer in timers.drain(..) {
                if timer.cancelled.load(Ordering::SeqCst) {
                    continue;
                }
                if timer.due_ms <= now_ms {
                    fired_timers.push(timer);
                } else {
                    remaining.push(timer);
                }
            }
            *timers = remaining;
        }

        let mut count = 0;
        let mut requeue = Vec::new();
        for mut timer in fired_timers {
            if timer.cancelled.load(Ordering::SeqCst) {
                continue;
            }
            (timer.callback)();
            count += 1;
            if let Some(interval) = timer.interval_ms
                && !timer.cancelled.load(Ordering::SeqCst)
            {
                let mut next_due = timer.due_ms.saturating_add(interval);
                if next_due <= now_ms {
                    next_due = now_ms.saturating_add(interval);
                }
                timer.due_ms = next_due;
                requeue.push(timer);
            }
        }

        if !requeue.is_empty()
            && let Ok(mut timers) = self.inner.timers.lock()
        {
            timers.extend(requeue);
        }

        count
    }

    /// Minimum remaining delay until the next pending timer fires, if any.
    pub fn next_due_delay_ms(&self, now_ms: u64) -> Option<u64> {
        let Ok(timers) = self.inner.timers.lock() else {
            return None;
        };
        timers
            .iter()
            .filter(|t| !t.cancelled.load(Ordering::SeqCst))
            .map(|t| t.due_ms.saturating_sub(now_ms))
            .min()
    }

    /// Drain all queued UI tasks and timers due as of [`monotonic_ms`].
    pub fn drain(&self) -> usize {
        self.drain_at(monotonic_ms())
    }

    /// Drain all queued UI tasks and timers due as of `now_ms`.
    pub fn drain_at(&self, now_ms: u64) -> usize {
        let timer_count = self.drain_due_timers(now_ms);
        let tasks: Vec<Task> = match self.inner.queue.lock() {
            Ok(mut q) => q.drain(..).collect(),
            Err(_) => return timer_count,
        };
        let count = tasks.len() + timer_count;
        for task in tasks {
            task();
        }
        count
    }

    pub fn pending_count(&self) -> usize {
        self.inner.queue.lock().map(|q| q.len()).unwrap_or(0)
    }

    pub fn pending_timer_count(&self) -> usize {
        self.inner
            .timers
            .lock()
            .map(|t| {
                t.iter()
                    .filter(|x| !x.cancelled.load(Ordering::SeqCst))
                    .count()
            })
            .unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

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

    #[test]
    fn dispatcher_timers_fire_and_cancel() {
        let dispatcher = UiDispatcher::new();
        let once = Arc::new(AtomicUsize::new(0));
        let repeat = Arc::new(AtomicUsize::new(0));
        let o = once.clone();
        let r = repeat.clone();

        dispatcher.schedule_after_at(100, 50, move || {
            o.fetch_add(1, Ordering::SeqCst);
        });
        let handle = dispatcher.schedule_interval_at(100, 30, move || {
            r.fetch_add(1, Ordering::SeqCst);
        });

        assert_eq!(dispatcher.next_due_delay_ms(100), Some(30));
        assert_eq!(dispatcher.drain_at(125), 0);
        assert_eq!(dispatcher.drain_at(130), 1);
        assert_eq!(repeat.load(Ordering::SeqCst), 1);
        assert_eq!(once.load(Ordering::SeqCst), 0);

        assert_eq!(dispatcher.drain_at(160), 2);
        assert_eq!(once.load(Ordering::SeqCst), 1);
        assert_eq!(repeat.load(Ordering::SeqCst), 2);

        handle.cancel();
        assert_eq!(dispatcher.drain_at(500), 0);
        assert_eq!(repeat.load(Ordering::SeqCst), 2);
    }
}
