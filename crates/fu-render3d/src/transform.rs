use glam::{Mat4, Quat, Vec3};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Transform {
    pub translation: Vec3,
    pub rotation: Quat,
    pub scale: Vec3,
}

impl Transform {
    pub const IDENTITY: Self = Self {
        translation: Vec3::ZERO,
        rotation: Quat::IDENTITY,
        scale: Vec3::ONE,
    };

    pub fn from_translation(translation: Vec3) -> Self {
        Self {
            translation,
            ..Self::IDENTITY
        }
    }

    /// Transform placed at `eye` whose local -Z axis points at `target`.
    pub fn look_at(eye: Vec3, target: Vec3, up: Vec3) -> Self {
        let direction = (target - eye).normalize_or_zero();
        if direction == Vec3::ZERO {
            return Self::from_translation(eye);
        }
        let world = Mat4::look_to_rh(eye, direction, up).inverse();
        let (_, rotation, translation) = world.to_scale_rotation_translation();
        Self {
            translation,
            rotation,
            scale: Vec3::ONE,
        }
    }

    pub fn matrix(&self) -> Mat4 {
        Mat4::from_scale_rotation_translation(self.scale, self.rotation, self.translation)
    }

    pub fn is_finite(&self) -> bool {
        self.translation.is_finite() && self.rotation.is_finite() && self.scale.is_finite()
    }
}

impl Default for Transform {
    fn default() -> Self {
        Self::IDENTITY
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: Vec3, b: Vec3) -> bool {
        (a - b).length() < 1e-4
    }

    #[test]
    fn matrix_applies_scale_then_translation() {
        let t = Transform {
            translation: Vec3::new(1.0, 2.0, 3.0),
            rotation: Quat::IDENTITY,
            scale: Vec3::splat(2.0),
        };
        assert!(close(
            t.matrix().transform_point3(Vec3::ONE),
            Vec3::new(3.0, 4.0, 5.0)
        ));
    }

    #[test]
    fn look_at_points_negative_z_at_target() {
        let t = Transform::look_at(Vec3::new(0.0, 0.0, 5.0), Vec3::ZERO, Vec3::Y);
        let ahead = t.matrix().transform_point3(Vec3::new(0.0, 0.0, -1.0));
        assert!(close(ahead, Vec3::new(0.0, 0.0, 4.0)));
    }

    #[test]
    fn look_at_degenerate_target_keeps_position() {
        let eye = Vec3::new(1.0, 1.0, 1.0);
        assert_eq!(Transform::look_at(eye, eye, Vec3::Y).translation, eye);
    }
}
