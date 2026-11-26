use engine_core::glam::Vec3;
use engine_ecs::Component;

/// A request and result for collision-aware translation of a kinematic body.
///
/// Attach it to an entity with a kinematic rigid body and a solid collider.
/// Submit displacement before the physics stage and read its result afterward.
#[derive(Component, Debug)]
pub struct KinematicMotion {
    pub(super) up: Vec3,
    pub(super) requested: Option<Vec3>,
    pub(super) result: Option<KinematicMove>,
}

impl Default for KinematicMotion {
    fn default() -> Self {
        Self::new(Vec3::Y)
    }
}

impl KinematicMotion {
    /// Creates a motion component with the given up direction for floor detection.
    ///
    /// # Panics
    ///
    /// If `up` is not a finite unit vector.
    #[must_use]
    pub fn new(up: Vec3) -> Self {
        assert!(
            up.is_finite() && up.is_normalized(),
            "kinematic up must be a unit vector"
        );
        Self {
            up,
            requested: None,
            result: None,
        }
    }

    /// Replaces the requested world-space displacement for the next fixed step.
    /// A zero displacement still checks for ground contact.
    ///
    /// # Panics
    ///
    /// If the displacement is non-finite.
    pub fn request_translation(&mut self, translation: Vec3) {
        assert!(
            translation.is_finite(),
            "kinematic translation must be finite"
        );
        self.requested = Some(translation);
    }

    /// Returns the last step's movement, if a request was applied to a
    /// kinematic body with a solid collider.
    #[must_use]
    pub fn result(&self) -> Option<KinematicMove> {
        self.result
    }
}

/// The result of one collision-aware translation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct KinematicMove {
    /// World-space displacement after sliding along obstacles.
    pub translation: Vec3,
    /// Whether the shape reaches a supporting surface at the end of the move.
    pub grounded: bool,
}
