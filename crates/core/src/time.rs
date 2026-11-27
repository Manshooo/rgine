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

    /// Time since the last wall-clock sample, without changing the accumulator.
    pub fn sample_elapsed(&mut self) -> Duration {
        let now = Instant::now();
        let elapsed = now.saturating_duration_since(self.last);
        self.last = now;
        elapsed
    }

    /// Samples wall time and returns the number of complete fixed steps.
    ///
    /// # Panics
    ///
    /// If `fixed_dt` is zero.
    pub fn tick(&mut self, fixed_dt: Duration) -> u32 {
        let elapsed = self.sample_elapsed();
        self.accumulate(elapsed, fixed_dt, u32::MAX)
    }

    /// Accumulates an explicit duration, returning at most `max_steps` steps.
    /// Excess whole steps are discarded and the fractional remainder is kept.
    ///
    /// # Panics
    ///
    /// If `fixed_dt` is zero.
    pub fn accumulate(&mut self, elapsed: Duration, fixed_dt: Duration, max_steps: u32) -> u32 {
        assert!(!fixed_dt.is_zero(), "fixed timestep must be positive");
        let nanos = self.accumulator.as_nanos() + elapsed.as_nanos();
        let step = fixed_dt.as_nanos();
        let remainder = nanos % step;
        self.accumulator = Duration::new(
            (remainder / 1_000_000_000) as u64,
            (remainder % 1_000_000_000) as u32,
        );
        (nanos / step).min(u128::from(max_steps)) as u32
    }

    /// The elapsed time left over after the last fixed-step accumulation.
    #[must_use]
    pub fn remainder(&self) -> Duration {
        self.accumulator
    }
}

impl Default for Clock {
    fn default() -> Self {
        Self::new()
    }
}
