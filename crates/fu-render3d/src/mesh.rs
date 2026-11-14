use crate::{RenderError, Result};
use glam::{Mat4, Vec3};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Vertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    pub uv: [f32; 2],
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Aabb {
    pub min: Vec3,
    pub max: Vec3,
}

impl Aabb {
    pub fn center(&self) -> Vec3 {
        (self.min + self.max) * 0.5
    }

    pub fn corners(&self) -> [Vec3; 8] {
        let (a, b) = (self.min, self.max);
        [
            Vec3::new(a.x, a.y, a.z),
            Vec3::new(b.x, a.y, a.z),
            Vec3::new(a.x, b.y, a.z),
            Vec3::new(b.x, b.y, a.z),
            Vec3::new(a.x, a.y, b.z),
            Vec3::new(b.x, a.y, b.z),
            Vec3::new(a.x, b.y, b.z),
            Vec3::new(b.x, b.y, b.z),
        ]
    }

    /// World-space AABB enclosing this box after `matrix`.
    pub fn transformed(&self, matrix: &Mat4) -> Aabb {
        let mut min = Vec3::splat(f32::INFINITY);
        let mut max = Vec3::splat(f32::NEG_INFINITY);
        for corner in self.corners() {
            let p = matrix.transform_point3(corner);
            min = min.min(p);
            max = max.max(p);
        }
        Aabb { min, max }
    }
}

/// Validated CPU mesh. Intended to be filled by procedural builders now and by
/// a glTF/GLB importer later. Bounds are computed once at construction.
#[derive(Debug, Clone, PartialEq)]
pub struct MeshData {
    name: String,
    vertices: Vec<Vertex>,
    indices: Vec<u32>,
    bounds: Aabb,
}

impl MeshData {
    pub fn new(name: impl Into<String>, vertices: Vec<Vertex>, indices: Vec<u32>) -> Result<Self> {
        if vertices.is_empty() {
            return Err(RenderError::invalid("mesh", "no vertices"));
        }
        if indices.is_empty() || !indices.len().is_multiple_of(3) {
            return Err(RenderError::invalid(
                "mesh",
                "index count must be a non-zero multiple of 3",
            ));
        }
        if indices.iter().any(|&i| i as usize >= vertices.len()) {
            return Err(RenderError::invalid("mesh", "index out of range"));
        }
        let mut min = Vec3::splat(f32::INFINITY);
        let mut max = Vec3::splat(f32::NEG_INFINITY);
        for v in &vertices {
            let finite = v.position.iter().all(|c| c.is_finite())
                && v.normal.iter().all(|c| c.is_finite())
                && v.uv.iter().all(|c| c.is_finite());
            if !finite {
                return Err(RenderError::invalid("mesh", "non-finite vertex data"));
            }
            let p = Vec3::from(v.position);
            min = min.min(p);
            max = max.max(p);
        }
        Ok(Self {
            name: name.into(),
            vertices,
            indices,
            bounds: Aabb { min, max },
        })
    }

    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn vertices(&self) -> &[Vertex] {
        &self.vertices
    }
    pub fn indices(&self) -> &[u32] {
        &self.indices
    }
    pub fn bounds(&self) -> Aabb {
        self.bounds
    }

    /// Axis-aligned box centred on the origin, counter-clockwise winding, 24 vertices.
    pub fn cuboid(size: Vec3) -> Self {
        let half = size * 0.5;
        // (normal, u axis, v axis) with u x v == normal.
        let faces = [
            (Vec3::X, Vec3::Y, Vec3::Z),
            (Vec3::NEG_X, Vec3::Z, Vec3::Y),
            (Vec3::Y, Vec3::Z, Vec3::X),
            (Vec3::NEG_Y, Vec3::X, Vec3::Z),
            (Vec3::Z, Vec3::X, Vec3::Y),
            (Vec3::NEG_Z, Vec3::Y, Vec3::X),
        ];
        let mut vertices = Vec::with_capacity(24);
        let mut indices = Vec::with_capacity(36);
        for (normal, u, v) in faces {
            push_face(&mut vertices, &mut indices, normal, u, v, half);
        }
        Self::new("cuboid", vertices, indices).expect("generated cuboid is valid")
    }

    /// Flat rectangle on the XZ plane facing +Y.
    pub fn plane(width: f32, depth: f32) -> Self {
        let half = Vec3::new(width * 0.5, 0.0, depth * 0.5);
        let mut vertices = Vec::with_capacity(4);
        let mut indices = Vec::with_capacity(6);
        push_face(&mut vertices, &mut indices, Vec3::Y, Vec3::Z, Vec3::X, half);
        Self::new("plane", vertices, indices).expect("generated plane is valid")
    }
}

fn push_face(
    vertices: &mut Vec<Vertex>,
    indices: &mut Vec<u32>,
    normal: Vec3,
    u: Vec3,
    v: Vec3,
    half: Vec3,
) {
    let base = vertices.len() as u32;
    let corners = [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)];
    let uvs = [[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]];
    for ((su, sv), uv) in corners.into_iter().zip(uvs) {
        let position = (normal + u * su + v * sv) * half;
        vertices.push(Vertex {
            position: position.to_array(),
            normal: normal.to_array(),
            uv,
        });
    }
    indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cuboid_has_expected_counts_and_bounds() {
        let mesh = MeshData::cuboid(Vec3::new(2.0, 4.0, 6.0));
        assert_eq!(mesh.vertices().len(), 24);
        assert_eq!(mesh.indices().len(), 36);
        assert_eq!(mesh.bounds().min, Vec3::new(-1.0, -2.0, -3.0));
        assert_eq!(mesh.bounds().max, Vec3::new(1.0, 2.0, 3.0));
    }

    #[test]
    fn cuboid_triangles_wind_counter_clockwise_outward() {
        let mesh = MeshData::cuboid(Vec3::ONE);
        for tri in mesh.indices().chunks(3) {
            let p: Vec<Vec3> = tri
                .iter()
                .map(|&i| Vec3::from(mesh.vertices()[i as usize].position))
                .collect();
            let n = Vec3::from(mesh.vertices()[tri[0] as usize].normal);
            let geometric = (p[1] - p[0]).cross(p[2] - p[0]);
            assert!(geometric.dot(n) > 0.0);
        }
    }

    #[test]
    fn plane_faces_up() {
        let mesh = MeshData::plane(2.0, 2.0);
        assert!(mesh.vertices().iter().all(|v| v.normal == [0.0, 1.0, 0.0]));
        assert_eq!(mesh.bounds().max.y, 0.0);
    }

    #[test]
    fn rejects_bad_meshes() {
        let v = Vertex {
            position: [0.0; 3],
            normal: [0.0, 1.0, 0.0],
            uv: [0.0; 2],
        };
        assert!(MeshData::new("e", vec![], vec![0, 0, 0]).is_err());
        assert!(MeshData::new("i", vec![v], vec![0, 0]).is_err());
        assert!(MeshData::new("o", vec![v], vec![0, 0, 1]).is_err());
        let mut nan = v;
        nan.position[0] = f32::NAN;
        assert!(MeshData::new("n", vec![nan], vec![0, 0, 0]).is_err());
    }

    #[test]
    fn transformed_aabb_encloses_translated_box() {
        let b = MeshData::cuboid(Vec3::ONE).bounds();
        let moved = b.transformed(&Mat4::from_translation(Vec3::new(10.0, 0.0, 0.0)));
        assert_eq!(moved.min.x, 9.5);
        assert_eq!(moved.max.x, 10.5);
    }
}
