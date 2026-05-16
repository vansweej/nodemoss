//! Typed glTF buffer reader helpers.

use rig_math::{Mat4, Quat, Vec3};

use crate::error::{GltfError, Result};

pub(crate) struct MorphTargetData {
    pub target_count: usize,
    pub position_deltas: Vec<f32>,
    pub normal_deltas: Vec<f32>,
}

pub(crate) fn read_positions(
    primitive: &gltf::Primitive<'_>,
    buffers: &[gltf::buffer::Data],
) -> Result<Vec<f32>> {
    let reader = primitive.reader(|buffer| buffer_data(buffers, buffer));
    let positions = reader.read_positions().ok_or(GltfError::MissingPositions)?;
    Ok(positions
        .flat_map(|position| position.into_iter())
        .collect())
}

pub(crate) fn read_normals(
    primitive: &gltf::Primitive<'_>,
    buffers: &[gltf::buffer::Data],
) -> Option<Vec<f32>> {
    let reader = primitive.reader(|buffer| buffer_data(buffers, buffer));
    reader
        .read_normals()
        .map(|normals| normals.flat_map(|normal| normal.into_iter()).collect())
}

pub(crate) fn read_tex_coords(
    primitive: &gltf::Primitive<'_>,
    buffers: &[gltf::buffer::Data],
    set: u32,
) -> Option<Vec<f32>> {
    let reader = primitive.reader(|buffer| buffer_data(buffers, buffer));
    reader
        .read_tex_coords(set)
        .map(|coords| coords.into_f32().flat_map(|uv| uv.into_iter()).collect())
}

pub(crate) fn read_tangents(
    primitive: &gltf::Primitive<'_>,
    buffers: &[gltf::buffer::Data],
) -> Option<Vec<[f32; 4]>> {
    let reader = primitive.reader(|buffer| buffer_data(buffers, buffer));
    reader.read_tangents().map(|tangents| tangents.collect())
}

pub(crate) fn read_indices(
    primitive: &gltf::Primitive<'_>,
    buffers: &[gltf::buffer::Data],
) -> Option<Vec<u32>> {
    let reader = primitive.reader(|buffer| buffer_data(buffers, buffer));
    reader
        .read_indices()
        .map(|indices| indices.into_u32().collect())
}

pub(crate) fn read_morph_targets(
    primitive: &gltf::Primitive<'_>,
    buffers: &[gltf::buffer::Data],
    vertex_count: usize,
) -> Option<MorphTargetData> {
    let reader = primitive.reader(|buffer| buffer_data(buffers, buffer));
    let mut target_count = 0;
    let mut position_deltas = Vec::new();
    let mut normal_deltas = Vec::new();

    for (positions, normals, _) in reader.read_morph_targets() {
        let positions = positions?;
        let target_positions: Vec<f32> = positions
            .flat_map(|position| position.into_iter())
            .collect();
        if target_positions.len() != vertex_count * 3 {
            return None;
        }

        let target_normals: Vec<f32> = normals
            .map(|normals| normals.flat_map(|normal| normal.into_iter()).collect())
            .unwrap_or_else(|| vec![0.0; vertex_count * 3]);
        if target_normals.len() != vertex_count * 3 {
            return None;
        }

        target_count += 1;
        position_deltas.extend(target_positions);
        normal_deltas.extend(target_normals);
    }

    (target_count > 0).then_some(MorphTargetData {
        target_count,
        position_deltas,
        normal_deltas,
    })
}

pub(crate) fn read_joints(
    primitive: &gltf::Primitive<'_>,
    buffers: &[gltf::buffer::Data],
    set: u32,
) -> Option<Vec<[u16; 4]>> {
    let reader = primitive.reader(|buffer| buffer_data(buffers, buffer));
    reader
        .read_joints(set)
        .map(|joints| joints.into_u16().collect())
}

pub(crate) fn read_weights(
    primitive: &gltf::Primitive<'_>,
    buffers: &[gltf::buffer::Data],
    set: u32,
) -> Option<Vec<[f32; 4]>> {
    let reader = primitive.reader(|buffer| buffer_data(buffers, buffer));
    reader
        .read_weights(set)
        .map(|weights| weights.into_f32().collect())
}

pub(crate) fn read_inverse_bind_matrices(
    skin: &gltf::Skin<'_>,
    buffers: &[gltf::buffer::Data],
) -> Vec<Mat4> {
    let reader = skin.reader(|buffer| buffer_data(buffers, buffer));
    reader
        .read_inverse_bind_matrices()
        .map(|matrices| {
            matrices
                .map(|matrix| Mat4::from_cols_array_2d(&matrix))
                .collect()
        })
        .unwrap_or_else(|| vec![Mat4::IDENTITY; skin.joints().count()])
}

pub(crate) fn read_timestamps(
    channel: &gltf::animation::Channel<'_>,
    buffers: &[gltf::buffer::Data],
) -> Vec<f32> {
    let reader = channel.reader(|buffer| buffer_data(buffers, buffer));
    reader
        .read_inputs()
        .map(|inputs| inputs.collect())
        .unwrap_or_default()
}

pub(crate) fn read_anim_translations(
    channel: &gltf::animation::Channel<'_>,
    buffers: &[gltf::buffer::Data],
) -> Vec<Vec3> {
    let reader = channel.reader(|buffer| buffer_data(buffers, buffer));
    match reader.read_outputs() {
        Some(gltf::animation::util::ReadOutputs::Translations(values)) => values
            .map(|value| Vec3::new(value[0], value[1], value[2]))
            .collect(),
        _ => Vec::new(),
    }
}

pub(crate) fn read_anim_rotations(
    channel: &gltf::animation::Channel<'_>,
    buffers: &[gltf::buffer::Data],
) -> Vec<Quat> {
    let reader = channel.reader(|buffer| buffer_data(buffers, buffer));
    match reader.read_outputs() {
        Some(gltf::animation::util::ReadOutputs::Rotations(values)) => values
            .into_f32()
            .map(|value| Quat::from_xyzw(value[0], value[1], value[2], value[3]))
            .collect(),
        _ => Vec::new(),
    }
}

pub(crate) fn read_anim_scales(
    channel: &gltf::animation::Channel<'_>,
    buffers: &[gltf::buffer::Data],
) -> Vec<Vec3> {
    let reader = channel.reader(|buffer| buffer_data(buffers, buffer));
    match reader.read_outputs() {
        Some(gltf::animation::util::ReadOutputs::Scales(values)) => values
            .map(|value| Vec3::new(value[0], value[1], value[2]))
            .collect(),
        _ => Vec::new(),
    }
}

pub(crate) fn read_anim_cubic_translations(
    channel: &gltf::animation::Channel<'_>,
    buffers: &[gltf::buffer::Data],
) -> Vec<[Vec3; 3]> {
    read_anim_translations(channel, buffers)
        .chunks_exact(3)
        .map(|chunk| [chunk[0], chunk[1], chunk[2]])
        .collect()
}

pub(crate) fn read_anim_cubic_rotations(
    channel: &gltf::animation::Channel<'_>,
    buffers: &[gltf::buffer::Data],
) -> Vec<[Quat; 3]> {
    read_anim_rotations(channel, buffers)
        .chunks_exact(3)
        .map(|chunk| [chunk[0], chunk[1], chunk[2]])
        .collect()
}

pub(crate) fn read_anim_cubic_scales(
    channel: &gltf::animation::Channel<'_>,
    buffers: &[gltf::buffer::Data],
) -> Vec<[Vec3; 3]> {
    read_anim_scales(channel, buffers)
        .chunks_exact(3)
        .map(|chunk| [chunk[0], chunk[1], chunk[2]])
        .collect()
}

pub(crate) fn read_anim_morph_weights(
    channel: &gltf::animation::Channel<'_>,
    buffers: &[gltf::buffer::Data],
) -> Vec<f32> {
    let reader = channel.reader(|buffer| buffer_data(buffers, buffer));
    match reader.read_outputs() {
        Some(gltf::animation::util::ReadOutputs::MorphTargetWeights(values)) => {
            values.into_f32().collect()
        }
        _ => Vec::new(),
    }
}

pub(crate) fn read_anim_morph_weight_frames(
    channel: &gltf::animation::Channel<'_>,
    buffers: &[gltf::buffer::Data],
) -> Vec<Vec<f32>> {
    let target_count = morph_target_count(channel);
    if target_count == 0 {
        return Vec::new();
    }
    read_anim_morph_weights(channel, buffers)
        .chunks_exact(target_count)
        .map(<[f32]>::to_vec)
        .collect()
}

fn morph_target_count(channel: &gltf::animation::Channel<'_>) -> usize {
    channel
        .target()
        .node()
        .mesh()
        .map(|mesh| {
            mesh.primitives()
                .next()
                .map(|primitive| primitive.morph_targets().count())
                .filter(|count| *count > 0)
                .or_else(|| mesh.weights().map(<[f32]>::len))
                .unwrap_or(0)
        })
        .unwrap_or(0)
}

pub(crate) fn read_anim_cubic_morph_weight_frames(
    channel: &gltf::animation::Channel<'_>,
    buffers: &[gltf::buffer::Data],
) -> Vec<[Vec<f32>; 3]> {
    read_anim_morph_weight_frames(channel, buffers)
        .chunks_exact(3)
        .map(|chunk| [chunk[0].clone(), chunk[1].clone(), chunk[2].clone()])
        .collect()
}

fn buffer_data<'a>(
    buffers: &'a [gltf::buffer::Data],
    buffer: gltf::Buffer<'_>,
) -> Option<&'a [u8]> {
    buffers.get(buffer.index()).map(|data| data.0.as_slice())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_positions_reports_missing_required_attribute() {
        let bytes = f32_bytes(&[0.0_f32, 0.0, 0.0]);
        let json = format!(
            r#"{{
                "asset": {{ "version": "2.0" }},
                "buffers": [ {{ "byteLength": {} }} ],
                "bufferViews": [ {{ "buffer": 0, "byteOffset": 0, "byteLength": {} }} ],
                "accessors": [
                    {{
                        "bufferView": 0,
                        "componentType": 5126,
                        "count": 1,
                        "type": "VEC3",
                        "min": [0.0, 0.0, 0.0],
                        "max": [0.0, 0.0, 0.0]
                    }}
                ],
                "meshes": [
                    {{ "primitives": [ {{ "attributes": {{ "POSITION": 0 }} }} ] }}
                ]
            }}"#,
            bytes.len(),
            bytes.len()
        );
        let gltf = parse_gltf(&json);
        let primitive = gltf
            .document
            .meshes()
            .next()
            .expect("mesh exists")
            .primitives()
            .next()
            .expect("primitive exists");

        let result = read_positions(&primitive, &[]);

        assert!(matches!(result, Err(GltfError::MissingPositions)));
    }

    #[test]
    fn read_positions_flattens_vec3_accessor_data() {
        let positions = [0.0_f32, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0];
        let bytes = f32_bytes(&positions);
        let json = format!(
            r#"{{
                "asset": {{ "version": "2.0" }},
                "buffers": [ {{ "byteLength": {} }} ],
                "bufferViews": [ {{ "buffer": 0, "byteOffset": 0, "byteLength": {} }} ],
                "accessors": [
                    {{
                        "bufferView": 0,
                        "componentType": 5126,
                        "count": 3,
                        "type": "VEC3",
                        "min": [0.0, 0.0, 0.0],
                        "max": [1.0, 1.0, 0.0]
                    }}
                ],
                "meshes": [
                    {{ "primitives": [ {{ "attributes": {{ "POSITION": 0 }} }} ] }}
                ]
            }}"#,
            bytes.len(),
            bytes.len()
        );
        let gltf = parse_gltf(&json);
        let primitive = gltf
            .document
            .meshes()
            .next()
            .expect("mesh exists")
            .primitives()
            .next()
            .expect("primitive exists");
        let buffers = [gltf::buffer::Data(bytes)];

        let flattened = read_positions(&primitive, &buffers).expect("positions read");

        assert_eq!(flattened, positions);
    }

    #[test]
    fn read_morph_targets_reads_position_deltas_and_fills_missing_normals() {
        let base_positions = [0.0_f32, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0];
        let morph_positions = [0.0_f32, 0.0, 1.0, 0.0, 0.0, 2.0, 0.0, 0.0, 3.0];
        let mut bytes = f32_bytes(&base_positions);
        let morph_offset = bytes.len();
        bytes.extend(f32_bytes(&morph_positions));
        let json = format!(
            r#"{{
                "asset": {{ "version": "2.0" }},
                "buffers": [ {{ "byteLength": {} }} ],
                "bufferViews": [
                    {{ "buffer": 0, "byteOffset": 0, "byteLength": {} }},
                    {{ "buffer": 0, "byteOffset": {}, "byteLength": {} }}
                ],
                "accessors": [
                    {{
                        "bufferView": 0,
                        "componentType": 5126,
                        "count": 3,
                        "type": "VEC3",
                        "min": [0.0, 0.0, 0.0],
                        "max": [1.0, 1.0, 0.0]
                    }},
                    {{
                        "bufferView": 1,
                        "componentType": 5126,
                        "count": 3,
                        "type": "VEC3"
                    }}
                ],
                "meshes": [
                    {{
                        "primitives": [
                            {{
                                "attributes": {{ "POSITION": 0 }},
                                "targets": [ {{ "POSITION": 1 }} ]
                            }}
                        ]
                    }}
                ]
            }}"#,
            bytes.len(),
            morph_offset,
            morph_offset,
            bytes.len() - morph_offset
        );
        let gltf = parse_gltf(&json);
        let primitive = gltf
            .document
            .meshes()
            .next()
            .expect("mesh exists")
            .primitives()
            .next()
            .expect("primitive exists");
        let buffers = [gltf::buffer::Data(bytes)];

        let data = read_morph_targets(&primitive, &buffers, 3).expect("morph target data");

        assert_eq!(data.target_count, 1);
        assert_eq!(data.position_deltas, morph_positions.to_vec());
        assert_eq!(data.normal_deltas, vec![0.0; 9]);
    }

    #[test]
    fn read_anim_morph_weight_frames_uses_primitive_target_count_without_default_weights() {
        let base_positions = [0.0_f32, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0];
        let first_morph_positions = [0.0_f32, 0.0, 1.0, 0.0, 0.0, 2.0, 0.0, 0.0, 3.0];
        let second_morph_positions = [0.0_f32, 1.0, 0.0, 0.0, 2.0, 0.0, 0.0, 3.0, 0.0];
        let times = [0.0_f32, 1.0];
        let weights = [0.0_f32, 1.0, 0.5, 0.25];
        let mut bytes = f32_bytes(&base_positions);
        let first_morph_offset = bytes.len();
        bytes.extend(f32_bytes(&first_morph_positions));
        let second_morph_offset = bytes.len();
        bytes.extend(f32_bytes(&second_morph_positions));
        let times_offset = bytes.len();
        bytes.extend(f32_bytes(&times));
        let weights_offset = bytes.len();
        bytes.extend(f32_bytes(&weights));
        let json = format!(
            r#"{{
                "asset": {{ "version": "2.0" }},
                "buffers": [ {{ "byteLength": {} }} ],
                "bufferViews": [
                    {{ "buffer": 0, "byteOffset": 0, "byteLength": {} }},
                    {{ "buffer": 0, "byteOffset": {}, "byteLength": {} }},
                    {{ "buffer": 0, "byteOffset": {}, "byteLength": {} }},
                    {{ "buffer": 0, "byteOffset": {}, "byteLength": {} }},
                    {{ "buffer": 0, "byteOffset": {}, "byteLength": {} }}
                ],
                "accessors": [
                    {{
                        "bufferView": 0,
                        "componentType": 5126,
                        "count": 3,
                        "type": "VEC3",
                        "min": [0.0, 0.0, 0.0],
                        "max": [1.0, 1.0, 0.0]
                    }},
                    {{ "bufferView": 1, "componentType": 5126, "count": 3, "type": "VEC3" }},
                    {{ "bufferView": 2, "componentType": 5126, "count": 3, "type": "VEC3" }},
                    {{
                        "bufferView": 3,
                        "componentType": 5126,
                        "count": 2,
                        "type": "SCALAR",
                        "min": [0.0],
                        "max": [1.0]
                    }},
                    {{ "bufferView": 4, "componentType": 5126, "count": 4, "type": "SCALAR" }}
                ],
                "meshes": [
                    {{
                        "primitives": [
                            {{
                                "attributes": {{ "POSITION": 0 }},
                                "targets": [ {{ "POSITION": 1 }}, {{ "POSITION": 2 }} ]
                            }}
                        ]
                    }}
                ],
                "nodes": [ {{ "mesh": 0 }} ],
                "animations": [
                    {{
                        "samplers": [ {{ "input": 3, "output": 4, "interpolation": "LINEAR" }} ],
                        "channels": [ {{ "sampler": 0, "target": {{ "node": 0, "path": "weights" }} }} ]
                    }}
                ]
            }}"#,
            bytes.len(),
            first_morph_offset,
            first_morph_offset,
            second_morph_offset - first_morph_offset,
            second_morph_offset,
            times_offset - second_morph_offset,
            times_offset,
            weights_offset - times_offset,
            weights_offset,
            bytes.len() - weights_offset
        );
        let gltf = parse_gltf(&json);
        let channel = gltf
            .document
            .animations()
            .next()
            .expect("animation exists")
            .channels()
            .next()
            .expect("channel exists");
        let buffers = [gltf::buffer::Data(bytes)];

        let frames = read_anim_morph_weight_frames(&channel, &buffers);

        assert_eq!(frames, vec![vec![0.0, 1.0], vec![0.5, 0.25]]);
    }

    #[test]
    fn read_inverse_bind_matrices_defaults_to_identity_per_joint() {
        let gltf = parse_gltf(
            r#"{
                "asset": { "version": "2.0" },
                "nodes": [ {}, {}, {} ],
                "skins": [ { "joints": [0, 1, 2] } ]
            }"#,
        );
        let skin = gltf.document.skins().next().expect("skin exists");

        let matrices = read_inverse_bind_matrices(&skin, &[]);

        assert_eq!(matrices, vec![Mat4::IDENTITY; 3]);
    }

    #[test]
    fn buffer_data_returns_none_when_buffer_payload_is_missing() {
        let gltf = parse_gltf(
            r#"{
                "asset": { "version": "2.0" },
                "buffers": [ { "byteLength": 4 } ]
            }"#,
        );
        let buffer = gltf.document.buffers().next().expect("buffer exists");

        assert_eq!(buffer_data(&[], buffer), None);
    }

    fn parse_gltf(json: &str) -> gltf::Gltf {
        gltf::Gltf::from_slice(json.as_bytes()).expect("valid glTF fixture")
    }

    fn f32_bytes(values: &[f32]) -> Vec<u8> {
        values
            .iter()
            .flat_map(|value| value.to_le_bytes())
            .collect()
    }
}
