//! Small, dependency-free motion helpers. Everything is time-based so the UI stays
//! smooth regardless of the frame rate.

use eframe::egui;

/// Frame time in seconds, clamped so a stalled frame cannot make a spring explode.
pub fn dt(ctx: &egui::Context) -> f32 {
    ctx.input(|i| i.stable_dt).clamp(0.001, 0.05)
}

/// Exponential smoothing toward a target. Cheap, stable, and reads like a soft spring
/// for the short distances used by hover and selection states.
#[derive(Clone, Copy, Debug)]
pub struct Blend {
    pub value: f32,
    pub target: f32,
    pub tau: f32,
}

impl Blend {
    pub fn new(value: f32, tau: f32) -> Self {
        Self {
            value,
            target: value,
            tau,
        }
    }

    pub fn to(&mut self, target: f32) {
        self.target = target;
    }

    pub fn snap(&mut self, value: f32) {
        self.value = value;
        self.target = value;
    }

    pub fn update(&mut self, dt: f32) {
        let k = 1.0 - (-dt / self.tau.max(0.001)).exp();
        self.value += (self.target - self.value) * k;
        if (self.target - self.value).abs() < 0.0005 {
            self.value = self.target;
        }
    }

    pub fn active(&self) -> bool {
        (self.target - self.value).abs() > 0.0005
    }
}

impl Default for Blend {
    fn default() -> Self {
        Self::new(0.0, 0.10)
    }
}

/// A real spring, used where overshoot is desirable (the primary button lift).
#[derive(Clone, Copy, Debug)]
pub struct Spring {
    pub value: f32,
    pub target: f32,
    pub velocity: f32,
    pub stiffness: f32,
    pub damping: f32,
}

impl Spring {
    pub fn new(value: f32) -> Self {
        Self {
            value,
            target: value,
            velocity: 0.0,
            stiffness: 260.0,
            damping: 24.0,
        }
    }

    pub fn to(&mut self, target: f32) {
        self.target = target;
    }

    pub fn update(&mut self, dt: f32) {
        let accel = (self.target - self.value) * self.stiffness - self.velocity * self.damping;
        self.velocity += accel * dt;
        self.value += self.velocity * dt;
        if (self.target - self.value).abs() < 0.0004 && self.velocity.abs() < 0.002 {
            self.value = self.target;
            self.velocity = 0.0;
        }
    }

    pub fn active(&self) -> bool {
        (self.target - self.value).abs() > 0.0004 || self.velocity.abs() > 0.002
    }
}

pub fn ease_out_cubic(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(3)
}

pub fn ease_out_quint(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(5)
}

pub fn ease_in_out_cubic(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    if t < 0.5 {
        4.0 * t * t * t
    } else {
        1.0 - (-2.0 * t + 2.0).powi(3) / 2.0
    }
}

/// 0..1..0 breathing curve used by the running dot and the logo halo.
pub fn breathe(seconds: f32, period: f32) -> f32 {
    let phase = (seconds / period.max(0.001)) * std::f32::consts::TAU;
    0.5 - 0.5 * phase.cos()
}
