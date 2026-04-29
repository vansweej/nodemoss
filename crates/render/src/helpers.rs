//! Pure utility functions, uniform types, and embedded WGSL shader constants.

use bytemuck::{Pod, Zeroable};
use rig_assets::{VertexFormat, VertexLayout};
use rig_math::{Camera, Mat4};
use rig_scene::ExtractedCamera;

use crate::{RenderError, Result};

/// The depth format used for all main render passes.
pub const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

// ── Uniform structs ──────────────────────────────────────────────────────────

/// Per-frame uniform data: camera matrices uploaded once per frame.
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct FrameUniforms {
    pub view: [[f32; 4]; 4],
    pub proj: [[f32; 4]; 4],
    /// xyz = camera world position, w = padding.
    pub camera_pos: [f32; 4],
}

/// Per-material uniform data.
///
/// Layout (32 bytes, `repr(C)`):
/// - `base_color`: 16 bytes — albedo / tint (RGBA)
/// - `metallic`:    4 bytes — 0.0 = dielectric, 1.0 = full metal (PBR)
/// - `roughness`:   4 bytes — 0.0 = mirror-smooth, 1.0 = fully diffuse (PBR)
/// - `flags`:       4 bytes — shader-defined bit flags (e.g. bit 0 = has texture)
/// - `_pad`:        4 bytes — alignment padding
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct MaterialUniforms {
    pub base_color: [f32; 4],
    pub metallic: f32,
    pub roughness: f32,
    pub flags: u32,
    pub _pad: u32,
}

/// Maximum number of simultaneous lights supported by the Phong shader.
/// Change this constant (and the matching WGSL `const MAX_LIGHTS`) to adjust.
pub const MAX_LIGHTS: usize = 16;

/// GPU-side representation of a single light.
///
/// `position.w` encodes light type: `0.0` = directional, `1.0` = point.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable, Default)]
pub struct LightUniform {
    /// xyz = world position (point lights) or ignored (directional). w = type (0=dir, 1=point).
    pub position: [f32; 4],
    /// xyz = world direction (normalized). w = padding.
    pub direction: [f32; 4],
    /// rgb = color. a = intensity.
    pub color_intensity: [f32; 4],
    /// x = range (point lights). yzw = padding.
    pub range_pad: [f32; 4],
}

/// Packed array of lights uploaded to the GPU each frame.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable, Default)]
pub struct LightsBuffer {
    pub lights: [LightUniform; MAX_LIGHTS],
    /// x = active light count. yzw = padding.
    pub count: [u32; 4],
}

// ── Embedded shaders ─────────────────────────────────────────────────────────

/// Vertex-color triangle shader — 3-group layout (group 0 = frame, 1 = material, 2 = object).
pub const TRIANGLE_SHADER: &str = r#"
struct FrameUniforms {
    view: mat4x4<f32>,
    proj: mat4x4<f32>,
    camera_pos: vec4<f32>,
}

struct MaterialUniforms {
    base_color: vec4<f32>,
    metallic:  f32,
    roughness: f32,
    flags: u32,
    _pad: u32,
}

struct ObjectUniforms {
    world: mat4x4<f32>,
}

struct LightUniform {
    position: vec4<f32>,
    direction: vec4<f32>,
    color_intensity: vec4<f32>,
    range_pad: vec4<f32>,
}
struct LightsBuffer {
    lights: array<LightUniform, 16>,
    count: vec4<u32>,
}

@group(0) @binding(0) var<uniform> frame: FrameUniforms;
@group(0) @binding(1) var<uniform> lights: LightsBuffer;
@group(1) @binding(0) var<uniform> material: MaterialUniforms;
@group(1) @binding(1) var t_diffuse: texture_2d<f32>;
@group(1) @binding(2) var s_diffuse: sampler;
@group(2) @binding(0) var<uniform> object: ObjectUniforms;

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) color: vec3<f32>,
}

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) color: vec3<f32>,
}

@vertex
fn vs_main(in: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    let pv = frame.proj * frame.view;
    out.clip_position = pv * object.world * vec4<f32>(in.position, 1.0);
    out.color = in.color;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    return vec4<f32>(in.color, 1.0);
}
"#;

/// WGSL shader that maps vertex normals to RGB colour — 3-group layout.
///
/// Vertex layout: position @ location 0 (`Float32x3`), normal @ location 1
/// (`Float32x3`), UV @ location 2 (`Float32x2`). Stride = 32 bytes.
pub const NORMAL_COLOR_SHADER: &str = r#"
struct FrameUniforms {
    view: mat4x4<f32>,
    proj: mat4x4<f32>,
    camera_pos: vec4<f32>,
}

struct MaterialUniforms {
    base_color: vec4<f32>,
    metallic:  f32,
    roughness: f32,
    flags: u32,
    _pad: u32,
}

struct ObjectUniforms {
    world: mat4x4<f32>,
}

struct LightUniform {
    position: vec4<f32>,
    direction: vec4<f32>,
    color_intensity: vec4<f32>,
    range_pad: vec4<f32>,
}
struct LightsBuffer {
    lights: array<LightUniform, 16>,
    count: vec4<u32>,
}

@group(0) @binding(0) var<uniform> frame: FrameUniforms;
@group(0) @binding(1) var<uniform> lights: LightsBuffer;
@group(1) @binding(0) var<uniform> material: MaterialUniforms;
@group(1) @binding(1) var t_diffuse: texture_2d<f32>;
@group(1) @binding(2) var s_diffuse: sampler;
@group(2) @binding(0) var<uniform> object: ObjectUniforms;

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) normal:   vec3<f32>,
    @location(2) uv:       vec2<f32>,
}

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0)       color:         vec3<f32>,
}

@vertex
fn vs_main(in: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    let pv = frame.proj * frame.view;
    out.clip_position = pv * object.world * vec4<f32>(in.position, 1.0);
    out.color = in.normal * 0.5 + vec3<f32>(0.5, 0.5, 0.5);
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    return vec4<f32>(in.color, 1.0);
}
"#;

/// WGSL shader that samples a diffuse texture — 3-group layout.
///
/// Vertex layout: position @ location 0 (`Float32x3`), normal @ location 1
/// (`Float32x3`), UV @ location 2 (`Float32x2`). Stride = 32 bytes.
pub const TEXTURED_SHADER: &str = r#"
struct FrameUniforms {
    view: mat4x4<f32>,
    proj: mat4x4<f32>,
    camera_pos: vec4<f32>,
}

struct MaterialUniforms {
    base_color: vec4<f32>,
    metallic:  f32,
    roughness: f32,
    flags: u32,
    _pad: u32,
}

struct ObjectUniforms {
    world: mat4x4<f32>,
}

struct LightUniform {
    position: vec4<f32>,
    direction: vec4<f32>,
    color_intensity: vec4<f32>,
    range_pad: vec4<f32>,
}
struct LightsBuffer {
    lights: array<LightUniform, 16>,
    count: vec4<u32>,
}

@group(0) @binding(0) var<uniform> frame: FrameUniforms;
@group(0) @binding(1) var<uniform> lights: LightsBuffer;
@group(1) @binding(0) var<uniform> material: MaterialUniforms;
@group(1) @binding(1) var t_diffuse: texture_2d<f32>;
@group(1) @binding(2) var s_diffuse: sampler;
@group(2) @binding(0) var<uniform> object: ObjectUniforms;

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) normal:   vec3<f32>,
    @location(2) uv:       vec2<f32>,
}

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0)       uv:            vec2<f32>,
}

@vertex
fn vs_main(in: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    let pv = frame.proj * frame.view;
    out.clip_position = pv * object.world * vec4<f32>(in.position, 1.0);
    out.uv = in.uv;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let tex_color = textureSample(t_diffuse, s_diffuse, in.uv);
    return tex_color * material.base_color;
}
"#;

/// Blinn-Phong shading shader — 3-group layout, supports up to 16 lights from group 0 binding 1.
///
/// Vertex layout: position @ location 0 (`Float32x3`), normal @ location 1
/// (`Float32x3`), UV @ location 2 (`Float32x2`). Stride = 32 bytes.
pub const PHONG_SHADER: &str = r#"
const MAX_LIGHTS: u32 = 16u;

struct FrameUniforms {
    view: mat4x4<f32>,
    proj: mat4x4<f32>,
    camera_pos: vec4<f32>,
}
struct LightUniform {
    position: vec4<f32>,
    direction: vec4<f32>,
    color_intensity: vec4<f32>,
    range_pad: vec4<f32>,
}
struct LightsBuffer {
    lights: array<LightUniform, 16>,
    count: vec4<u32>,
}
struct MaterialUniforms {
    base_color: vec4<f32>,
    metallic:  f32,
    roughness: f32,
    flags: u32,
    _pad: u32,
}
struct ObjectUniforms {
    world: mat4x4<f32>,
}

@group(0) @binding(0) var<uniform> frame: FrameUniforms;
@group(0) @binding(1) var<uniform> lights_data: LightsBuffer;
@group(1) @binding(0) var<uniform> material: MaterialUniforms;
@group(1) @binding(1) var t_diffuse: texture_2d<f32>;
@group(1) @binding(2) var s_diffuse: sampler;
@group(2) @binding(0) var<uniform> object: ObjectUniforms;

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
}
struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) world_position: vec3<f32>,
    @location(1) world_normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
}

@vertex
fn vs_main(in: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    let world_pos = object.world * vec4<f32>(in.position, 1.0);
    let normal_mat = mat3x3<f32>(
        object.world[0].xyz,
        object.world[1].xyz,
        object.world[2].xyz,
    );
    out.world_position = world_pos.xyz;
    out.world_normal = normalize(normal_mat * in.normal);
    out.clip_position = frame.proj * frame.view * world_pos;
    out.uv = in.uv;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let N = normalize(in.world_normal);
    let V = normalize(frame.camera_pos.xyz - in.world_position);
    let base = material.base_color.rgb;

    var color = base * 0.05; // ambient

    let n_lights = min(lights_data.count.x, MAX_LIGHTS);
    for (var i = 0u; i < n_lights; i++) {
        let light = lights_data.lights[i];
        var L: vec3<f32>;
        var attenuation = 1.0;

        if light.position.w < 0.5 {
            // Directional
            L = normalize(-light.direction.xyz);
        } else {
            // Point
            let to_light = light.position.xyz - in.world_position;
            let dist = length(to_light);
            L = normalize(to_light);
            let range = light.range_pad.x;
            attenuation = clamp(1.0 - dist / range, 0.0, 1.0);
        }

        let light_color = light.color_intensity.rgb * light.color_intensity.a * attenuation;

        // Diffuse
        let diff = max(dot(N, L), 0.0);
        color += base * light_color * diff;

        // Specular (Blinn-Phong half-vector)
        let H = normalize(L + V);
        let spec = pow(max(dot(N, H), 0.0), 32.0);
        color += light_color * spec * 0.3;
    }

    return vec4<f32>(color, material.base_color.a);
}
"#;

/// Cook-Torrance PBR shader — metallic-roughness workflow, 3-group layout.
///
/// Implements GGX NDF (Trowbridge-Reitz), Smith's Schlick-GGX geometry term,
/// Fresnel-Schlick, a roughness-aware hemisphere ambient that approximates
/// environment reflections, UE4-style windowed inverse-square attenuation, and
/// ACES filmic tone mapping.  Supports up to 16 analytic lights from group 0
/// binding 1.
///
/// Material parameters used from `MaterialUniforms`:
/// - `base_color.rgb` — albedo / base reflectance colour
/// - `base_color.a`   — alpha (passed through)
/// - `metallic`       — 0 = dielectric, 1 = full metal
/// - `roughness`      — 0 = mirror-smooth, 1 = fully diffuse
///
/// Vertex layout: position @ location 0 (`Float32x3`), normal @ location 1
/// (`Float32x3`), UV @ location 2 (`Float32x2`). Stride = 32 bytes.
pub const PBR_SHADER: &str = r#"
const PI: f32 = 3.14159265358979323846;
const MAX_LIGHTS: u32 = 16u;

struct FrameUniforms {
    view: mat4x4<f32>,
    proj: mat4x4<f32>,
    camera_pos: vec4<f32>,
}
struct LightUniform {
    position: vec4<f32>,
    direction: vec4<f32>,
    color_intensity: vec4<f32>,
    range_pad: vec4<f32>,
}
struct LightsBuffer {
    lights: array<LightUniform, 16>,
    count: vec4<u32>,
}
struct MaterialUniforms {
    base_color: vec4<f32>,
    metallic:  f32,
    roughness: f32,
    flags: u32,
    _pad: u32,
}
struct ObjectUniforms {
    world: mat4x4<f32>,
}

@group(0) @binding(0) var<uniform> frame: FrameUniforms;
@group(0) @binding(1) var<uniform> lights_data: LightsBuffer;
@group(1) @binding(0) var<uniform> material: MaterialUniforms;
@group(1) @binding(1) var t_diffuse: texture_2d<f32>;
@group(1) @binding(2) var s_diffuse: sampler;
@group(2) @binding(0) var<uniform> object: ObjectUniforms;

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) normal:   vec3<f32>,
    @location(2) uv:       vec2<f32>,
}
struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) world_position: vec3<f32>,
    @location(1) world_normal:   vec3<f32>,
    @location(2) uv:             vec2<f32>,
}

@vertex
fn vs_main(in: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    let world_pos = object.world * vec4<f32>(in.position, 1.0);
    let normal_mat = mat3x3<f32>(
        object.world[0].xyz,
        object.world[1].xyz,
        object.world[2].xyz,
    );
    out.world_position = world_pos.xyz;
    out.world_normal   = normalize(normal_mat * in.normal);
    out.clip_position  = frame.proj * frame.view * world_pos;
    out.uv             = in.uv;
    return out;
}

// ── BRDF helpers ─────────────────────────────────────────────────────────────

/// Trowbridge-Reitz GGX normal distribution function.
fn distribution_ggx(N: vec3<f32>, H: vec3<f32>, roughness: f32) -> f32 {
    let a  = roughness * roughness;
    let a2 = a * a;
    let NdH  = max(dot(N, H), 0.0);
    let NdH2 = NdH * NdH;
    let denom = NdH2 * (a2 - 1.0) + 1.0;
    return a2 / (PI * denom * denom);
}

/// Schlick-GGX geometry sub-term (single direction).
fn geometry_schlick_ggx(NdotV: f32, roughness: f32) -> f32 {
    let r = roughness + 1.0;
    let k = (r * r) / 8.0;
    return NdotV / (NdotV * (1.0 - k) + k);
}

/// Smith's geometry term — accounts for both view and light directions.
fn geometry_smith(N: vec3<f32>, V: vec3<f32>, L: vec3<f32>, roughness: f32) -> f32 {
    let NdV = max(dot(N, V), 0.0);
    let NdL = max(dot(N, L), 0.0);
    return geometry_schlick_ggx(NdV, roughness) * geometry_schlick_ggx(NdL, roughness);
}

/// Fresnel-Schlick for analytic lights.
fn fresnel_schlick(cos_theta: f32, F0: vec3<f32>) -> vec3<f32> {
    return F0 + (1.0 - F0) * pow(clamp(1.0 - cos_theta, 0.0, 1.0), 5.0);
}

/// Fresnel-Schlick with roughness bias — used for the ambient/IBL term so that
/// rough surfaces don't get an unrealistically strong environment reflection.
/// From Sebastien Lagarde's "Moving Frostbite to PBR" (2014).
fn fresnel_schlick_roughness(cos_theta: f32, F0: vec3<f32>, roughness: f32) -> vec3<f32> {
    let one_minus_r = vec3<f32>(1.0 - roughness);
    return F0 + (max(one_minus_r, F0) - F0) * pow(clamp(1.0 - cos_theta, 0.0, 1.0), 5.0);
}

/// UE4-style windowed inverse-square falloff (Brian Karis, Siggraph 2013).
/// Physically motivated but range-bounded: zero at dist >= range.
fn point_light_attenuation(dist: f32, range: f32) -> f32 {
    let d_over_r = dist / range;
    let window   = clamp(1.0 - d_over_r * d_over_r * d_over_r * d_over_r, 0.0, 1.0);
    return (window * window) / (dist * dist + 1.0);
}

/// ACES filmic tone mapping (Narkowicz 2015 approximation).
/// Maps HDR radiance to [0,1] with a natural highlight roll-off.
fn aces_tonemap(x: vec3<f32>) -> vec3<f32> {
    let a = 2.51;
    let b = 0.03;
    let c = 2.43;
    let d = 0.59;
    let e = 0.14;
    return clamp((x * (a * x + b)) / (x * (c * x + d) + e), vec3<f32>(0.0), vec3<f32>(1.0));
}

// ── Hemisphere environment approximation ─────────────────────────────────────
//
// Without a cubemap we fake environment reflections by sampling a simple
// sky/ground gradient in the direction of the ideal reflection vector R.
// The gradient is blended by the Y-component of R:
//   R.y ≈ +1  →  sky colour  (bright, cool blue-white)
//   R.y ≈ -1  →  ground colour (darker, warm grey)
// The Fresnel term (roughness-biased) weights the contribution so that
// smooth surfaces pick up more of the environment, rough surfaces less.
// This gives polished metals the characteristic sheen across their whole
// surface, not just at the specular highlight positions.

fn sample_environment(R: vec3<f32>, roughness: f32) -> vec3<f32> {
    // HDR sky / ground gradient — values above 1.0 are intentional; ACES
    // tone-maps them back to display range.  Think of this as the studio
    // lighting rig reflected in the metal surface.
    // Silver studio environment: bright cool sky + DARK horizon + warm ground.
    // The contrast between sky and horizon is what makes a surface read as
    // "silvery" rather than "white" — the eye sees bright-vs-dark variation
    // across the surface instead of a uniform wash.
    let sky_col     = vec3<f32>(1.00, 1.05, 1.20); // bright cool blue sky (HDR)
    let horizon_col = vec3<f32>(0.18, 0.20, 0.26); // DARK cool grey — key to silver look
    let ground_col  = vec3<f32>(0.30, 0.27, 0.22); // dark warm ground bounce

    let t_sky    = clamp(R.y, 0.0, 1.0);
    let t_ground = clamp(-R.y, 0.0, 1.0);
    let env      = mix(horizon_col, sky_col, t_sky * t_sky); // squared for sharper sky
    let env_full = mix(env, ground_col, t_ground);

    // Only slightly dampen for rough surfaces — smooth metals should see
    // close to the full environment.
    let env_mip_bias = roughness * roughness;
    return mix(env_full, vec3<f32>(0.5), env_mip_bias);
}

// ── Fragment shader ───────────────────────────────────────────────────────────

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let albedo    = material.base_color.rgb;
    let metallic  = material.metallic;
    // Clamp roughness to 0.02 minimum so mirror-smooth is possible but the
    // GGX denominator never reaches zero.
    let roughness = clamp(material.roughness, 0.02, 1.0);

    let N   = normalize(in.world_normal);
    let V   = normalize(frame.camera_pos.xyz - in.world_position);
    let NdV = max(dot(N, V), 0.0);

    // Base reflectivity: dielectrics ≈ 0.04, metals use albedo as F0.
    let F0 = mix(vec3<f32>(0.04), albedo, metallic);

    // ── Analytic lights (direct illumination) ─────────────────────────────
    var Lo = vec3<f32>(0.0);

    let n_lights = min(lights_data.count.x, MAX_LIGHTS);
    for (var i = 0u; i < n_lights; i++) {
        let light = lights_data.lights[i];
        var L: vec3<f32>;
        var attenuation = 1.0;

        if light.position.w < 0.5 {
            // Directional light — no falloff.
            L = normalize(-light.direction.xyz);
        } else {
            // Point light — UE4 windowed inverse-square falloff.
            let to_light = light.position.xyz - in.world_position;
            let dist     = length(to_light);
            L            = normalize(to_light);
            let range    = light.range_pad.x;
            attenuation  = point_light_attenuation(dist, range);
        }

        let H           = normalize(V + L);
        let NdL         = max(dot(N, L), 0.0);
        let light_color = light.color_intensity.rgb * light.color_intensity.a * attenuation;

        // Cook-Torrance BRDF
        let D = distribution_ggx(N, H, roughness);
        let G = geometry_smith(N, V, L, roughness);
        let F = fresnel_schlick(max(dot(H, V), 0.0), F0);

        // kD = (1 - kS) * (1 - metallic): metals have no Lambertian diffuse.
        let kD      = (vec3<f32>(1.0) - F) * (1.0 - metallic);
        let specular = D * G * F / (4.0 * NdV * NdL + 0.0001);
        let diffuse  = kD * albedo / PI;

        Lo += (diffuse + specular) * light_color * NdL;
    }

    // ── Ambient / environment (indirect approximation) ────────────────────
    //
    // Use the roughness-biased Fresnel term to weight how much of the faked
    // environment shows up on this fragment.  Smooth metals get a near-full
    // mirror-like reflection of the sky/ground gradient; rough surfaces get
    // a flat diffuse ambient instead.
    let R         = reflect(-V, N);
    let F_env     = fresnel_schlick_roughness(NdV, F0, roughness);
    let kD_env    = (vec3<f32>(1.0) - F_env) * (1.0 - metallic);
    let irradiance = mix(vec3<f32>(0.25), albedo, 0.5); // cheap diffuse irradiance
    let env_spec  = sample_environment(R, roughness);
    let ambient   = kD_env * irradiance * albedo + F_env * env_spec;

    // ── Combine and tone-map ──────────────────────────────────────────────
    let hdr_color = ambient + Lo;

    // ACES filmic tone mapping: maps HDR radiance to display-referred [0,1].
    // Specular highlights can safely exceed 1.0 before this point.
    let ldr_color = aces_tonemap(hdr_color);

    return vec4<f32>(ldr_color, material.base_color.a);
}
"#;

// ── Helper functions ─────────────────────────────────────────────────────────

pub(crate) fn aligned_uniform_size(size: u64, alignment: u64) -> u64 {
    if alignment <= 1 {
        return size;
    }
    let remainder = size % alignment;
    if remainder == 0 {
        size
    } else {
        size + (alignment - remainder)
    }
}

pub(crate) fn object_uniform_offset(index: usize, stride: u64) -> Result<u32> {
    let offset = index as u64 * stride;
    u32::try_from(offset)
        .map_err(|_| RenderError::Asset("object uniform offset exceeds u32 range".into()))
}

pub(crate) fn encode_object_uniforms(
    uniforms: &[crate::frame::ObjectUniforms],
    stride: u64,
) -> Vec<u8> {
    let object_size = std::mem::size_of::<crate::frame::ObjectUniforms>();
    let stride = stride as usize;
    let mut bytes = vec![0_u8; stride * uniforms.len()];
    for (index, uniform) in uniforms.iter().enumerate() {
        let offset = index * stride;
        bytes[offset..offset + object_size].copy_from_slice(bytemuck::bytes_of(uniform));
    }
    bytes
}

pub(crate) fn decompose_pose(world: Mat4) -> rig_math::Transform {
    let (_, rotation, translation) = world.to_scale_rotation_translation();
    rig_math::Transform {
        translation,
        rotation,
        scale: rig_math::Vec3::ONE,
    }
}

pub(crate) fn camera_projection_view(camera: &ExtractedCamera, aspect: f32) -> Mat4 {
    let pose = decompose_pose(camera.world_transform);
    let camera_value = Camera {
        pose,
        projection: camera.projection,
    };
    camera_value.projection_view_matrix(aspect)
}

pub fn vertex_format_size(format: VertexFormat) -> u64 {
    match format {
        VertexFormat::Float32 => std::mem::size_of::<f32>() as u64,
        VertexFormat::Float32x2 => std::mem::size_of::<[f32; 2]>() as u64,
        VertexFormat::Float32x3 => std::mem::size_of::<[f32; 3]>() as u64,
        VertexFormat::Float32x4 => std::mem::size_of::<[f32; 4]>() as u64,
    }
}

pub fn wgpu_vertex_format(format: VertexFormat) -> wgpu::VertexFormat {
    match format {
        VertexFormat::Float32 => wgpu::VertexFormat::Float32,
        VertexFormat::Float32x2 => wgpu::VertexFormat::Float32x2,
        VertexFormat::Float32x3 => wgpu::VertexFormat::Float32x3,
        VertexFormat::Float32x4 => wgpu::VertexFormat::Float32x4,
    }
}

/// Generic vertex layout validator.
pub fn validate_vertex_layout(vertex_layout: &VertexLayout) -> std::result::Result<(), String> {
    if vertex_layout.array_stride == 0 {
        return Err("vertex layout must use a non-zero array stride".into());
    }
    if vertex_layout.attributes.is_empty() {
        return Err("vertex layout must contain at least one attribute".into());
    }
    let mut seen_locations = std::collections::HashSet::new();
    for attribute in &vertex_layout.attributes {
        if !seen_locations.insert(attribute.shader_location) {
            return Err(format!(
                "vertex layout contains duplicate shader location {}",
                attribute.shader_location
            ));
        }
        let format_size = vertex_format_size(attribute.format);
        if attribute.offset + format_size > vertex_layout.array_stride {
            return Err(format!(
                "vertex attribute at location {} exceeds the declared array stride",
                attribute.shader_location
            ));
        }
    }
    Ok(())
}

pub(crate) fn mesh_vertex_attributes(
    vertex_layout: &VertexLayout,
) -> std::result::Result<Vec<wgpu::VertexAttribute>, String> {
    validate_vertex_layout(vertex_layout)?;
    Ok(vertex_layout
        .attributes
        .iter()
        .map(|attribute| wgpu::VertexAttribute {
            format: wgpu_vertex_format(attribute.format),
            offset: attribute.offset,
            shader_location: attribute.shader_location,
        })
        .collect())
}

/// Create a depth texture and its default view sized to `width x height`.
#[cfg(not(tarpaulin_include))]
pub fn create_depth_texture(
    device: &wgpu::Device,
    width: u32,
    height: u32,
) -> (wgpu::Texture, wgpu::TextureView) {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("depth texture"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: DEPTH_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    (texture, view)
}

#[cfg(not(tarpaulin_include))]
pub(crate) fn create_pipeline(
    device: &wgpu::Device,
    shader: &wgpu::ShaderModule,
    pipeline_layout: &wgpu::PipelineLayout,
    color_format: wgpu::TextureFormat,
    depth_format: Option<wgpu::TextureFormat>,
    vertex_layout: &VertexLayout,
    polygon_mode: wgpu::PolygonMode,
) -> Result<wgpu::RenderPipeline> {
    let attributes = mesh_vertex_attributes(vertex_layout).map_err(RenderError::Asset)?;
    let buffer_layout = wgpu::VertexBufferLayout {
        array_stride: vertex_layout.array_stride,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &attributes,
    };
    let depth_stencil = depth_format.map(|format| wgpu::DepthStencilState {
        format,
        depth_write_enabled: Some(true),
        depth_compare: Some(wgpu::CompareFunction::Less),
        stencil: wgpu::StencilState::default(),
        bias: wgpu::DepthBiasState::default(),
    });
    Ok(
        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("rig render pipeline"),
            layout: Some(pipeline_layout),
            vertex: wgpu::VertexState {
                module: shader,
                entry_point: Some("vs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[buffer_layout],
            },
            fragment: Some(wgpu::FragmentState {
                module: shader,
                entry_point: Some("fs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: color_format,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: Some(wgpu::Face::Back),
                polygon_mode,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        }),
    )
}
