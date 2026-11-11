use craftui_core::Color;

/// Animation easing curves, tweens, and spring physics.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum Easing {
    Linear,
    EaseIn,
    EaseOut,
    #[default]
    EaseInOut,
}

impl Easing {
    pub fn sample(self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        match self {
            Self::Linear => t,
            Self::EaseIn => t * t * t,
            Self::EaseOut => {
                let inv = 1.0 - t;
                1.0 - inv * inv * inv
            }
            Self::EaseInOut => {
                if t < 0.5 {
                    4.0 * t * t * t
                } else {
                    1.0 - (-2.0 * t + 2.0).powi(3) * 0.5
                }
            }
        }
    }
}

/// Property identifier for per-node declarative and imperative transition specs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AnimatableProp {
    Opacity,
    TranslateX,
    TranslateY,
    ScaleX,
    ScaleY,
    Rotation,
    BackgroundColor,
    BorderColor,
}

/// Transition timing model: either a duration + easing curve or damped spring physics.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TransitionCurve {
    Tween { duration_sec: f32, easing: Easing },
    Spring { stiffness: f32, damping: f32 },
}

impl Default for TransitionCurve {
    fn default() -> Self {
        Self::Spring {
            stiffness: 280.0,
            damping: 26.0,
        }
    }
}

impl TransitionCurve {
    pub fn tween(duration_sec: f32, easing: Easing) -> Self {
        Self::Tween {
            duration_sec: duration_sec.max(0.001),
            easing,
        }
    }

    pub fn spring(stiffness: f32, damping: f32) -> Self {
        Self::Spring {
            stiffness: stiffness.max(1.0),
            damping: damping.max(0.1),
        }
    }

    pub fn snappy() -> Self {
        Self::Spring {
            stiffness: 420.0,
            damping: 30.0,
        }
    }

    pub fn gentle() -> Self {
        Self::Spring {
            stiffness: 200.0,
            damping: 24.0,
        }
    }

    pub fn instant() -> Self {
        Self::Tween {
            duration_sec: 0.001,
            easing: Easing::Linear,
        }
    }
}

/// Animated scalar value supporting retargetable tweens and velocity-preserving springs.
#[derive(Debug, Clone, Copy)]
pub struct AnimatedFloat {
    pub value: f32,
    pub target: f32,
    pub velocity: f32,
    pub curve: TransitionCurve,
    start_value: f32,
    elapsed_sec: f32,
    animating: bool,
}

impl AnimatedFloat {
    pub fn new(initial: f32, curve: TransitionCurve) -> Self {
        Self {
            value: initial,
            target: initial,
            velocity: 0.0,
            curve,
            start_value: initial,
            elapsed_sec: 0.0,
            animating: false,
        }
    }

    pub fn set_curve(&mut self, curve: TransitionCurve) {
        self.curve = curve;
    }

    pub fn set_target(&mut self, target: f32) {
        if (target - self.target).abs() <= 1e-4
            && (!self.animating || (target - self.value).abs() <= 1e-4)
        {
            return;
        }
        self.target = target;
        match self.curve {
            TransitionCurve::Tween { .. } => {
                self.start_value = self.value;
                self.elapsed_sec = 0.0;
                self.velocity = 0.0;
                self.animating = (self.target - self.value).abs() > 1e-4;
                if !self.animating {
                    self.value = self.target;
                }
            }
            TransitionCurve::Spring { .. } => {
                self.animating =
                    (self.target - self.value).abs() > 0.0005 || self.velocity.abs() > 0.005;
                if !self.animating {
                    self.value = self.target;
                    self.velocity = 0.0;
                }
            }
        }
    }

    pub fn snap(&mut self, value: f32) {
        self.value = value;
        self.target = value;
        self.start_value = value;
        self.velocity = 0.0;
        self.elapsed_sec = 0.0;
        self.animating = false;
    }

    pub fn is_animating(&self) -> bool {
        self.animating
    }

    /// Advance animation by `dt_sec`. Returns `true` if `value` changed during this step
    /// (including the final settle frame; check [`Self::is_animating`] for whether more frames are needed).
    pub fn step(&mut self, dt_sec: f32) -> bool {
        if !self.animating {
            return false;
        }
        let dt_total = dt_sec.clamp(0.0001, 0.05);
        match self.curve {
            TransitionCurve::Tween {
                duration_sec,
                easing,
            } => {
                let dur = duration_sec.max(0.001);
                self.elapsed_sec = (self.elapsed_sec + dt_total).min(dur);
                let t = easing.sample(self.elapsed_sec / dur);
                self.value = self.start_value + (self.target - self.start_value) * t;
                if self.elapsed_sec >= dur - 1e-6 {
                    self.value = self.target;
                    self.animating = false;
                }
                true
            }
            TransitionCurve::Spring { stiffness, damping } => {
                let steps = ((dt_total / 0.008).ceil() as u32).clamp(1, 8);
                let dt = dt_total / steps as f32;
                for _ in 0..steps {
                    let force = -stiffness * (self.value - self.target);
                    let damp = -damping * self.velocity;
                    let accel = force + damp;
                    self.velocity += accel * dt;
                    self.value += self.velocity * dt;
                }
                let settled =
                    (self.value - self.target).abs() < 0.0005 && self.velocity.abs() < 0.005;
                if settled {
                    self.value = self.target;
                    self.velocity = 0.0;
                    self.animating = false;
                }
                true
            }
        }
    }
}

/// Animated RGBA [`Color`] smoothly interpolating when its target color changes.
#[derive(Debug, Clone, Copy)]
pub struct AnimatedColor {
    pub value: Color,
    from: Color,
    target: Color,
    progress: AnimatedFloat,
}

impl AnimatedColor {
    pub fn new(initial: Color, curve: TransitionCurve) -> Self {
        Self {
            value: initial,
            from: initial,
            target: initial,
            progress: AnimatedFloat::new(1.0, curve),
        }
    }

    pub fn target(&self) -> Color {
        self.target
    }

    pub fn set_curve(&mut self, curve: TransitionCurve) {
        self.progress.set_curve(curve);
    }

    pub fn set_target(&mut self, target: Color) {
        if target == self.target {
            return;
        }
        self.from = self.value;
        self.target = target;
        self.progress.snap(0.0);
        self.progress.set_target(1.0);
    }

    pub fn snap(&mut self, color: Color) {
        self.value = color;
        self.from = color;
        self.target = color;
        self.progress.snap(1.0);
    }

    pub fn is_animating(&self) -> bool {
        self.progress.is_animating()
    }

    pub fn step(&mut self, dt_sec: f32) -> bool {
        if !self.progress.step(dt_sec) {
            return false;
        }
        let t = self.progress.value.clamp(0.0, 1.0);
        self.value = self.from.lerp(self.target, t);
        if !self.progress.is_animating() {
            self.value = self.target;
        }
        true
    }
}

/// Time-based tween animation.
#[derive(Debug, Clone, Copy)]
pub struct Tween {
    pub from: f32,
    pub to: f32,
    pub duration_sec: f32,
    pub elapsed_sec: f32,
    pub easing: Easing,
}

impl Tween {
    pub fn new(from: f32, to: f32, duration_sec: f32, easing: Easing) -> Self {
        Self {
            from,
            to,
            duration_sec: duration_sec.max(0.001),
            elapsed_sec: 0.0,
            easing,
        }
    }

    pub fn step(&mut self, dt_sec: f32) -> (f32, bool) {
        self.elapsed_sec = (self.elapsed_sec + dt_sec).min(self.duration_sec);
        let t = self.easing.sample(self.elapsed_sec / self.duration_sec);
        let value = self.from + (self.to - self.from) * t;
        let active = self.elapsed_sec < self.duration_sec;
        (value, active)
    }
}

/// Damped harmonic spring physics solver for smooth UI transitions and overscroll bounce.
#[derive(Debug, Clone, Copy)]
pub struct Spring {
    pub value: f32,
    pub target: f32,
    pub velocity: f32,
    pub stiffness: f32,
    pub damping: f32,
}

impl Default for Spring {
    fn default() -> Self {
        Self::new(0.0, 240.0, 26.0)
    }
}

impl Spring {
    pub fn new(initial: f32, stiffness: f32, damping: f32) -> Self {
        Self {
            value: initial,
            target: initial,
            velocity: 0.0,
            stiffness,
            damping,
        }
    }

    pub fn set_target(&mut self, target: f32) {
        self.target = target;
    }

    /// Advance spring by `dt_sec`. Returns `true` if still animating.
    pub fn step(&mut self, dt_sec: f32) -> bool {
        let dt_total = dt_sec.clamp(0.0001, 0.05);
        let steps = ((dt_total / 0.008).ceil() as u32).clamp(1, 8);
        let dt = dt_total / steps as f32;
        for _ in 0..steps {
            let force = -self.stiffness * (self.value - self.target);
            let damp = -self.damping * self.velocity;
            let accel = force + damp;
            self.velocity += accel * dt;
            self.value += self.velocity * dt;
        }

        let settled = (self.value - self.target).abs() < 0.0005 && self.velocity.abs() < 0.005;
        if settled {
            self.value = self.target;
            self.velocity = 0.0;
            false
        } else {
            true
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spring_converges_to_target() {
        let mut s = Spring::new(0.0, 220.0, 24.0);
        s.set_target(100.0);
        for _ in 0..240 {
            if !s.step(1.0 / 60.0) {
                break;
            }
        }
        assert!((s.value - 100.0).abs() < 0.01);
    }

    #[test]
    fn animated_float_tween_and_color_retarget_smoothly() {
        let mut f = AnimatedFloat::new(0.0, TransitionCurve::tween(0.16, Easing::EaseOut));
        f.set_target(1.0);
        assert!(f.is_animating());
        assert!(f.step(0.05));
        assert!(f.value > 0.2 && f.value < 0.99);
        f.set_target(0.25);
        for _ in 0..20 {
            f.step(0.016);
        }
        assert!(!f.is_animating());
        assert!((f.value - 0.25).abs() < 1e-4);

        let mut c = AnimatedColor::new(Color::BLACK, TransitionCurve::tween(0.1, Easing::Linear));
        c.set_target(Color::WHITE);
        assert!(c.step(0.05));
        assert!(c.value.r > 100 && c.value.r < 160);
        for _ in 0..10 {
            c.step(0.02);
        }
        assert_eq!(c.value, Color::WHITE);
    }
}
