use craftui_reactive::{Scope, Signal};

/// Semantic visual severity for toast notifications.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ToastSeverity {
    #[default]
    Info,
    Success,
    Warning,
    Error,
}

/// Single notification entry managed by [`ToastManager`].
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ToastItem {
    pub id: u64,
    pub title: String,
    pub message: String,
    pub severity: ToastSeverity,
    pub duration_ms: u64,
}

impl ToastItem {
    pub fn new(
        id: u64,
        title: impl Into<String>,
        message: impl Into<String>,
        severity: ToastSeverity,
        duration_ms: u64,
    ) -> Self {
        Self {
            id,
            title: title.into(),
            message: message.into(),
            severity,
            duration_ms,
        }
    }
}

/// Copyable reactive notification queue backed by CraftUI's deterministic scope timers.
#[derive(Clone, Copy)]
pub struct ToastManager {
    scope: Scope,
    items: Signal<Vec<ToastItem>>,
    next_id: Signal<u64>,
    max_visible: usize,
}

impl ToastManager {
    pub fn new(scope: Scope) -> Self {
        Self {
            scope,
            items: scope.signal(Vec::new()),
            next_id: scope.signal(1),
            max_visible: 5,
        }
    }

    pub fn with_max_visible(mut self, max: usize) -> Self {
        self.max_visible = max.max(1);
        self
    }

    pub fn items_signal(self) -> Signal<Vec<ToastItem>> {
        self.items
    }

    pub fn items(self) -> Vec<ToastItem> {
        self.items.get()
    }

    pub fn len(self) -> usize {
        self.items.with(Vec::len)
    }

    pub fn is_empty(self) -> bool {
        self.items.with(Vec::is_empty)
    }

    /// Push a notification with optional auto-dismiss (`duration_ms == 0` stays pinned until dismissed).
    pub fn push(
        self,
        title: impl Into<String>,
        message: impl Into<String>,
        severity: ToastSeverity,
        duration_ms: u64,
    ) -> u64 {
        let mut id = 0;
        self.next_id.update(|cur| {
            id = *cur;
            *cur = cur.wrapping_add(1).max(1);
        });
        let item = ToastItem::new(id, title, message, severity, duration_ms);
        let max_v = self.max_visible;
        self.items.update(|list| {
            list.push(item);
            if list.len() > max_v {
                let extra = list.len() - max_v;
                list.drain(0..extra);
            }
        });
        if duration_ms > 0 {
            self.scope.timeout(duration_ms, move || {
                self.dismiss(id);
            });
        }
        id
    }

    /// Push at explicit `now_ms` timestamp (useful in deterministic tests without wall-clock jitter).
    pub fn push_at(
        self,
        now_ms: u64,
        title: impl Into<String>,
        message: impl Into<String>,
        severity: ToastSeverity,
        duration_ms: u64,
    ) -> u64 {
        let mut id = 0;
        self.next_id.update(|cur| {
            id = *cur;
            *cur = cur.wrapping_add(1).max(1);
        });
        let item = ToastItem::new(id, title, message, severity, duration_ms);
        let max_v = self.max_visible;
        self.items.update(|list| {
            list.push(item);
            if list.len() > max_v {
                let extra = list.len() - max_v;
                list.drain(0..extra);
            }
        });
        if duration_ms > 0 {
            self.scope.timeout_at(now_ms, duration_ms, move || {
                self.dismiss(id);
            });
        }
        id
    }

    pub fn info(self, message: impl Into<String>) -> u64 {
        self.push("Info", message, ToastSeverity::Info, 3600)
    }

    pub fn success(self, message: impl Into<String>) -> u64 {
        self.push("Success", message, ToastSeverity::Success, 3600)
    }

    pub fn warning(self, message: impl Into<String>) -> u64 {
        self.push("Warning", message, ToastSeverity::Warning, 4500)
    }

    pub fn error(self, message: impl Into<String>) -> u64 {
        self.push("Error", message, ToastSeverity::Error, 6000)
    }

    pub fn dismiss(self, id: u64) -> bool {
        let mut removed = false;
        self.items.update(|list| {
            let before = list.len();
            list.retain(|item| item.id != id);
            removed = list.len() != before;
        });
        removed
    }

    pub fn clear(self) {
        self.items.update(Vec::clear);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use craftui_reactive::{reset_runtime, tick_timers_at};

    #[test]
    fn toast_manager_queues_dismisses_and_auto_expires() {
        reset_runtime();
        let cx = Scope::root();
        let mgr = ToastManager::new(cx).with_max_visible(3);

        let t1 = mgr.push_at(100, "Build", "Started", ToastSeverity::Info, 1000);
        let t2 = mgr.push_at(120, "Render", "Ready", ToastSeverity::Success, 2000);
        let t3 = mgr.push_at(140, "Lint", "Clean", ToastSeverity::Warning, 0);
        assert_eq!(mgr.len(), 3);

        // Overflow evicts oldest (`t1`)
        let t4 = mgr.push_at(160, "Deploy", "Failed", ToastSeverity::Error, 3000);
        assert_eq!(mgr.len(), 3);
        let ids: Vec<u64> = mgr.items().into_iter().map(|i| i.id).collect();
        assert_eq!(ids, vec![t2, t3, t4]);

        // Deterministic expiry removes `t2` at 2120ms, leaves persistent `t3`
        tick_timers_at(2120);
        let ids_after: Vec<u64> = mgr.items().into_iter().map(|i| i.id).collect();
        assert_eq!(ids_after, vec![t3, t4]);

        // Explicit dismiss + clear
        assert!(mgr.dismiss(t3));
        assert!(!mgr.dismiss(t1));
        assert_eq!(mgr.len(), 1);
        mgr.clear();
        assert!(mgr.is_empty());
    }
}
