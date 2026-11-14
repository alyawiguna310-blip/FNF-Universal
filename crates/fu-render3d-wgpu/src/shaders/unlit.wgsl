// builtin/unlit.wgsl

struct CameraUniform {
    view_proj: mat4x4<f32>,
    camera_pos: vec3<f32>,
    _pad0: f32,
};

struct ModelUniform {
    model: mat4x4<f32>,
    normal_mat: mat4x4<f32>,
};

struct MaterialUniform {
    base_color: vec4<f32>,
    emissive_roughness: vec4<f32>, // rgb: emissive, a: roughness
    params: vec4<f32>,            // x: metallic, y: alpha_cutoff, z: has_texture (1.0 or 0.0), w: padding
};

@group(0) @binding(0) var<uniform> camera: CameraUniform;
@group(1) @binding(0) var<uniform> model: ModelUniform;
@group(2) @binding(0) var<uniform> material: MaterialUniform;
@group(2) @binding(1) var t_diffuse: texture_2d<f32>;
@group(2) @binding(2) var s_diffuse: sampler;

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
};

@vertex
fn vs_main(in: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    let world_pos = model.model * vec4<f32>(in.position, 1.0);
    out.clip_position = camera.view_proj * world_pos;
    out.uv = in.uv;
    out.color = material.base_color;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    var col = in.color;
    if (material.params.z > 0.5) {
        col = col * textureSample(t_diffuse, s_diffuse, in.uv);
    }

    // Check alpha cutoff (params.y)
    let cutoff = material.params.y;
    if (cutoff > 0.0 && col.a < cutoff) {
        discard;
    }

    return col;
}
