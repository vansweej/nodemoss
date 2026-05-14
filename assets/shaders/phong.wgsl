struct FrameUniforms {
    view: mat4x4<f32>,
    proj: mat4x4<f32>,
    camera_pos: vec4<f32>,
}

struct MaterialUniforms {
    base_color: vec4<f32>,
    metallic: f32,
    roughness: f32,
    flags: u32,
    _pad: u32,
}

struct ObjectUniforms {
    world: mat4x4<f32>,
}

@group(0) @binding(0) var<uniform> frame: FrameUniforms;
@group(1) @binding(0) var<uniform> material: MaterialUniforms;
@group(2) @binding(0) var<uniform> object: ObjectUniforms;

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
}

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) world_normal: vec3<f32>,
}

@vertex
fn vs_main(in: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    let world_pos = object.world * vec4<f32>(in.position, 1.0);
    let normal_mat = mat3x3<f32>(object.world[0].xyz, object.world[1].xyz, object.world[2].xyz);
    out.clip_position = frame.proj * frame.view * world_pos;
    out.world_normal = normalize(normal_mat * in.normal);
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let normal_color = in.world_normal * 0.5 + vec3<f32>(0.5);
    let tint = material.base_color.rgb;
    return vec4<f32>(normal_color * tint, material.base_color.a);
}
