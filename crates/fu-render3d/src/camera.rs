use crate::transform::Transform;
use crate::{RenderError, Result};
use glam::{Mat4, Vec3};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Projection {
    Perspective {
        fov_y_radians: f32,
        near: f32,
        far: f32,
    },
    Orthographic {
        height: f32,
        near: f32,
        far: f32,
    },
}

/// Right-handed camera looking down its local -Z axis. Projection output uses
/// a 0..1 clip-space depth range (wgpu/D3D/Metal/Vulkan convention).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Camera {
    pub transform: Transform,
    pub projection: Projection,
}

impl Default for Camera {
    fn default() -> Self {
        Self {
            transform: Transform::IDENTITY,
            projection: Projection::Perspective {
                fov_y_radians: 60.0_f32.to_radians(),
                near: 0.1,
                far: 1000.0,
            },
        }
    }
}

impl Camera {
    pub fn position(&self) -> Vec3 {
        self.transform.translation
    }

    /// Camera scale is ignored; only rotation and translation form the view.
    pub fn view_matrix(&self) -> Mat4 {
        Mat4::from_rotation_translation(self.transform.rotation, self.transform.translation)
            .inverse()
    }

    pub fn projection_matrix(&self, aspect: f32) -> Mat4 {
        match self.projection {
            Projection::Perspective {
                fov_y_radians,
                near,
                far,
            } => Mat4::perspective_rh(fov_y_radians, aspect, near, far),
            Projection::Orthographic { height, near, far } => {
                let half_h = height * 0.5;
                let half_w = half_h * aspect;
                Mat4::orthographic_rh(-half_w, half_w, -half_h, half_h, near, far)
            }
        }
    }

    pub fn view_projection(&self, aspect: f32) -> Mat4 {
        self.projection_matrix(aspect) * self.view_matrix()
    }

    pub fn validate(&self) -> Result<()> {
        if !self.transform.is_finite() {
            return Err(RenderError::invalid("camera", "transform must be finite"));
        }
        match self.projection {
            Projection::Perspective {
                fov_y_radians,
                near,
                far,
            } => {
                if !(fov_y_radians.is_finite()
                    && fov_y_radians > 0.0
                    && fov_y_radians < std::f32::consts::PI)
                {
                    return Err(RenderError::invalid("camera", "fov must be in (0, pi)"));
                }
                if !(near.is_finite() && far.is_finite() && near > 0.0 && far > near) {
                    return Err(RenderError::invalid(
                        "camera",
                        "perspective needs 0 < near < far",
                    ));
                }
            }
            Projection::Orthographic { height, near, far } => {
                if !(height.is_finite() && height > 0.0) {
                    return Err(RenderError::invalid("camera", "height must be positive"));
                }
                if !(near.is_finite() && far.is_finite() && far > near) {
                    return Err(RenderError::invalid("camera", "needs near < far"));
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::Vec4;

    #[test]
    fn point_in_front_has_depth_in_unit_range() {
        let clip = Camera::default().view_projection(16.0 / 9.0) * Vec4::new(0.0, 0.0, -5.0, 1.0);
        let ndc_z = clip.z / clip.w;
        assert!(clip.w > 0.0 && ndc_z > 0.0 && ndc_z < 1.0);
    }

    #[test]
    fn point_behind_has_negative_w() {
        let clip = Camera::default().view_projection(1.0) * Vec4::new(0.0, 0.0, 5.0, 1.0);
        assert!(clip.w < 0.0);
    }

    #[test]
    fn rejects_invalid_projection() {
        let bad_perspective = Camera {
            projection: Projection::Perspective {
                fov_y_radians: 1.0,
                near: 0.0,
                far: 10.0,
            },
            ..Camera::default()
        };
        assert!(bad_perspective.validate().is_err());
        let bad_ortho = Camera {
            projection: Projection::Orthographic {
                height: -1.0,
                near: 0.1,
                far: 10.0,
            },
            ..Camera::default()
        };
        assert!(bad_ortho.validate().is_err());
        assert!(Camera::default().validate().is_ok());
    }
}
