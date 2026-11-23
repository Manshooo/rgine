//! Simulation ticks and the per-component timestamps used by queries.

use std::sync::atomic::{AtomicU32, Ordering};

const MAX_CHANGE_AGE: u32 = u32::MAX / 2;
const AGING_INTERVAL: u32 = 1 << 30;

/// A snapshot of a world's simulation clock.
///
/// Keep the tick from the previous system run and pass it to
/// [`World::query_filtered_since`](crate::World::query_filtered_since).
/// Use [`Tick::ZERO`] for a system's first run. Once running, refresh the
/// snapshot within half the 32-bit tick range.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tick(u32);

impl Tick {
    /// The tick immediately before a new world's initial tick.
    pub const ZERO: Self = Self(0);

    /// The initial tick of a newly created world.
    pub const INITIAL: Self = Self(1);

    pub(crate) const fn next(self) -> Self {
        Self(self.0.wrapping_add(1))
    }

    pub(crate) const fn previous(self) -> Self {
        Self(self.0.wrapping_sub(1))
    }

    pub(crate) const fn needs_aging(self) -> bool {
        self.0.is_multiple_of(AGING_INTERVAL)
    }

    fn in_window(self, last_run: Self, now: Self) -> bool {
        now.0.wrapping_sub(self.0) < now.0.wrapping_sub(last_run.0).min(MAX_CHANGE_AGE)
    }

    fn age(&mut self, now: Self) {
        if now.0.wrapping_sub(self.0) > MAX_CHANGE_AGE {
            *self = Self(now.0.wrapping_sub(MAX_CHANGE_AGE));
        }
    }
}

impl Default for Tick {
    fn default() -> Self {
        Self::INITIAL
    }
}

pub(crate) struct ComponentTicks {
    added: Tick,
    // Filters can inspect this independently of the whole-type values lock.
    changed: AtomicU32,
}

impl ComponentTicks {
    pub(crate) fn new(now: Tick) -> Self {
        Self {
            added: now,
            changed: AtomicU32::new(now.0),
        }
    }

    pub(crate) fn mark_changed(&self, now: Tick) {
        self.changed.store(now.0, Ordering::Relaxed);
    }

    pub(crate) fn matches<const ADDED: bool>(&self, last_run: Tick, now: Tick) -> bool {
        let tick = if ADDED {
            self.added
        } else {
            Tick(self.changed.load(Ordering::Relaxed))
        };
        tick.in_window(last_run, now)
    }

    pub(crate) fn age(&mut self, now: Tick) {
        self.added.age(now);
        let changed = self.changed.get_mut();
        let mut tick = Tick(*changed);
        tick.age(now);
        *changed = tick.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_events_in_the_open_closed_window_across_wraparound() {
        let last = Tick(u32::MAX - 2);
        let event = Tick(u32::MAX);
        let now = event.next();
        assert_eq!(now, Tick(0));
        assert!(event.in_window(last, now));
        assert!(now.in_window(last, now));
        assert!(!last.in_window(last, now));
        assert!(!now.next().in_window(last, now));
        assert!(!now.in_window(now, now));
    }

    #[test]
    fn aging_keeps_events_from_reappearing_after_a_full_turn() {
        let mut ticks = ComponentTicks::new(Tick(1));
        for now in [AGING_INTERVAL, 2 * AGING_INTERVAL, 3 * AGING_INTERVAL, 0] {
            let now = Tick(now);
            assert!(now.needs_aging());
            ticks.age(now);
        }
        assert!(!ticks.matches::<true>(Tick(u32::MAX), Tick(1)));
        assert!(!ticks.matches::<false>(Tick(u32::MAX), Tick(1)));
    }
}
