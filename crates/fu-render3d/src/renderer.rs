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

    pub fn render(
        &self,
        camera: &Camera,
        viewport: Viewport,
        backend: &mut dyn RenderBackend,
    ) -> Result<()> {
        if viewport.width == 0 || viewport.height == 0 {
            return Err(RenderError::invalid(
                "viewport",
                "width and height must be > 0",
            ));
        }
        let quality = backend.capabilities().clamp_quality(&self.quality);
        let aspect = viewport.aspect();
        let queue = build_queue(
            &self.scene,
            &self.meshes,
            &self.materials,
            &self.shaders,
            camera,
            aspect,
            self.layer_mask,
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
}
