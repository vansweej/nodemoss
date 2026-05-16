//! Typed glTF buffer reader helpers.

use rig_math::{Mat4, Quat, Vec3};

use crate::error::{GltfError, Result};

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

fn buffer_data<'a>(
    buffers: &'a [gltf::buffer::Data],
    buffer: gltf::Buffer<'_>,
) -> Option<&'a [u8]> {
    buffers.get(buffer.index()).map(|data| data.0.as_slice())
}
