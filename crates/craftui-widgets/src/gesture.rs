use craftui_core::{NodeId, Point, PointerEvent, PointerId, PointerPhase};
use std::collections::{HashMap, VecDeque};

/// Sliding-window velocity estimator for touch fling momentum physics.
#[derive(Debug, Clone, Default)]
pub struct VelocityTracker {
    samples: VecDeque<(Point, u64)>,
}

impl VelocityTracker {
    pub fn add_sample(&mut self, pos: Point, timestamp_ms: u64) {
        self.samples.push_back((pos, timestamp_ms));
        while self.samples.len() > 12 {
            self.samples.pop_front();
        }
        while let Some(&(_, ts)) = self.samples.front() {
            if timestamp_ms.saturating_sub(ts) > 120 && self.samples.len() > 2 {
                self.samples.pop_front();
            } else {
                break;
            }
        }
    }

    pub fn velocity_px_per_sec(&self) -> (f32, f32) {
        let (Some(&(first, t0)), Some(&(last, t1))) = (self.samples.front(), self.samples.back())
        else {
            return (0.0, 0.0);
        };
        let dt_ms = t1.saturating_sub(t0);
        if dt_ms < 4 {
            return (0.0, 0.0);
        }
        let dt = dt_ms as f32 / 1000.0;
        ((last.x - first.x) / dt, (last.y - first.y) / dt)
    }

    pub fn clear(&mut self) {
        self.samples.clear();
    }
}

/// Inertial momentum fling and edge clamping physics for `ScrollView` / `VirtualList`.
#[derive(Debug, Clone, Copy)]
pub struct ScrollPhysics {
    pub friction: f32,
}

impl Default for ScrollPhysics {
    fn default() -> Self {
        Self { friction: 5.5 }
    }
}

impl ScrollPhysics {
    /// Step momentum velocity by `dt_sec`. Returns `(delta_offset, still_moving)`.
    pub fn step_fling(&self, velocity: &mut f32, dt_sec: f32) -> (f32, bool) {
        if velocity.abs() < 12.0 {
            *velocity = 0.0;
            return (0.0, false);
        }
        let dt = dt_sec.clamp(0.0001, 0.05);
        let decay = (-self.friction * dt).exp();
        let avg_v = *velocity * (1.0 + decay) * 0.5;
        *velocity *= decay;
        let delta = avg_v * dt;
        let active = velocity.abs() >= 12.0;
        if !active {
            *velocity = 0.0;
        }
        (delta, active)
    }
}

/// Recognized high-level gesture events resolved by [`GestureArena`].
#[derive(Debug, Clone, PartialEq)]
pub enum RecognizedGesture {
    Tap {
        node: NodeId,
        position: Point,
    },
    DoubleTap {
        node: NodeId,
        position: Point,
    },
    LongPress {
        node: NodeId,
        position: Point,
    },
    DragStart {
        node: NodeId,
        position: Point,
    },
    DragUpdate {
        node: NodeId,
        position: Point,
        delta: Point,
    },
    DragEnd {
        node: NodeId,
        velocity: (f32, f32),
    },
    PinchRotate {
        center: Point,
        scale_delta: f32,
        angle_delta_rad: f32,
    },
}

#[derive(Debug, Clone)]
struct ActivePointer {
    down_pos: Point,
    last_pos: Point,
    down_time_ms: u64,
    captured_node: Option<NodeId>,
    dragging: bool,
    long_press_fired: bool,
    tracker: VelocityTracker,
}

/// Disambiguates competing Tap, DoubleTap, LongPress, Drag, and 2-finger Pinch/Rotate gestures.
#[derive(Debug, Default)]
pub struct GestureArena {
    pointers: HashMap<PointerId, ActivePointer>,
    last_tap: Option<(NodeId, Point, u64)>,
    prev_two_finger: Option<(Point, f32, f32)>,
    pub drag_slop_dp: f32,
    pub double_tap_window_ms: u64,
    pub long_press_ms: u64,
}

impl GestureArena {
    pub fn new() -> Self {
        Self {
            pointers: HashMap::new(),
            last_tap: None,
            prev_two_finger: None,
            drag_slop_dp: 6.0,
            double_tap_window_ms: 300,
            long_press_ms: 450,
        }
    }

    pub fn active_capture(&self, id: PointerId) -> Option<NodeId> {
        self.pointers.get(&id).and_then(|p| p.captured_node)
    }

    pub fn process_pointer(
        &mut self,
        event: &PointerEvent,
        hit_target: Option<NodeId>,
    ) -> Vec<RecognizedGesture> {
        let mut out = Vec::new();
        match event.phase {
            PointerPhase::Down => {
                let mut tracker = VelocityTracker::default();
                tracker.add_sample(event.position, event.timestamp_ms);
                self.pointers.insert(
                    event.id,
                    ActivePointer {
                        down_pos: event.position,
                        last_pos: event.position,
                        down_time_ms: event.timestamp_ms,
                        captured_node: hit_target,
                        dragging: false,
                        long_press_fired: false,
                        tracker,
                    },
                );
                if self.pointers.len() == 2 {
                    self.prev_two_finger = self.compute_two_finger_state();
                }
            }
            PointerPhase::Move => {
                if self.pointers.len() == 2 {
                    if let Some(p) = self.pointers.get_mut(&event.id) {
                        p.last_pos = event.position;
                    }
                    if let (Some((_old_c, old_d, old_a)), Some((new_c, new_d, new_a))) =
                        (self.prev_two_finger, self.compute_two_finger_state())
                    {
                        let scale_delta = if old_d > 1.0 { new_d / old_d } else { 1.0 };
                        let angle_delta_rad = new_a - old_a;
                        out.push(RecognizedGesture::PinchRotate {
                            center: new_c,
                            scale_delta,
                            angle_delta_rad,
                        });
                        self.prev_two_finger = Some((new_c, new_d, new_a));
                    }
                    return out;
                }

                if let Some(p) = self.pointers.get_mut(&event.id) {
                    let dx = event.position.x - p.last_pos.x;
                    let dy = event.position.y - p.last_pos.y;
                    p.last_pos = event.position;
                    p.tracker.add_sample(event.position, event.timestamp_ms);

                    if !p.dragging && p.down_pos.distance_to(event.position) >= self.drag_slop_dp {
                        p.dragging = true;
                        if let Some(node) = p.captured_node {
                            out.push(RecognizedGesture::DragStart {
                                node,
                                position: p.down_pos,
                            });
                        }
                    }
                    if p.dragging
                        && let Some(node) = p.captured_node
                    {
                        out.push(RecognizedGesture::DragUpdate {
                            node,
                            position: event.position,
                            delta: Point::new(dx, dy),
                        });
                    } else if !p.long_press_fired
                        && event.timestamp_ms.saturating_sub(p.down_time_ms) >= self.long_press_ms
                        && let Some(node) = p.captured_node
                    {
                        p.long_press_fired = true;
                        out.push(RecognizedGesture::LongPress {
                            node,
                            position: event.position,
                        });
                    }
                }
            }
            PointerPhase::Up => {
                self.prev_two_finger = None;
                if let Some(p) = self.pointers.remove(&event.id)
                    && let Some(node) = p.captured_node
                {
                    if p.dragging {
                        out.push(RecognizedGesture::DragEnd {
                            node,
                            velocity: p.tracker.velocity_px_per_sec(),
                        });
                    } else if !p.long_press_fired {
                        out.push(RecognizedGesture::Tap {
                            node,
                            position: event.position,
                        });
                        if let Some((prev_node, prev_pos, prev_ts)) = self.last_tap
                            && prev_node == node
                            && prev_pos.distance_to(event.position) < 24.0
                            && event.timestamp_ms.saturating_sub(prev_ts)
                                <= self.double_tap_window_ms
                        {
                            out.push(RecognizedGesture::DoubleTap {
                                node,
                                position: event.position,
                            });
                            self.last_tap = None;
                        } else {
                            self.last_tap = Some((node, event.position, event.timestamp_ms));
                        }
                    }
                }
            }
            PointerPhase::Cancel => {
                self.prev_two_finger = None;
                self.pointers.remove(&event.id);
            }
        }
        out
    }

    fn compute_two_finger_state(&self) -> Option<(Point, f32, f32)> {
        let mut iter = self.pointers.values();
        let a = iter.next()?.last_pos;
        let b = iter.next()?.last_pos;
        let center = Point::new((a.x + b.x) * 0.5, (a.y + b.y) * 0.5);
        let dx = b.x - a.x;
        let dy = b.y - a.y;
        let dist = (dx * dx + dy * dy).sqrt();
        let angle = dy.atan2(dx);
        Some((center, dist, angle))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use craftui_core::{Modifiers, PointerButton};
    use slotmap::SlotMap;

    #[test]
    fn arena_disambiguates_tap_double_tap_and_drag() {
        let mut nodes: SlotMap<NodeId, ()> = SlotMap::with_key();
        let btn = nodes.insert(());
        let mut arena = GestureArena::new();

        let down = |ts, x, y| PointerEvent {
            id: PointerId(1),
            phase: PointerPhase::Down,
            position: Point::new(x, y),
            button: Some(PointerButton::Primary),
            modifiers: Modifiers::default(),
            is_touch: true,
            timestamp_ms: ts,
        };
        let up = |ts, x, y| PointerEvent {
            id: PointerId(1),
            phase: PointerPhase::Up,
            position: Point::new(x, y),
            button: Some(PointerButton::Primary),
            modifiers: Modifiers::default(),
            is_touch: true,
            timestamp_ms: ts,
        };

        assert!(
            arena
                .process_pointer(&down(0, 10.0, 10.0), Some(btn))
                .is_empty()
        );
        let g1 = arena.process_pointer(&up(50, 11.0, 10.0), Some(btn));
        assert!(matches!(g1.as_slice(), [RecognizedGesture::Tap { .. }]));

        arena.process_pointer(&down(120, 11.0, 10.0), Some(btn));
        let g2 = arena.process_pointer(&up(160, 11.0, 10.0), Some(btn));
        assert_eq!(g2.len(), 2);
        assert!(matches!(g2[1], RecognizedGesture::DoubleTap { .. }));

        // Drag exceeds slop
        arena.process_pointer(&down(500, 0.0, 0.0), Some(btn));
        let drag = arena.process_pointer(
            &PointerEvent {
                phase: PointerPhase::Move,
                position: Point::new(0.0, 30.0),
                timestamp_ms: 520,
                ..down(500, 0.0, 0.0)
            },
            Some(btn),
        );
        assert!(matches!(drag[0], RecognizedGesture::DragStart { .. }));
        assert!(matches!(drag[1], RecognizedGesture::DragUpdate { .. }));
    }
}
