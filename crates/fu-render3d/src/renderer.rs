use crate::backend::{Composition, FramePacket, RenderBackend, Viewport};
use crate::camera::Camera;
use crate::material::{Material, TextureData};
use crate::mesh::MeshData;
use crate::quality::QualitySettings;
use crate::queue::build_queue;
use crate::resources::Store;
use crate::scene::Scene;
use crate::shader::ShaderRegistry;
use crate::{RenderError, Result};

/// Owns all CPU-side 3D state and turns it into a `FramePacket` for a backend.
pub struct Renderer3D {
    pub scene: Scene,
    pub meshes: Store<MeshData>,
    pub materials: Store<Material>,
    pub textures: Store<TextureData>,
    pub shaders: ShaderRegistry,
    pub composition: Composition,
    /// Nodes render only if `node.layer_mask & layer_mask != 0`.
    pub layer_mask: u32,
    quality: QualitySettings,
}

impl Renderer3D {
    pub fn new(quality: QualitySettings) -> Result<Self> {
        quality.validate()?;
        Ok(Self {
            scene: Scene::default(),
            meshes: Store::default(),
            materials: Store::default(),
            textures: Store::default(),
            shaders: ShaderRegistry::default(),
            composition: Composition::default(),
            layer_mask: u32::MAX,
            quality,
        })
    }

    pub fn quality(&self) -> &QualitySettings {
        &self.quality
    }

    pub fn set_quality(&mut self, quality: QualitySettings) -> Result<()> {
        quality.validate()?;
        self.quality = quality;
        Ok(())
    }

    /// Renders the whole target with the renderer's own layer mask. The
    /// projection's aspect ratio is the target's.
    pub fn render(
        &self,
        camera: &Camera,
        viewport: Viewport,
        backend: &mut dyn RenderBackend,
    ) -> Result<()> {
        self.render_masked(
            camera,
            viewport,
            viewport.aspect(),
            self.layer_mask,
            backend,
        )
    }

    /// Renders one pass of the scene.
    ///
    /// `viewport` is the size of the target the backend draws into. `aspect` is
    /// the aspect ratio the projection is built for, which differs from the
    /// target's when the backend draws into a sub-rectangle of it (a letterboxed
    /// 16:9 area of a window, for example). `layer_mask` selects the nodes to
    /// draw: a node is drawn only if `node.layer_mask & layer_mask != 0`. This
    /// does not read or change [`Renderer3D::layer_mask`], so several passes with
    /// different cameras and masks can be issued from a shared reference.
    pub fn render_masked(
        &self,
        camera: &Camera,
        viewport: Viewport,
        aspect: f32,
        layer_mask: u32,
        backend: &mut dyn RenderBackend,
    ) -> Result<()> {
        if viewport.width == 0 || viewport.height == 0 {
            return Err(RenderError::invalid(
                "viewport",
                "width and height must be > 0",
            ));
        }
        if !aspect.is_finite() || aspect <= 0.0 {
            return Err(RenderError::invalid(
                "aspect",
                "aspect ratio must be finite and > 0",
            ));
        }
        let quality = backend.capabilities().clamp_quality(&self.quality);
        let queue = build_queue(
            &self.scene,
            &self.meshes,
            &self.materials,
            &self.shaders,
            camera,
            aspect,
            layer_mask,
        )?;
        let packet = FramePacket {
            viewport,
            composition: self.composition,
            view: camera.view_matrix(),
            projection: camera.projection_matrix(aspect),
            camera_position: camera.position(),
            quality: &quality,
            queue: &queue,
            lights: self
                .scene
                .lights
                .select(quality.max_lights, camera.position()),
            ambient_color: self.scene.lights.ambient_color,
            ambient_intensity: self.scene.lights.ambient_intensity,
            meshes: &self.meshes,
            materials: &self.materials,
            textures: &self.textures,
            shaders: &self.shaders,
        };
        backend.render(&packet)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::BackendCapabilities;
    use crate::light::Light;
    use crate::quality::{Platform, QualityLevel};
    use crate::scene::Node;
    use crate::shader::ShaderId;
    use crate::transform::Transform;
    use glam::Vec3;

    #[derive(Default)]
    struct Recorder {
        frames: usize,
        opaque: usize,
        lights: usize,
        shadow_size: Option<u32>,
        projection: glam::Mat4,
    }

    impl RenderBackend for Recorder {
        fn name(&self) -> &str {
            "recorder"
        }
        fn capabilities(&self) -> BackendCapabilities {
            BackendCapabilities {
                max_texture_size: 4096,
                max_msaa_samples: 4,
                supports_shadows: false,
            }
        }
        fn render(&mut self, frame: &FramePacket<'_>) -> Result<()> {
            self.frames += 1;
            self.opaque = frame.queue.opaque.len();
            self.lights = frame.lights.len();
            self.shadow_size = frame.quality.shadow_map_size;
            self.projection = frame.projection;
            Ok(())
        }
    }

    fn renderer_with_cube(z: f32) -> Renderer3D {
        let mut r = Renderer3D::new(QualitySettings::preset(
            QualityLevel::Medium,
            Platform::Desktop,
        ))
        .unwrap();
        r.shaders.register(ShaderId::lit(), "// lit").unwrap();
        let mesh = r.meshes.insert(MeshData::cuboid(Vec3::ONE));
        let material = r.materials.insert(Material::new("m", ShaderId::lit()));
        let mut node = Node::new("cube");
        node.transform = Transform::from_translation(Vec3::new(0.0, 0.0, z));
        node.mesh = Some(mesh);
        node.material = Some(material);
        r.scene.add_node(node);
        r
    }

    #[test]
    fn renders_visible_node_and_clamps_quality_to_backend() {
        let mut r = renderer_with_cube(-5.0);
        r.scene
            .lights
            .lights
            .push(Light::directional([1.0; 3], 1.0, Transform::IDENTITY));
        let mut backend = Recorder::default();
        r.render(
            &Camera::default(),
            Viewport {
                width: 1280,
                height: 720,
            },
            &mut backend,
        )
        .unwrap();
        assert_eq!(backend.frames, 1);
        assert_eq!(backend.opaque, 1);
        assert_eq!(backend.lights, 1);
        assert_eq!(backend.shadow_size, None);
        // The renderer's own settings are untouched by the backend clamp.
        assert!(r.quality().shadow_map_size.is_some());
    }

    #[test]
    fn node_behind_camera_is_not_submitted() {
        let r = renderer_with_cube(5.0);
        let mut backend = Recorder::default();
        r.render(
            &Camera::default(),
            Viewport {
                width: 640,
                height: 360,
            },
            &mut backend,
        )
        .unwrap();
        assert_eq!(backend.opaque, 0);
    }

    #[test]
    fn zero_sized_viewport_is_rejected() {
        let r = renderer_with_cube(-5.0);
        let mut backend = Recorder::default();
        let result = r.render(
            &Camera::default(),
            Viewport {
                width: 0,
                height: 10,
            },
            &mut backend,
        );
        assert!(result.is_err());
        assert_eq!(backend.frames, 0);
    }

    #[test]
    fn invalid_quality_is_rejected() {
        let mut q = QualitySettings::preset(QualityLevel::Low, Platform::Desktop);
        q.msaa_samples = 3;
        assert!(Renderer3D::new(q).is_err());
    }

    #[test]
    fn a_pass_draws_only_the_layers_it_asks_for() {
        let r = renderer_with_cube(-5.0);
        let viewport = Viewport {
            width: 1280,
            height: 720,
        };
        let camera = Camera::default();

        // The cube is on layer bit 0 (the default).
        let mut backend = Recorder::default();
        r.render_masked(&camera, viewport, 16.0 / 9.0, 0b01, &mut backend)
            .unwrap();
        assert_eq!(backend.opaque, 1);

        let mut backend = Recorder::default();
        r.render_masked(&camera, viewport, 16.0 / 9.0, 0b10, &mut backend)
            .unwrap();
        assert_eq!(backend.frames, 1, "the pass still runs");
        assert_eq!(backend.opaque, 0, "but the cube is on another layer");

        // The renderer's own mask is untouched, so `render` still sees everything.
        assert_eq!(r.layer_mask, u32::MAX);
        let mut backend = Recorder::default();
        r.render(&camera, viewport, &mut backend).unwrap();
        assert_eq!(backend.opaque, 1);
    }

    #[test]
    fn the_projection_uses_the_requested_aspect_not_the_targets() {
        let r = renderer_with_cube(-5.0);
        // The target is 2:1, but the pass is built for 1:1 and for 2:1.
        let viewport = Viewport {
            width: 1000,
            height: 500,
        };
        let camera = Camera::default();

        let mut square = Recorder::default();
        r.render_masked(&camera, viewport, 1.0, u32::MAX, &mut square)
            .unwrap();
        let mut wide = Recorder::default();
        r.render_masked(&camera, viewport, 2.0, u32::MAX, &mut wide)
            .unwrap();

        // Horizontal scale is inversely proportional to the aspect ratio and the
        // vertical scale does not depend on it.
        let ratio = square.projection.x_axis.x / wide.projection.x_axis.x;
        assert!((ratio - 2.0).abs() < 1e-4, "ratio was {ratio}");
        assert!((square.projection.y_axis.y - wide.projection.y_axis.y).abs() < 1e-6);

        // `render` uses the target's own aspect (2:1).
        let mut plain = Recorder::default();
        r.render(&camera, viewport, &mut plain).unwrap();
        assert!((plain.projection.x_axis.x - wide.projection.x_axis.x).abs() < 1e-6);
    }

    #[test]
    fn an_unusable_aspect_is_rejected() {
        let r = renderer_with_cube(-5.0);
        let viewport = Viewport {
            width: 640,
            height: 360,
        };
        for aspect in [0.0, -1.0, f32::NAN, f32::INFINITY] {
            let mut backend = Recorder::default();
            let result = r.render_masked(
                &Camera::default(),
                viewport,
                aspect,
                u32::MAX,
                &mut backend,
            );
            assert!(result.is_err(), "aspect {aspect} must be refused");
            assert_eq!(backend.frames, 0);
        }
    }
}
