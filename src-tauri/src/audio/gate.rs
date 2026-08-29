use std::time::{Duration, Instant};

pub fn rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum_sq: f32 = samples.iter().map(|s| s * s).sum();
    (sum_sq / samples.len() as f32).sqrt()
}

/// Throttles level-meter updates to a fixed rate so the UI isn't flooded.
pub struct LevelThrottle {
    interval: Duration,
    last_emit: Instant,
}

impl LevelThrottle {
    pub fn new(hz: f32) -> Self {
        Self {
            interval: Duration::from_secs_f32(1.0 / hz),
            last_emit: Instant::now() - Duration::from_secs(1),
        }
    }

    pub fn should_emit(&mut self) -> bool {
        let now = Instant::now();
        if now.duration_since(self.last_emit) >= self.interval {
            self.last_emit = now;
            true
        } else {
            false
        }
    }
}
