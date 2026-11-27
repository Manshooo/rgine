use std::io;

use serde::Deserialize;

pub(super) const BUNDLED: &str = include_str!("tuning.ron");

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Tuning {
    pub(super) move_speed: f32,
    strike_speed_curve: Vec<SpeedPoint>,
    pub(super) transitions: Transitions,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SpeedPoint {
    at: f32,
    speed: f32,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Transitions {
    pub(super) strike_to_cooldown_ticks: u32,
    pub(super) cooldown_to_ready_ticks: u32,
}

impl Tuning {
    pub(super) fn bundled() -> Self {
        let tuning: Self = ron::from_str(BUNDLED).expect("bundled arena tuning parses");
        tuning.validate().expect("bundled arena tuning is valid");
        tuning
    }

    pub(super) fn validate(&self) -> io::Result<()> {
        if !self.move_speed.is_finite() || self.move_speed <= 0.0 {
            return Err(io::Error::other("move_speed must be finite and positive"));
        }
        let points = &self.strike_speed_curve;
        if points.first().is_none_or(|point| point.at != 0.0)
            || points.last().is_none_or(|point| point.at != 1.0)
            || !points.windows(2).all(|pair| pair[0].at < pair[1].at)
        {
            return Err(io::Error::other(
                "strike_speed_curve positions must increase strictly from 0 to 1",
            ));
        }
        if points
            .iter()
            .any(|point| !point.speed.is_finite() || point.speed < 0.0)
        {
            return Err(io::Error::other(
                "strike_speed_curve speeds must be finite and nonnegative",
            ));
        }
        if self.transitions.strike_to_cooldown_ticks == 0 {
            return Err(io::Error::other(
                "strike_to_cooldown_ticks must be positive",
            ));
        }
        Ok(())
    }

    pub(super) fn strike_speed(&self, progress: f32) -> f32 {
        let points = &self.strike_speed_curve;
        let upper = points.partition_point(|point| point.at < progress).max(1);
        let before = &points[upper - 1];
        let after = &points[upper];
        let fraction = (progress - before.at) / (after.at - before.at);
        before.speed + (after.speed - before.speed) * fraction
    }
}
