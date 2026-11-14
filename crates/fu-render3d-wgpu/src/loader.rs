use fu_render3d::material::{Material, TextureData};
use fu_render3d::mesh::MeshData;
use fu_render3d::resources::{Handle, Store};
use fu_render3d::shader::ShaderId;
use fu_render3d::{RenderError, Result};
use std::path::Path;

#[derive(Debug)]
pub struct ImportedNode {
    pub name: String,
    pub mesh: Option<Handle<MeshData>>,
    pub material: Option<Handle<Material>>,
    pub transform: glam::Mat4,
    pub children: Vec<ImportedNode>,
}

#[derive(Debug)]
pub struct ImportedScene {
    pub meshes: Vec<Handle<MeshData>>,
    pub materials: Vec<Handle<Material>>,
    pub textures: Vec<Handle<TextureData>>,
    pub root_nodes: Vec<ImportedNode>,
}

pub struct GltfImporter;

impl GltfImporter {
    pub fn import_file(
        path: impl AsRef<Path>,
        meshes: &mut Store<MeshData>,
        materials: &mut Store<Material>,
        textures: &mut Store<TextureData>,
    ) -> Result<ImportedScene> {
        let (document, buffers, images) =
            gltf::import(path.as_ref()).map_err(|e| RenderError::Invalid {
                what: "gltf import",
                reason: e.to_string(),
            })?;

        // 1. Import Textures/Images
        let mut loaded_textures = Vec::new();
        for (i, img) in images.into_iter().enumerate() {
            let rgba8 = match img.format {
                gltf::image::Format::R8G8B8A8 => img.pixels,
                gltf::image::Format::R8G8B8 => {
                    let mut rgba = Vec::with_capacity(img.pixels.len() / 3 * 4);
                    for chunk in img.pixels.chunks(3) {
                        rgba.extend_from_slice(&[chunk[0], chunk[1], chunk[2], 255]);
                    }
                    rgba
                }
                _ => {
                    // Fallback 1x1 white
                    vec![255, 255, 255, 255]
                }
            };
            let tex_data = TextureData::new(format!("texture_{i}"), img.width, img.height, rgba8)?;
            let handle = textures.insert(tex_data);
            loaded_textures.push(handle);
        }

        // 2. Import Materials
        let mut loaded_materials = Vec::new();
        for mat in document.materials() {
            let pbr = mat.pbr_metallic_roughness();
            let base_color = pbr.base_color_factor();
            let metallic = pbr.metallic_factor();
            let roughness = pbr.roughness_factor();
            let emissive = mat.emissive_factor();

            let tex_handle = pbr.base_color_texture().and_then(|info| {
                let idx = info.texture().source().index();
                loaded_textures.get(idx).copied()
            });

            let shader = ShaderId::lit();

            let mut m = Material::new(mat.name().unwrap_or("material"), shader);
            m.base_color = base_color;
            m.metallic = metallic;
            m.roughness = roughness;
            m.emissive = emissive;
            m.base_color_texture = tex_handle;

            let alpha_mode = match mat.alpha_mode() {
                gltf::material::AlphaMode::Opaque => fu_render3d::material::AlphaMode::Opaque,
                gltf::material::AlphaMode::Mask => fu_render3d::material::AlphaMode::Mask {
                    cutoff: mat.alpha_cutoff().unwrap_or(0.5),
                },
                gltf::material::AlphaMode::Blend => fu_render3d::material::AlphaMode::Blend,
            };
            m.alpha_mode = alpha_mode;

            let handle = materials.insert(m);
            loaded_materials.push(handle);
        }

        // 3. Import Meshes
        let mut loaded_meshes = Vec::new();
        for mesh in document.meshes() {
            for primitive in mesh.primitives() {
                let reader = primitive.reader(|b| Some(&buffers[b.index()]));

                let positions: Vec<[f32; 3]> = reader
                    .read_positions()
                    .map(|iter| iter.collect())
                    .unwrap_or_default();

                let normals: Vec<[f32; 3]> = reader
                    .read_normals()
                    .map(|iter| iter.collect())
                    .unwrap_or_else(|| vec![[0.0, 1.0, 0.0]; positions.len()]);

                let uvs: Vec<[f32; 2]> = reader
                    .read_tex_coords(0)
                    .map(|iter| iter.into_f32().collect())
                    .unwrap_or_else(|| vec![[0.0, 0.0]; positions.len()]);

                let indices: Vec<u32> = reader
                    .read_indices()
                    .map(|iter| iter.into_u32().collect())
                    .unwrap_or_default();

                let mut vertices = Vec::with_capacity(positions.len());
                for i in 0..positions.len() {
                    vertices.push(fu_render3d::mesh::Vertex {
                        position: positions[i],
                        normal: normals[i],
                        uv: uvs[i],
                    });
                }

                if let Ok(mesh_data) =
                    MeshData::new(mesh.name().unwrap_or("mesh"), vertices, indices)
                {
                    let handle = meshes.insert(mesh_data);
                    loaded_meshes.push(handle);
                }
            }
        }

        Ok(ImportedScene {
            meshes: loaded_meshes,
            materials: loaded_materials,
            textures: loaded_textures,
            root_nodes: Vec::new(),
        })
    }
}
