pub mod backend;
#[cfg(feature = "gltf-import")]
pub mod loader;
pub mod surface;
pub mod types;

#[cfg(test)]
mod tests_surface;

#[cfg(test)]
pub(crate) mod test_gpu;

pub use backend::{register_builtin_shaders, WgpuBackend, LIT_WGSL, UNLIT_WGSL};
#[cfg(feature = "gltf-import")]
pub use loader::GltfImporter;
pub use surface::WindowedRenderer;

#[cfg(test)]
mod tests {
    use super::*;
    use fu_render3d::backend::{RenderBackend, Viewport};
    use fu_render3d::camera::Camera;
    use fu_render3d::material::Material;
    use fu_render3d::mesh::MeshData;
    use fu_render3d::quality::{Platform, QualityLevel, QualitySettings};
    use fu_render3d::renderer::Renderer3D;
    #[cfg(feature = "gltf-import")]
    use fu_render3d::resources::Store;
    use fu_render3d::scene::Node;
    use fu_render3d::shader::ShaderId;
    use fu_render3d::transform::Transform;
    use glam::Vec3;

    #[test]
    fn test_headless_wgpu_backend_creation() {
        let _gpu = crate::test_gpu::gpu_lock();
        let backend = WgpuBackend::new_headless();
        assert!(
            backend.is_ok(),
            "Failed to create headless wgpu backend: {:?}",
            backend.err()
        );
        let backend = backend.unwrap();
        assert_eq!(backend.name(), "wgpu");
        assert!(backend.capabilities().max_texture_size > 0);
    }

    #[test]
    fn test_render_plane_headless() {
        let _gpu = crate::test_gpu::gpu_lock();
        let mut backend = match WgpuBackend::new_headless() {
            Ok(b) => b,
            Err(e) => {
                eprintln!("Skipping test on system with no GPU adapter: {e}");
                return;
            }
        };

        let quality = QualitySettings::preset(QualityLevel::Medium, Platform::Desktop);
        let mut renderer = Renderer3D::new(quality).expect("renderer creation failed");
        register_builtin_shaders(&mut renderer.shaders)
            .expect("builtin shaders registration failed");

        let mesh = renderer.meshes.insert(MeshData::plane(10.0, 10.0));

        let mut mat = Material::new("mat_unlit", ShaderId::unlit());
        mat.base_color = [1.0, 0.0, 0.0, 1.0];
        let mat_handle = renderer.materials.insert(mat);

        let mut node = Node::new("floor_node");
        node.mesh = Some(mesh);
        node.material = Some(mat_handle);
        renderer.scene.add_node(node);

        let camera = Camera {
            transform: Transform::from_translation(Vec3::new(0.0, 2.0, 5.0)),
            ..Default::default()
        };
        let viewport = Viewport {
            width: 800,
            height: 600,
        };

        let res = renderer.render(&camera, viewport, &mut backend);

        assert!(res.is_ok(), "Rendering failed: {:?}", res.err());
    }

    #[test]
    fn test_render_lit_scene_headless() {
        let _gpu = crate::test_gpu::gpu_lock();
        let mut backend = match WgpuBackend::new_headless() {
            Ok(b) => b,
            Err(e) => {
                eprintln!("Skipping test on system with no GPU adapter: {e}");
                return;
            }
        };

        let quality = QualitySettings::preset(QualityLevel::High, Platform::Desktop);
        let mut renderer = Renderer3D::new(quality).expect("renderer creation failed");
        register_builtin_shaders(&mut renderer.shaders)
            .expect("builtin shaders registration failed");

        let mesh = renderer.meshes.insert(MeshData::cuboid(Vec3::ONE));

        let mut mat = Material::new("mat_lit", ShaderId::lit());
        mat.base_color = [0.2, 0.8, 0.2, 1.0];
        mat.metallic = 0.5;
        mat.roughness = 0.3;
        let mat_handle = renderer.materials.insert(mat);

        let mut node = Node::new("cube_node");
        node.mesh = Some(mesh);
        node.material = Some(mat_handle);
        renderer.scene.add_node(node);

        renderer.scene.lights.ambient_color = [0.1, 0.1, 0.2];
        renderer.scene.lights.ambient_intensity = 0.5;
        renderer
            .scene
            .lights
            .lights
            .push(fu_render3d::light::Light::directional(
                [1.0, 1.0, 0.9],
                1.2,
                Transform::look_at(Vec3::new(5.0, 10.0, 5.0), Vec3::ZERO, Vec3::Y),
            ));

        let camera = Camera {
            transform: Transform::look_at(Vec3::new(3.0, 3.0, 3.0), Vec3::ZERO, Vec3::Y),
            ..Default::default()
        };
        let viewport = Viewport {
            width: 1280,
            height: 720,
        };

        let res = renderer.render(&camera, viewport, &mut backend);

        assert!(res.is_ok(), "Lit rendering failed: {:?}", res.err());
    }

    #[cfg(feature = "gltf-import")]
    #[test]
    fn test_gltf_importer_mock_data() {
        // Verify GltfImporter on a minimal glTF JSON representation in memory
        let gltf_json = r#"{
            "asset": { "version": "2.0" },
            "materials": [
                {
                    "name": "red_pbr",
                    "pbrMetallicRoughness": {
                        "baseColorFactor": [1.0, 0.0, 0.0, 1.0],
                        "metallicFactor": 0.5,
                        "roughnessFactor": 0.2
                    }
                }
            ],
            "meshes": []
        }"#;

        let temp_dir = std::env::temp_dir();
        let test_gltf_path = temp_dir.join("test_model.gltf");
        std::fs::write(&test_gltf_path, gltf_json).expect("failed to write test gltf");

        let mut meshes = Store::default();
        let mut materials = Store::default();
        let mut textures = Store::default();

        let imported =
            GltfImporter::import_file(&test_gltf_path, &mut meshes, &mut materials, &mut textures);
        let _ = std::fs::remove_file(test_gltf_path);

        assert!(imported.is_ok(), "gltf import failed: {:?}", imported.err());
        let imported = imported.unwrap();
        assert_eq!(imported.materials.len(), 1);
        let mat = materials
            .get(imported.materials[0])
            .expect("material should exist");
        assert_eq!(mat.name, "red_pbr");
        assert_eq!(mat.base_color, [1.0, 0.0, 0.0, 1.0]);
        assert_eq!(mat.metallic, 0.5);
        assert_eq!(mat.roughness, 0.2);
    }
}
