// builtin/lit.wgsl

struct CameraUniform {
    view_proj: mat4x4<f32>,
    camera_pos: vec3<f32>,
    _pad0: f32,
};

struct LightData {
    position_range: vec4<f32>, // xyz: pos/dir, w: range (<= 0 for directional)
    color_intensity: vec4<f32>, // rgb: color, w: intensity
    direction_kind: vec4<f32>,  // xyz: direction, w: kind (0 = directional, 1 = point, 2 = spot)
    spot_angles: vec4<f32>,     // x: inner_cone, y: outer_cone, zw: pad
};

struct LightingUniform {
    ambient_color_intensity: vec4<f32>, // rgb: ambient_color, w: ambient_intensity
    light_count: vec4<u32>,             // x: count, yzw: pad
    lights: array<LightData, 16>,
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
@group(0) @binding(1) var<uniform> lighting: LightingUniform;

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
    @location(0) world_pos: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
};

@vertex
fn vs_main(in: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    let world_pos = model.model * vec4<f32>(in.position, 1.0);
    out.clip_position = camera.view_proj * world_pos;
    out.world_pos = world_pos.xyz;
    out.normal = normalize((model.normal_mat * vec4<f32>(in.normal, 0.0)).xyz);
    out.uv = in.uv;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    var albedo = material.base_color;
    if (material.params.z > 0.5) {
        albedo = albedo * textureSample(t_diffuse, s_diffuse, in.uv);
    }

    // Alpha cutoff
    let cutoff = material.params.y;
    if (cutoff > 0.0 && albedo.a < cutoff) {
        discard;
    }

    let N = normalize(in.normal);
    let V = normalize(camera.camera_pos - in.world_pos);

    // Ambient
    let ambient = lighting.ambient_color_intensity.rgb * lighting.ambient_color_intensity.w;
    var diffuse_accum = vec3<f32>(0.0);
    var specular_accum = vec3<f32>(0.0);

    let count = min(lighting.light_count.x, 16u);
    for (var i = 0u; i < count; i = i + 1u) {
        let light = lighting.lights[i];
        let kind = u32(light.direction_kind.w + 0.5);

        var L = vec3<f32>(0.0);
        var attenuation = 1.0;

        if (kind == 0u) {
            // Directional
            L = normalize(-light.direction_kind.xyz);
        } else {
            // Point or Spot
            let light_to_frag = in.world_pos - light.position_range.xyz;
            let dist = length(light_to_frag);
            let range = light.position_range.w;
            if (dist > range && range > 0.0) {
                continue;
            }
            L = normalize(-light_to_frag);

            // Standard inverse square falloff with range clamp
            let d_norm = clamp(dist / max(range, 0.001), 0.0, 1.0);
            attenuation = max(0.0, 1.0 - d_norm * d_norm);

            if (kind == 2u) {
                // Spot cone
                let spot_dir = normalize(light.direction_kind.xyz);
                let theta = dot(-L, spot_dir);
                let inner = light.spot_angles.x;
                let outer = light.spot_angles.y;
                let epsilon = max(inner - outer, 0.0001);
                let spot_intensity = clamp((theta - outer) / epsilon, 0.0, 1.0);
                attenuation = attenuation * spot_intensity;
            }
        }

        let NdotL = max(dot(N, L), 0.0);
        let light_col = light.color_intensity.rgb * light.color_intensity.w;
        diffuse_accum = diffuse_accum + light_col * NdotL * attenuation;

        // Simple blinn-phong specular
        if (NdotL > 0.0) {
            let H = normalize(L + V);
            let NdotH = max(dot(N, H), 0.0);
            let roughness = clamp(material.emissive_roughness.w, 0.04, 1.0);
            let shininess = (2.0 / (roughness * roughness)) - 2.0;
            let spec = pow(NdotH, shininess);
            specular_accum = specular_accum + light_col * spec * attenuation;
        }
    }

    let emissive = material.emissive_roughness.rgb;
    let final_rgb = albedo.rgb * (ambient + diffuse_accum) + specular_accum * material.params.x + emissive;

    return vec4<f32>(final_rgb, albedo.a);
}
