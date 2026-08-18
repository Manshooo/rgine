use std::time::{Duration, Instant};

#[derive(Debug)]
pub struct Clock {
    last: Instant,
    accumulator: Duration,
}

impl Clock {
    pub fn new() -> Self {
        Self {
            last: Instant::now(),
            accumulator: Duration::ZERO,
        }
    }

    pub fn tick(&mut self, fixed_dt: Duration) -> u32 {
        let now = Instant::now();
        let elapsed = now.saturating_duration_since(self.last);
        self.last = now;
        self.accumulator += elapsed;

        let mut steps = 0;
        while self.accumulator >= fixed_dt {
            self.accumulator -= fixed_dt;
            steps += 1;
        }
        steps
    }
}

impl Default for Clock {
    fn default() -> Self {
        Self::new()
    }
}
