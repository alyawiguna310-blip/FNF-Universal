use crate::light::LightEnvironment;
use crate::material::Material;
use crate::mesh::MeshData;
use crate::resources::{Handle, Store};
use crate::transform::Transform;
use crate::{RenderError, Result};
use glam::Mat4;

pub type NodeId = Handle<Node>;

const MAX_HIERARCHY_DEPTH: usize = 256;

#[derive(Debug, Clone)]
pub struct Node {
    pub name: String,
    pub transform: Transform,
    pub parent: Option<NodeId>,
    pub mesh: Option<Handle<MeshData>>,
    pub material: Option<Handle<Material>>,
    pub visible: bool,
    /// Bitmask matched against the renderer's layer mask.
    pub layer_mask: u32,
}

impl Node {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            transform: Transform::IDENTITY,
            parent: None,
            mesh: None,
            material: None,
            visible: true,
            layer_mask: 1,
        }
    }
}

#[derive(Debug, Default)]
pub struct Scene {
    nodes: Store<Node>,
    pub lights: LightEnvironment,
}

impl Scene {
    pub fn add_node(&mut self, node: Node) -> NodeId {
        self.nodes.insert(node)
    }

    pub fn remove_node(&mut self, id: NodeId) -> Option<Node> {
        self.nodes.remove(id)
    }

    pub fn node(&self, id: NodeId) -> Option<&Node> {
        self.nodes.get(id)
    }

    pub fn node_mut(&mut self, id: NodeId) -> Option<&mut Node> {
        self.nodes.get_mut(id)
    }

    pub fn iter(&self) -> impl Iterator<Item = (NodeId, &Node)> {
        self.nodes.iter()
    }

    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// World matrix = parent chain composed root-first. Fails on dangling
    /// parents and on cyclic or excessively deep hierarchies.
    pub fn world_matrix(&self, id: NodeId) -> Result<Mat4> {
        let mut matrix = Mat4::IDENTITY;
        let mut current = Some(id);
        let mut depth = 0;
        while let Some(node_id) = current {
            if depth > MAX_HIERARCHY_DEPTH {
                return Err(RenderError::invalid(
                    "node hierarchy",
                    "parent chain is cyclic or too deep",
                ));
            }
            let node = self
                .nodes
                .get(node_id)
                .ok_or_else(|| RenderError::NotFound(format!("node {node_id:?}")))?;
            matrix = node.transform.matrix() * matrix;
            current = node.parent;
            depth += 1;
        }
        Ok(matrix)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::Vec3;

    #[test]
    fn child_inherits_parent_translation() {
        let mut scene = Scene::default();
        let mut parent = Node::new("p");
        parent.transform = Transform::from_translation(Vec3::new(10.0, 0.0, 0.0));
        let parent_id = scene.add_node(parent);
        let mut child = Node::new("c");
        child.parent = Some(parent_id);
        child.transform = Transform::from_translation(Vec3::new(0.0, 2.0, 0.0));
        let child_id = scene.add_node(child);
        let origin = scene
            .world_matrix(child_id)
            .unwrap()
            .transform_point3(Vec3::ZERO);
        assert_eq!(origin, Vec3::new(10.0, 2.0, 0.0));
    }

    #[test]
    fn cycles_and_dangling_parents_are_errors() {
        let mut scene = Scene::default();
        let a = scene.add_node(Node::new("a"));
        let b = scene.add_node(Node::new("b"));
        scene.node_mut(a).unwrap().parent = Some(b);
        scene.node_mut(b).unwrap().parent = Some(a);
        assert!(scene.world_matrix(a).is_err());

        let mut scene = Scene::default();
        let parent = scene.add_node(Node::new("p"));
        let mut child = Node::new("c");
        child.parent = Some(parent);
        let child_id = scene.add_node(child);
        scene.remove_node(parent);
        assert!(matches!(
            scene.world_matrix(child_id),
            Err(RenderError::NotFound(_))
        ));
    }
}
