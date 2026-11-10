//! Fine-grained reactive state engine backed by generational `SlotMap` arenas.
//!
//! All signal, computed, effect, and scope handles are `Copy` regardless of `T`,
//! eliminating `.clone()` boilerplate in UI event closures while guaranteeing
//! deterministic scope-based disposal.

use craftui_core::{DirtyFlags, NodeId};
use slotmap::{SlotMap, new_key_type};
use std::any::{Any, TypeId};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet, VecDeque};
use std::marker::PhantomData;
use std::rc::Rc;

new_key_type! {
    struct SignalKey;
    struct ComputedKey;
    struct EffectKey;
    struct ScopeKey;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum SourceId {
    Signal(SignalKey),
    Computed(ComputedKey),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum SubscriberId {
    Computed(ComputedKey),
    Effect(EffectKey),
}

struct SignalSlot {
    value: Rc<RefCell<Box<dyn Any>>>,
    subscribers: HashSet<SubscriberId>,
    node_bindings: Vec<(NodeId, DirtyFlags)>,
}

struct ComputedSlot {
    value: Option<Rc<RefCell<Box<dyn Any>>>>,
    compute: Rc<RefCell<dyn FnMut() -> Box<dyn Any>>>,
    dirty: bool,
    sources: Vec<SourceId>,
    subscribers: HashSet<SubscriberId>,
    node_bindings: Vec<(NodeId, DirtyFlags)>,
}

struct EffectSlot {
    callback: Rc<RefCell<dyn FnMut()>>,
    sources: Vec<SourceId>,
    disposed: bool,
}

struct ScopeSlot {
    parent: Option<ScopeKey>,
    children: Vec<ScopeKey>,
    signals: Vec<SignalKey>,
    computeds: Vec<ComputedKey>,
    effects: Vec<EffectKey>,
    cleanups: Vec<Box<dyn FnOnce()>>,
    contexts: HashMap<TypeId, Rc<dyn Any>>,
    disposed: bool,
}

struct ReactiveArena {
    scopes: SlotMap<ScopeKey, ScopeSlot>,
    signals: SlotMap<SignalKey, SignalSlot>,
    computeds: SlotMap<ComputedKey, ComputedSlot>,
    effects: SlotMap<EffectKey, EffectSlot>,
    observer_stack: Vec<SubscriberId>,
    pending_effects: VecDeque<EffectKey>,
    pending_effect_set: HashSet<EffectKey>,
    dirty_nodes: HashMap<NodeId, DirtyFlags>,
    batch_depth: usize,
    flushing: bool,
    root_scope: Option<ScopeKey>,
}

impl ReactiveArena {
    fn new() -> Self {
        let mut scopes = SlotMap::with_key();
        let root_scope = scopes.insert(ScopeSlot {
            parent: None,
            children: Vec::new(),
            signals: Vec::new(),
            computeds: Vec::new(),
            effects: Vec::new(),
            cleanups: Vec::new(),
            contexts: HashMap::new(),
            disposed: false,
        });
        Self {
            scopes,
            signals: SlotMap::with_key(),
            computeds: SlotMap::with_key(),
            effects: SlotMap::with_key(),
            observer_stack: Vec::new(),
            pending_effects: VecDeque::new(),
            pending_effect_set: HashSet::new(),
            dirty_nodes: HashMap::new(),
            batch_depth: 0,
            flushing: false,
            root_scope: Some(root_scope),
        }
    }
}

thread_local! {
    static ARENA: RefCell<ReactiveArena> = RefCell::new(ReactiveArena::new());
}

/// Reset the thread-local reactive arena (intended for isolated unit tests).
pub fn reset_runtime() {
    ARENA.with(|a| {
        *a.borrow_mut() = ReactiveArena::new();
    });
}

/// Drain all widget [`NodeId`]s marked dirty since the last frame.
pub fn drain_dirty_nodes() -> Vec<(NodeId, DirtyFlags)> {
    ARENA.with(|a| {
        let mut arena = a.borrow_mut();
        arena.dirty_nodes.drain().collect()
    })
}

/// Mark a retained node dirty outside of a signal update (e.g. direct widget mutation).
pub fn mark_node_dirty(node: NodeId, flags: DirtyFlags) {
    ARENA.with(|a| {
        let mut arena = a.borrow_mut();
        arena
            .dirty_nodes
            .entry(node)
            .and_modify(|f| *f |= flags)
            .or_insert(flags);
    });
}

/// Batch multiple signal updates, running effects once after the closure completes.
pub fn batch<R>(f: impl FnOnce() -> R) -> R {
    ARENA.with(|a| a.borrow_mut().batch_depth += 1);
    let result = f();
    let should_flush = ARENA.with(|a| {
        let mut arena = a.borrow_mut();
        arena.batch_depth = arena.batch_depth.saturating_sub(1);
        arena.batch_depth == 0
    });
    if should_flush {
        flush_effects();
    }
    result
}

/// Execute `f` without tracking any reactive signal reads as dependencies.
pub fn untrack<R>(f: impl FnOnce() -> R) -> R {
    let saved = ARENA.with(|a| {
        let mut arena = a.borrow_mut();
        std::mem::take(&mut arena.observer_stack)
    });
    let result = f();
    ARENA.with(|a| {
        a.borrow_mut().observer_stack = saved;
    });
    result
}

/// Ownership scope for signals, computed values, effects, and child components.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Scope {
    key: ScopeKey,
}

impl Scope {
    /// Returns the global root scope for the current UI thread.
    pub fn root() -> Self {
        let key = ARENA.with(|a| {
            let mut arena = a.borrow_mut();
            if let Some(root) = arena.root_scope
                && arena.scopes.contains_key(root)
            {
                return root;
            }
            let root = arena.scopes.insert(ScopeSlot {
                parent: None,
                children: Vec::new(),
                signals: Vec::new(),
                computeds: Vec::new(),
                effects: Vec::new(),
                cleanups: Vec::new(),
                contexts: HashMap::new(),
                disposed: false,
            });
            arena.root_scope = Some(root);
            root
        });
        Self { key }
    }

    /// Create an isolated child scope owned by this scope.
    pub fn child_scope(self) -> Self {
        let key = ARENA.with(|a| {
            let mut arena = a.borrow_mut();
            let child = arena.scopes.insert(ScopeSlot {
                parent: Some(self.key),
                children: Vec::new(),
                signals: Vec::new(),
                computeds: Vec::new(),
                effects: Vec::new(),
                cleanups: Vec::new(),
                contexts: HashMap::new(),
                disposed: false,
            });
            if let Some(parent) = arena.scopes.get_mut(self.key) {
                parent.children.push(child);
            }
            child
        });
        Self { key }
    }

    /// Allocate a reactive [`Signal<T>`] owned by this scope.
    pub fn signal<T: 'static>(self, initial: T) -> Signal<T> {
        let key = ARENA.with(|a| {
            let mut arena = a.borrow_mut();
            let sig_key = arena.signals.insert(SignalSlot {
                value: Rc::new(RefCell::new(Box::new(initial))),
                subscribers: HashSet::new(),
                node_bindings: Vec::new(),
            });
            if let Some(scope) = arena.scopes.get_mut(self.key) {
                scope.signals.push(sig_key);
            }
            sig_key
        });
        Signal {
            key,
            _marker: PhantomData,
        }
    }

    /// Allocate a lazy memoized [`Computed<T>`] owned by this scope.
    pub fn computed<T: 'static>(self, mut compute: impl FnMut() -> T + 'static) -> Computed<T> {
        let key = ARENA.with(|a| {
            let mut arena = a.borrow_mut();
            let comp_key = arena.computeds.insert(ComputedSlot {
                value: None,
                compute: Rc::new(RefCell::new(move || Box::new(compute()) as Box<dyn Any>)),
                dirty: true,
                sources: Vec::new(),
                subscribers: HashSet::new(),
                node_bindings: Vec::new(),
            });
            if let Some(scope) = arena.scopes.get_mut(self.key) {
                scope.computeds.push(comp_key);
            }
            comp_key
        });
        Computed {
            key,
            _marker: PhantomData,
        }
    }

    /// Register an auto-tracking reactive [`Effect`] owned by this scope.
    pub fn effect(self, callback: impl FnMut() + 'static) -> Effect {
        let key = ARENA.with(|a| {
            let mut arena = a.borrow_mut();
            let eff_key = arena.effects.insert(EffectSlot {
                callback: Rc::new(RefCell::new(callback)),
                sources: Vec::new(),
                disposed: false,
            });
            if let Some(scope) = arena.scopes.get_mut(self.key) {
                scope.effects.push(eff_key);
            }
            eff_key
        });
        run_effect(key);
        Effect { key }
    }

    /// Register a cleanup closure executed when this scope is disposed.
    pub fn on_cleanup(self, cleanup: impl FnOnce() + 'static) {
        ARENA.with(|a| {
            let mut arena = a.borrow_mut();
            if let Some(scope) = arena.scopes.get_mut(self.key) {
                scope.cleanups.push(Box::new(cleanup));
            }
        });
    }

    /// Provide a typed context value accessible to descendant scopes.
    pub fn provide_context<T: 'static>(self, value: T) {
        ARENA.with(|a| {
            let mut arena = a.borrow_mut();
            if let Some(scope) = arena.scopes.get_mut(self.key) {
                scope.contexts.insert(TypeId::of::<T>(), Rc::new(value));
            }
        });
    }

    /// Look up the nearest ancestor context value of type `T`.
    pub fn use_context<T: Clone + 'static>(self) -> Option<T> {
        ARENA.with(|a| {
            let arena = a.borrow();
            let mut cursor = Some(self.key);
            while let Some(key) = cursor {
                let scope = arena.scopes.get(key)?;
                if let Some(val) = scope.contexts.get(&TypeId::of::<T>()) {
                    return val.downcast_ref::<T>().cloned();
                }
                cursor = scope.parent;
            }
            None
        })
    }

    pub fn is_alive(self) -> bool {
        ARENA.with(|a| a.borrow().scopes.get(self.key).is_some_and(|s| !s.disposed))
    }

    /// Dispose this scope and recursively free all child scopes, signals, computeds, and effects.
    pub fn dispose(self) {
        dispose_scope_recursive(self.key);
    }
}

fn dispose_scope_recursive(scope_key: ScopeKey) {
    let scope_data = ARENA.with(|a| {
        let mut arena = a.borrow_mut();
        let scope = arena.scopes.get_mut(scope_key)?;
        if scope.disposed {
            return None;
        }
        scope.disposed = true;
        let children = std::mem::take(&mut scope.children);
        let signals = std::mem::take(&mut scope.signals);
        let computeds = std::mem::take(&mut scope.computeds);
        let effects = std::mem::take(&mut scope.effects);
        let cleanups = std::mem::take(&mut scope.cleanups);
        let parent = scope.parent;

        if let Some(parent_key) = parent
            && let Some(parent_scope) = arena.scopes.get_mut(parent_key)
        {
            parent_scope.children.retain(|c| *c != scope_key);
        }

        Some((children, signals, computeds, effects, cleanups))
    });

    let Some((children, signals, computeds, effects, cleanups)) = scope_data else {
        return;
    };

    for child in children {
        dispose_scope_recursive(child);
    }

    for cleanup in cleanups {
        cleanup();
    }

    ARENA.with(|a| {
        let mut arena = a.borrow_mut();
        for eff_key in effects {
            let sources = arena
                .effects
                .get_mut(eff_key)
                .map(|e| {
                    e.disposed = true;
                    std::mem::take(&mut e.sources)
                })
                .unwrap_or_default();
            detach_subscriber(&mut arena, SubscriberId::Effect(eff_key), &sources);
            arena.effects.remove(eff_key);
        }
        for comp_key in computeds {
            let sources = arena
                .computeds
                .get_mut(comp_key)
                .map(|c| std::mem::take(&mut c.sources))
                .unwrap_or_default();
            detach_subscriber(&mut arena, SubscriberId::Computed(comp_key), &sources);
            arena.computeds.remove(comp_key);
        }
        for sig_key in signals {
            arena.signals.remove(sig_key);
        }
        arena.scopes.remove(scope_key);
    });
}

fn detach_subscriber(arena: &mut ReactiveArena, sub: SubscriberId, sources: &[SourceId]) {
    for src in sources {
        match *src {
            SourceId::Signal(sk) => {
                if let Some(sig) = arena.signals.get_mut(sk) {
                    sig.subscribers.remove(&sub);
                }
            }
            SourceId::Computed(ck) => {
                if let Some(comp) = arena.computeds.get_mut(ck) {
                    comp.subscribers.remove(&sub);
                }
            }
        }
    }
}

fn record_dependency(source: SourceId) {
    ARENA.with(|a| {
        let mut arena = a.borrow_mut();
        let Some(&observer) = arena.observer_stack.last() else {
            return;
        };
        match source {
            SourceId::Signal(sk) => {
                if let Some(sig) = arena.signals.get_mut(sk) {
                    sig.subscribers.insert(observer);
                }
            }
            SourceId::Computed(ck) => {
                if let Some(comp) = arena.computeds.get_mut(ck) {
                    comp.subscribers.insert(observer);
                }
            }
        }
        match observer {
            SubscriberId::Computed(ck) => {
                if let Some(comp) = arena.computeds.get_mut(ck)
                    && !comp.sources.contains(&source)
                {
                    comp.sources.push(source);
                }
            }
            SubscriberId::Effect(ek) => {
                if let Some(eff) = arena.effects.get_mut(ek)
                    && !eff.sources.contains(&source)
                {
                    eff.sources.push(source);
                }
            }
        }
    });
}

fn propagate_dirty(subscribers: Vec<SubscriberId>, node_bindings: Vec<(NodeId, DirtyFlags)>) {
    let should_flush = ARENA.with(|a| {
        let mut arena = a.borrow_mut();
        for (node, flags) in node_bindings {
            arena
                .dirty_nodes
                .entry(node)
                .and_modify(|f| *f |= flags)
                .or_insert(flags);
        }

        let mut stack = subscribers;
        let mut visited_computeds = HashSet::new();

        while let Some(sub) = stack.pop() {
            match sub {
                SubscriberId::Effect(ek) => {
                    if arena.effects.get(ek).is_some_and(|e| !e.disposed)
                        && arena.pending_effect_set.insert(ek)
                    {
                        arena.pending_effects.push_back(ek);
                    }
                }
                SubscriberId::Computed(ck) => {
                    if !visited_computeds.insert(ck) {
                        continue;
                    }
                    let info = arena.computeds.get_mut(ck).map(|comp| {
                        comp.dirty = true;
                        let bindings = comp.node_bindings.clone();
                        let next_subs: Vec<SubscriberId> =
                            comp.subscribers.iter().copied().collect();
                        (bindings, next_subs)
                    });
                    if let Some((bindings, next_subs)) = info {
                        for (node, flags) in bindings {
                            arena
                                .dirty_nodes
                                .entry(node)
                                .and_modify(|f| *f |= flags)
                                .or_insert(flags);
                        }
                        stack.extend(next_subs);
                    }
                }
            }
        }

        arena.batch_depth == 0 && !arena.flushing
    });

    if should_flush {
        flush_effects();
    }
}

fn flush_effects() {
    let already_flushing = ARENA.with(|a| {
        let mut arena = a.borrow_mut();
        if arena.flushing {
            return true;
        }
        arena.flushing = true;
        false
    });
    if already_flushing {
        return;
    }

    loop {
        let next = ARENA.with(|a| {
            let mut arena = a.borrow_mut();
            let ek = arena.pending_effects.pop_front()?;
            arena.pending_effect_set.remove(&ek);
            Some(ek)
        });
        let Some(ek) = next else {
            break;
        };
        run_effect(ek);
    }

    ARENA.with(|a| {
        a.borrow_mut().flushing = false;
    });
}

fn run_effect(ek: EffectKey) {
    let prepared = ARENA.with(|a| {
        let mut arena = a.borrow_mut();
        let eff = arena.effects.get_mut(ek)?;
        if eff.disposed {
            return None;
        }
        let old_sources = std::mem::take(&mut eff.sources);
        let cb = eff.callback.clone();
        detach_subscriber(&mut arena, SubscriberId::Effect(ek), &old_sources);
        arena.observer_stack.push(SubscriberId::Effect(ek));
        Some(cb)
    });

    let Some(cb) = prepared else {
        return;
    };

    if let Ok(mut func) = cb.try_borrow_mut() {
        func();
    }

    ARENA.with(|a| {
        let mut arena = a.borrow_mut();
        if arena.observer_stack.last() == Some(&SubscriberId::Effect(ek)) {
            arena.observer_stack.pop();
        }
    });
}

/// Generational `Copy` handle to a reactive state cell.
pub struct Signal<T: 'static> {
    key: SignalKey,
    _marker: PhantomData<fn() -> T>,
}

impl<T: 'static> Copy for Signal<T> {}
impl<T: 'static> Clone for Signal<T> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T: 'static> PartialEq for Signal<T> {
    fn eq(&self, other: &Self) -> bool {
        self.key == other.key
    }
}
impl<T: 'static> Eq for Signal<T> {}
impl<T: 'static> std::fmt::Debug for Signal<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("Signal").field(&self.key).finish()
    }
}

/// Read-only `Copy` view of a [`Signal<T>`].
pub struct ReadSignal<T: 'static> {
    key: SignalKey,
    _marker: PhantomData<fn() -> T>,
}

impl<T: 'static> Copy for ReadSignal<T> {}
impl<T: 'static> Clone for ReadSignal<T> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T: 'static> PartialEq for ReadSignal<T> {
    fn eq(&self, other: &Self) -> bool {
        self.key == other.key
    }
}
impl<T: 'static> Eq for ReadSignal<T> {}
impl<T: 'static> std::fmt::Debug for ReadSignal<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("ReadSignal").field(&self.key).finish()
    }
}

impl<T: 'static> From<Signal<T>> for ReadSignal<T> {
    fn from(s: Signal<T>) -> Self {
        s.read_only()
    }
}

impl<T: 'static> Signal<T> {
    pub fn read_only(self) -> ReadSignal<T> {
        ReadSignal {
            key: self.key,
            _marker: PhantomData,
        }
    }

    pub fn is_alive(self) -> bool {
        ARENA.with(|a| a.borrow().signals.contains_key(self.key))
    }

    pub fn try_with<R>(self, f: impl FnOnce(&T) -> R) -> Option<R> {
        let rc = ARENA.with(|a| a.borrow().signals.get(self.key).map(|s| s.value.clone()))?;
        record_dependency(SourceId::Signal(self.key));
        let borrowed = rc.borrow();
        borrowed.downcast_ref::<T>().map(f)
    }

    pub fn with<R>(self, f: impl FnOnce(&T) -> R) -> R {
        self.try_with(f)
            .expect("attempted to read a disposed Signal")
    }

    pub fn try_get(self) -> Option<T>
    where
        T: Clone,
    {
        self.try_with(Clone::clone)
    }

    pub fn get(self) -> T
    where
        T: Clone,
    {
        self.with(Clone::clone)
    }

    /// Read without tracking a reactive dependency.
    pub fn get_untracked(self) -> T
    where
        T: Clone,
    {
        untrack(|| self.get())
    }

    pub fn set(self, value: T) {
        self.update(|slot| *slot = value);
    }

    pub fn set_if_changed(self, value: T) -> bool
    where
        T: PartialEq,
    {
        let changed = self.try_with(|curr| *curr != value).unwrap_or(false);
        if changed {
            self.set(value);
        }
        changed
    }

    pub fn update<R>(self, f: impl FnOnce(&mut T) -> R) -> R {
        let (rc, subs, nodes) = ARENA.with(|a| {
            let arena = a.borrow();
            let sig = arena
                .signals
                .get(self.key)
                .expect("attempted to update a disposed Signal");
            (
                sig.value.clone(),
                sig.subscribers.iter().copied().collect::<Vec<_>>(),
                sig.node_bindings.clone(),
            )
        });
        let result = {
            let mut guard = rc.borrow_mut();
            let typed = guard
                .downcast_mut::<T>()
                .expect("Signal type mismatch in arena");
            f(typed)
        };
        propagate_dirty(subs, nodes);
        result
    }

    /// Subscribe a retained widget [`NodeId`] so updates mark it dirty directly.
    pub fn bind_node(self, node: NodeId, flags: DirtyFlags) {
        ARENA.with(|a| {
            let mut arena = a.borrow_mut();
            if let Some(sig) = arena.signals.get_mut(self.key) {
                if let Some((_, existing)) = sig.node_bindings.iter_mut().find(|(n, _)| *n == node)
                {
                    *existing |= flags;
                } else {
                    sig.node_bindings.push((node, flags));
                }
            }
        });
    }
}

impl<T: 'static> ReadSignal<T> {
    pub fn is_alive(self) -> bool {
        ARENA.with(|a| a.borrow().signals.contains_key(self.key))
    }

    pub fn try_with<R>(self, f: impl FnOnce(&T) -> R) -> Option<R> {
        Signal::<T> {
            key: self.key,
            _marker: PhantomData,
        }
        .try_with(f)
    }

    pub fn with<R>(self, f: impl FnOnce(&T) -> R) -> R {
        Signal::<T> {
            key: self.key,
            _marker: PhantomData,
        }
        .with(f)
    }

    pub fn try_get(self) -> Option<T>
    where
        T: Clone,
    {
        self.try_with(Clone::clone)
    }

    pub fn get(self) -> T
    where
        T: Clone,
    {
        self.with(Clone::clone)
    }

    pub fn bind_node(self, node: NodeId, flags: DirtyFlags) {
        Signal::<T> {
            key: self.key,
            _marker: PhantomData,
        }
        .bind_node(node, flags);
    }
}

/// Lazy memoized derived reactive handle (`Copy`).
pub struct Computed<T: 'static> {
    key: ComputedKey,
    _marker: PhantomData<fn() -> T>,
}

impl<T: 'static> Copy for Computed<T> {}
impl<T: 'static> Clone for Computed<T> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T: 'static> PartialEq for Computed<T> {
    fn eq(&self, other: &Self) -> bool {
        self.key == other.key
    }
}
impl<T: 'static> Eq for Computed<T> {}
impl<T: 'static> std::fmt::Debug for Computed<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("Computed").field(&self.key).finish()
    }
}

impl<T: 'static> Computed<T> {
    fn ensure_evaluated(self) -> Option<Rc<RefCell<Box<dyn Any>>>> {
        let needs_eval = ARENA.with(|a| {
            let arena = a.borrow();
            let comp = arena.computeds.get(self.key)?;
            Some(comp.dirty || comp.value.is_none())
        })?;

        if needs_eval {
            let prepared = ARENA.with(|a| {
                let mut arena = a.borrow_mut();
                let comp = arena.computeds.get_mut(self.key)?;
                let old_sources = std::mem::take(&mut comp.sources);
                let compute = comp.compute.clone();
                detach_subscriber(&mut arena, SubscriberId::Computed(self.key), &old_sources);
                arena.observer_stack.push(SubscriberId::Computed(self.key));
                Some(compute)
            })?;

            let new_val = prepared.borrow_mut()();

            ARENA.with(|a| {
                let mut arena = a.borrow_mut();
                if arena.observer_stack.last() == Some(&SubscriberId::Computed(self.key)) {
                    arena.observer_stack.pop();
                }
                if let Some(comp) = arena.computeds.get_mut(self.key) {
                    comp.value = Some(Rc::new(RefCell::new(new_val)));
                    comp.dirty = false;
                }
            });
        }

        ARENA.with(|a| {
            a.borrow()
                .computeds
                .get(self.key)
                .and_then(|c| c.value.clone())
        })
    }

    pub fn try_with<R>(self, f: impl FnOnce(&T) -> R) -> Option<R> {
        let rc = self.ensure_evaluated()?;
        record_dependency(SourceId::Computed(self.key));
        let borrowed = rc.borrow();
        borrowed.downcast_ref::<T>().map(f)
    }

    pub fn with<R>(self, f: impl FnOnce(&T) -> R) -> R {
        self.try_with(f)
            .expect("attempted to read a disposed Computed")
    }

    pub fn get(self) -> T
    where
        T: Clone,
    {
        self.with(Clone::clone)
    }

    pub fn bind_node(self, node: NodeId, flags: DirtyFlags) {
        ARENA.with(|a| {
            let mut arena = a.borrow_mut();
            if let Some(comp) = arena.computeds.get_mut(self.key) {
                if let Some((_, existing)) = comp.node_bindings.iter_mut().find(|(n, _)| *n == node)
                {
                    *existing |= flags;
                } else {
                    comp.node_bindings.push((node, flags));
                }
            }
        });
    }
}

/// Handle to a reactive side-effect bound to a [`Scope`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Effect {
    key: EffectKey,
}

impl Effect {
    pub fn dispose(self) {
        ARENA.with(|a| {
            let mut arena = a.borrow_mut();
            let sources = arena
                .effects
                .get_mut(self.key)
                .map(|e| {
                    e.disposed = true;
                    std::mem::take(&mut e.sources)
                })
                .unwrap_or_default();
            detach_subscriber(&mut arena, SubscriberId::Effect(self.key), &sources);
            arena.effects.remove(self.key);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use slotmap::SlotMap;
    use std::cell::Cell;

    #[test]
    fn signals_are_copy_and_track_effects() {
        reset_runtime();
        let cx = Scope::root();
        let count = cx.signal(0i32);
        let label = cx.signal("hello".to_string());
        let observed = Rc::new(Cell::new(0));
        let obs = observed.clone();

        // Notice: `count` and `label` are Copy even though `String` is !Copy.
        cx.effect(move || {
            obs.set(count.get() + label.with(|s| s.len() as i32));
        });
        assert_eq!(observed.get(), 5);

        count.set(10);
        assert_eq!(observed.get(), 15);
        label.update(|s| s.push('!'));
        assert_eq!(observed.get(), 16);
    }

    #[test]
    fn computed_is_lazy_and_diamond_consistent() {
        reset_runtime();
        let cx = Scope::root();
        let base = cx.signal(2);
        let eval_count = Rc::new(Cell::new(0));
        let ec = eval_count.clone();
        let doubled = cx.computed(move || {
            ec.set(ec.get() + 1);
            base.get() * 2
        });

        // Never evaluated until read
        assert_eq!(eval_count.get(), 0);
        assert_eq!(doubled.get(), 4);
        assert_eq!(eval_count.get(), 1);
        // Cached on repeated read
        assert_eq!(doubled.get(), 4);
        assert_eq!(eval_count.get(), 1);

        // Dirty on change, still lazy until read
        base.set(5);
        assert_eq!(eval_count.get(), 1);
        assert_eq!(doubled.get(), 10);
        assert_eq!(eval_count.get(), 2);
    }

    #[test]
    fn batch_coalesces_effects() {
        reset_runtime();
        let cx = Scope::root();
        let a = cx.signal(1);
        let b = cx.signal(2);
        let runs = Rc::new(Cell::new(0));
        let r = runs.clone();
        cx.effect(move || {
            let _ = a.get() + b.get();
            r.set(r.get() + 1);
        });
        assert_eq!(runs.get(), 1);

        batch(|| {
            a.set(10);
            b.set(20);
        });
        assert_eq!(runs.get(), 2);
    }

    #[test]
    fn scope_disposal_frees_signals_and_runs_cleanups() {
        reset_runtime();
        let root = Scope::root();
        let child = root.child_scope();
        let sig = child.signal(42);
        let cleaned = Rc::new(Cell::new(false));
        let c = cleaned.clone();
        child.on_cleanup(move || c.set(true));

        assert!(sig.is_alive());
        child.dispose();
        assert!(cleaned.get());
        assert!(!sig.is_alive());
        assert_eq!(sig.try_get(), None);
    }

    #[test]
    fn node_dirty_propagation_and_context_lookup() {
        reset_runtime();
        let root = Scope::root();
        root.provide_context(123u32);
        let child = root.child_scope();
        assert_eq!(child.use_context::<u32>(), Some(123));

        let mut nodes: SlotMap<NodeId, ()> = SlotMap::with_key();
        let n1 = nodes.insert(());
        let sig = child.signal(0.0f32);
        sig.bind_node(n1, DirtyFlags::PAINT);
        sig.set(0.5);

        let dirty = drain_dirty_nodes();
        assert_eq!(dirty, vec![(n1, DirtyFlags::PAINT)]);
        assert!(drain_dirty_nodes().is_empty());
    }
}
