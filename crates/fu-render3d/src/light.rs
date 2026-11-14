use crate::transform::Transform;
use crate::{RenderError, Result};
use glam::Vec3;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LightKind {
    /// Direction is the transform's local -Z axis.
    Directional,
    Point {
        range: f32,
    },
    Spot {
        range: f32,
        inner_cone: f32,
        outer_cone: f32,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Light {
    pub kind: LightKind,
    pub color: [f32; 3],
    pub intensity: f32,
    pub transform: Transform,
    pub enabled: bool,
}

impl Light {
    pub fn directional(color: [f32; 3], intensity: f32, transform: Transform) -> Self {
        Self {
            kind: LightKind::Directional,
            color,
            intensity,
            transform,
            enabled: true,
        }
    }

    pub fn point(color: [f32; 3], intensity: f32, range: f32, position: Vec3) -> Self {
        Self {
            kind: LightKind::Point { range },
            color,
            intensity,
            transform: Transform::from_translation(position),
            enabled: true,
        }
    }

    pub fn validate(&self) -> Result<()> {
        if !self.transform.is_finite()
            || !self.color.iter().all(|c| c.is_finite() && *c >= 0.0)
            || !(self.intensity.is_finite() && self.intensity >= 0.0)
        {
            return Err(RenderError::invalid(
                "light",
                "color/intensity/transform invalid",
            ));
        }
        match self.kind {
            LightKind::Directional => {}
            LightKind::Point { range } => {
                if !(range.is_finite() && range > 0.0) {
                    return Err(RenderError::invalid("light", "range must be > 0"));
                }
            }
            LightKind::Spot {
                range,
                inner_cone,
                outer_cone,
            } => {
                let half_pi = std::f32::consts::FRAC_PI_2;
                if !(range.is_finite()
                    && range > 0.0
                    && inner_cone.is_finite()
                    && outer_cone.is_finite()
                    && inner_cone > 0.0
                    && inner_cone <= outer_cone
                    && outer_cone <= half_pi)
                {
                    return Err(RenderError::invalid(
                        "light",
                        "spot needs range > 0 and 0 < inner <= outer <= pi/2",
                    ));
                }
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct LightEnvironment {
    pub ambient_color: [f32; 3],
    pub ambient_intensity: f32,
    pub lights: Vec<Light>,
}

impl Default for LightEnvironment {
    fn default() -> Self {
        Self {
            ambient_color: [1.0; 3],
            ambient_intensity: 0.1,
            lights: Vec::new(),
        }
    }
}

impl LightEnvironment {
    /// Enabled lights capped at `max`: directional lights first (scene order),
    /// then local lights nearest to `reference`.
    pub fn select(&self, max: usize, reference: Vec3) -> Vec<&Light> {
        let mut directional = Vec::new();
        let mut local = Vec::new();
        for light in self.lights.iter().filter(|l| l.enabled) {
            match light.kind {
                LightKind::Directional => directional.push(light),
                _ => local.push(light),
            }
        }
        local.sort_by(|a, b| {
            let da = a.transform.translation.distance_squared(reference);
            let db = b.transform.translation.distance_squared(reference);
            da.total_cmp(&db)
        });
        directional.into_iter().chain(local).take(max).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn select_prefers_directional_then_nearest_and_skips_disabled() {
        let mut env = LightEnvironment::default();
        env.lights
            .push(Light::point([1.0; 3], 1.0, 10.0, Vec3::new(50.0, 0.0, 0.0)));
        env.lights
            .push(Light::point([1.0; 3], 1.0, 10.0, Vec3::new(1.0, 0.0, 0.0)));
        let mut off = Light::point([1.0; 3], 1.0, 10.0, Vec3::ZERO);
        off.enabled = false;
        env.lights.push(off);
        env.lights
            .push(Light::directional([1.0; 3], 1.0, Transform::IDENTITY));

        let picked = env.select(2, Vec3::ZERO);
        assert_eq!(picked.len(), 2);
        assert_eq!(picked[0].kind, LightKind::Directional);
        assert_eq!(picked[1].transform.translation.x, 1.0);
    }

    #[test]
    fn validation_rejects_bad_lights() {
        assert!(Light::point([1.0; 3], 1.0, 0.0, Vec3::ZERO)
            .validate()
            .is_err());
        assert!(Light::point([1.0; 3], -1.0, 5.0, Vec3::ZERO)
            .validate()
            .is_err());
        assert!(Light::point([1.0; 3], 1.0, 5.0, Vec3::ZERO)
            .validate()
            .is_ok());
        let spot = Light {
            kind: LightKind::Spot {
                range: 5.0,
                inner_cone: 1.0,
                outer_cone: 0.5,
            },
            ..Light::point([1.0; 3], 1.0, 5.0, Vec3::ZERO)
        };
        assert!(spot.validate().is_err());
    }
}
