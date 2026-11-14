//! Visibility culling and render ordering.
//!
//! Opaque items are grouped by pipeline key (shader, blend, culling) to limit
//! render-state changes, then ordered front-to-back inside a group for early-z.
//! Blended items are ordered strictly back-to-front.

use crate::camera::Camera;
use crate::material::Material;
use crate::mesh::{Aabb, MeshData};
use crate::resources::{Handle, Store};
use crate::scene::{NodeId, Scene};
use crate::shader::{ShaderId, ShaderRegistry};
use crate::{RenderError, Result};
use glam::{Mat4, Vec3, Vec4};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PipelineKey {
    pub shader: ShaderId,
    pub blended: bool,
    pub double_sided: bool,
}

#[derive(Debug, Clone)]
pub struct DrawItem {
    pub node: NodeId,
    pub mesh: Handle<MeshData>,
    pub material: Handle<Material>,
    pub world: Mat4,
    /// Distance along the camera's forward axis to the world-space bounds centre.
    pub view_depth: f32,
    pub key: PipelineKey,
}

#[derive(Debug, Default, Clone)]
pub struct DrawQueue {
    pub opaque: Vec<DrawItem>,
    pub transparent: Vec<DrawItem>,
}

impl DrawQueue {
    pub fn len(&self) -> usize {
        self.opaque.len() + self.transparent.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Six clip planes (left, right, bottom, top, near, far) for a 0..1 depth range.
#[derive(Debug, Clone, Copy)]
pub struct Frustum {
    planes: [Vec4; 6],
}

impl Frustum {
    pub fn from_view_projection(vp: &Mat4) -> Self {
        let (r0, r1, r2, r3) = (vp.row(0), vp.row(1), vp.row(2), vp.row(3));
        let mut planes = [r3 + r0, r3 - r0, r3 + r1, r3 - r1, r2, r3 - r2];
        for plane in &mut planes {
            let length = plane.truncate().length();
            if length > 0.0 {
                *plane /= length;
            }
        }
        Self { planes }
    }

    pub fn intersects_aabb(&self, aabb: &Aabb) -> bool {
        for plane in &self.planes {
            let n = plane.truncate();
            let farthest = Vec3::new(
                if n.x >= 0.0 { aabb.max.x } else { aabb.min.x },
                if n.y >= 0.0 { aabb.max.y } else { aabb.min.y },
                if n.z >= 0.0 { aabb.max.z } else { aabb.min.z },
            );
            if n.dot(farthest) + plane.w < 0.0 {
                return false;
            }
        }
        true
    }
}

pub fn build_queue(
    scene: &Scene,
    meshes: &Store<MeshData>,
    materials: &Store<Material>,
    shaders: &ShaderRegistry,
    camera: &Camera,
    aspect: f32,
    layer_mask: u32,
) -> Result<DrawQueue> {
    camera.validate()?;
    if !(aspect.is_finite() && aspect > 0.0) {
        return Err(RenderError::invalid("aspect", "must be finite and > 0"));
    }
    let view = camera.view_matrix();
    let frustum = Frustum::from_view_projection(&(camera.projection_matrix(aspect) * view));
    let mut queue = DrawQueue::default();

    for (id, node) in scene.iter() {
        if !node.visible || node.layer_mask & layer_mask == 0 {
            continue;
        }
        let (Some(mesh_handle), Some(material_handle)) = (node.mesh, node.material) else {
            continue;
        };
        let mesh = meshes
            .get(mesh_handle)
            .ok_or_else(|| RenderError::NotFound(format!("mesh for node '{}'", node.name)))?;
        let material = materials
            .get(material_handle)
            .ok_or_else(|| RenderError::NotFound(format!("material for node '{}'", node.name)))?;
        if !shaders.contains(&material.shader) {
            return Err(RenderError::NotFound(format!(
                "shader '{}' used by node '{}'",
                material.shader.as_str(),
                node.name
            )));
        }

        let world = scene.world_matrix(id)?;
        let bounds = mesh.bounds();
        if !frustum.intersects_aabb(&bounds.transformed(&world)) {
            continue;
        }
        let center = view.transform_point3(world.transform_point3(bounds.center()));
        let item = DrawItem {
            node: id,
            mesh: mesh_handle,
            material: material_handle,
            world,
            view_depth: -center.z,
            key: PipelineKey {
                shader: material.shader.clone(),
                blended: material.is_blended(),
                double_sided: material.double_sided,
            },
        };
        if item.key.blended {
            queue.transparent.push(item);
        } else {
            queue.opaque.push(item);
        }
    }

    queue.opaque.sort_by(|a, b| {
        a.key
            .cmp(&b.key)
            .then(a.view_depth.total_cmp(&b.view_depth))
    });
    queue
        .transparent
        .sort_by(|a, b| b.view_depth.total_cmp(&a.view_depth));
    Ok(queue)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::material::AlphaMode;
    use crate::scene::Node;
    use crate::transform::Transform;

    struct Fixture {
        scene: Scene,
        meshes: Store<MeshData>,
        materials: Store<Material>,
        shaders: ShaderRegistry,
    }

    fn fixture() -> Fixture {
        let mut shaders = ShaderRegistry::default();
        shaders.register(ShaderId::lit(), "// lit").unwrap();
        shaders.register(ShaderId::unlit(), "// unlit").unwrap();
        Fixture {
            scene: Scene::default(),
            meshes: Store::default(),
            materials: Store::default(),
            shaders,
        }
    }

    fn add(f: &mut Fixture, z: f32, material: Material) -> NodeId {
        let mesh = f.meshes.insert(MeshData::cuboid(Vec3::ONE));
        let material = f.materials.insert(material);
        let mut node = Node::new(format!("z{z}"));
        node.transform = Transform::from_translation(Vec3::new(0.0, 0.0, z));
        node.mesh = Some(mesh);
        node.material = Some(material);
        f.scene.add_node(node)
    }

    fn build(f: &Fixture) -> DrawQueue {
        build_queue(
            &f.scene,
            &f.meshes,
            &f.materials,
            &f.shaders,
            &Camera::default(),
            16.0 / 9.0,
            u32::MAX,
        )
        .unwrap()
    }

    #[test]
    fn culls_objects_behind_and_beyond_far_plane() {
        let mut f = fixture();
        let visible = add(&mut f, -5.0, Material::new("m", ShaderId::lit()));
        add(&mut f, 5.0, Material::new("m", ShaderId::lit()));
        add(&mut f, -5000.0, Material::new("m", ShaderId::lit()));
        let queue = build(&f);
        assert_eq!(queue.len(), 1);
        assert_eq!(queue.opaque[0].node, visible);
    }

    #[test]
    fn opaque_groups_by_shader_then_sorts_front_to_back() {
        let mut f = fixture();
        add(&mut f, -20.0, Material::new("lit-far", ShaderId::lit()));
        add(&mut f, -5.0, Material::new("lit-near", ShaderId::lit()));
        add(&mut f, -10.0, Material::new("unlit", ShaderId::unlit()));
        let queue = build(&f);
        let keys: Vec<&str> = queue.opaque.iter().map(|i| i.key.shader.as_str()).collect();
        assert_eq!(keys, vec!["builtin/lit", "builtin/lit", "builtin/unlit"]);
        assert!(queue.opaque[0].view_depth < queue.opaque[1].view_depth);
    }

    #[test]
    fn blended_items_sort_back_to_front() {
        let mut f = fixture();
        let mut glass = Material::new("glass", ShaderId::lit());
        glass.alpha_mode = AlphaMode::Blend;
        add(&mut f, -5.0, glass.clone());
        add(&mut f, -15.0, glass);
        let queue = build(&f);
        assert!(queue.opaque.is_empty());
        assert!(queue.transparent[0].view_depth > queue.transparent[1].view_depth);
    }

    #[test]
    fn hidden_nodes_and_masked_layers_are_skipped() {
        let mut f = fixture();
        let a = add(&mut f, -5.0, Material::new("m", ShaderId::lit()));
        let b = add(&mut f, -6.0, Material::new("m", ShaderId::lit()));
        f.scene.node_mut(a).unwrap().visible = false;
        f.scene.node_mut(b).unwrap().layer_mask = 2;
        let queue = build_queue(
            &f.scene,
            &f.meshes,
            &f.materials,
            &f.shaders,
            &Camera::default(),
            1.0,
            1,
        )
        .unwrap();
        assert!(queue.is_empty());
    }

    #[test]
    fn unregistered_shader_is_an_error() {
        let mut f = fixture();
        add(
            &mut f,
            -5.0,
            Material::new("m", ShaderId::new("custom/missing")),
        );
        let result = build_queue(
            &f.scene,
            &f.meshes,
            &f.materials,
            &f.shaders,
            &Camera::default(),
            1.0,
            u32::MAX,
        );
        assert!(matches!(result, Err(RenderError::NotFound(_))));
    }
}
