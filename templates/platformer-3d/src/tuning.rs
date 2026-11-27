use serde::Deserialize;

#[derive(Clone, Copy, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Tuning {
    pub move_speed: f32,
    pub jump_speed: f32,
    pub fall_acceleration: f32,
}

impl Tuning {
    pub fn bundled() -> Self {
        ron::from_str(include_str!("../assets/tuning.ron")).expect("bundled platformer tuning")
    }

    #[cfg(feature = "dev-reload")]
    pub fn validate(&self) -> std::io::Result<()> {
        for (field, value) in [
            ("move_speed", self.move_speed),
            ("jump_speed", self.jump_speed),
            ("fall_acceleration", self.fall_acceleration),
        ] {
            if !value.is_finite() || value <= 0.0 {
                return Err(std::io::Error::other(format!(
                    "{field} must be finite and positive"
                )));
            }
        }
        Ok(())
    }
}
