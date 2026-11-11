use craftui_reactive::{Scope, Signal};

/// Visual transition played when navigating between routes with [`Router`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RouterTransition {
    None,
    #[default]
    Fade,
    SlideLeft,
    SlideRight,
    ScaleFade,
}

/// Declarative stack-based navigation controller (PRD FR-6.2).
pub struct Router<R: Clone + PartialEq + 'static> {
    stack: Signal<Vec<R>>,
}

impl<R: Clone + PartialEq + 'static> Copy for Router<R> {}

impl<R: Clone + PartialEq + 'static> Clone for Router<R> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<R: Clone + PartialEq + 'static> Router<R> {
    pub fn new(cx: Scope, initial: R) -> Self {
        Self {
            stack: cx.signal(vec![initial]),
        }
    }

    pub fn current(&self) -> R {
        self.stack
            .with(|s| s.last().cloned().expect("router stack empty"))
    }

    pub fn depth(&self) -> usize {
        self.stack.with(|s| s.len())
    }

    pub fn can_pop(&self) -> bool {
        self.depth() > 1
    }

    pub fn push(&self, route: R) {
        self.stack.update(|s| s.push(route));
    }

    pub fn pop(&self) -> bool {
        let mut popped = false;
        self.stack.update(|s| {
            if s.len() > 1 {
                s.pop();
                popped = true;
            }
        });
        popped
    }

    pub fn replace(&self, route: R) {
        self.stack.update(|s| {
            if let Some(last) = s.last_mut() {
                *last = route;
            } else {
                s.push(route);
            }
        });
    }

    pub fn pop_to_root(&self) {
        self.stack.update(|s| {
            if s.len() > 1 {
                s.truncate(1);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use craftui_reactive::reset_runtime;

    #[test]
    fn stack_navigation_push_pop() {
        reset_runtime();
        let cx = Scope::root();
        let router = Router::new(cx, "Home");
        assert!(!router.can_pop());
        router.push("Editor");
        assert_eq!(router.current(), "Editor");
        assert!(router.can_pop());
        assert!(router.pop());
        assert_eq!(router.current(), "Home");
        assert!(!router.pop());
    }
}
