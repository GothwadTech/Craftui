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
        let dt = dt_sec.clamp(0.0001, 0.05);
        let force = -self.stiffness * (self.value - self.target);
        let damp = -self.damping * self.velocity;
        let accel = force + damp;
        self.velocity += accel * dt;
        self.value += self.velocity * dt;

        let settled = (self.value - self.target).abs() < 0.001 && self.velocity.abs() < 0.01;
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
}
