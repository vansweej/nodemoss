//! glTF animation → AnimationClip adaptation.

use rig_assets::{
    AnimationChannel, AnimationClip, AnimationClipHandle, AssetStore, ChannelProperty,
    KeyframeSampler, KeyframeValues,
};
use rig_math::Interpolation;
use rig_scene::{NodeId, SceneGraph};

use crate::buffers;
use crate::error::Result;

/// Adapt all glTF animations into `AnimationClip` assets registered in `store`.
pub(crate) fn adapt_animations(
    document: &gltf::Document,
    buffers: &[gltf::buffer::Data],
    node_map: &[Option<NodeId>],
    scene: &SceneGraph,
    store: &mut AssetStore,
) -> Result<Vec<AnimationClipHandle>> {
    let mut handles = Vec::new();
    for animation in document.animations() {
        let mut channels = Vec::new();
        for channel in animation.channels() {
            let target = channel.target();
            let Some(node_id) = node_map.get(target.node().index()).copied().flatten() else {
                continue;
            };
            let target_node = scene.node_name(node_id).unwrap_or("unknown").to_string();
            let property = match target.property() {
                gltf::animation::Property::Translation => ChannelProperty::Translation,
                gltf::animation::Property::Rotation => ChannelProperty::Rotation,
                gltf::animation::Property::Scale => ChannelProperty::Scale,
                gltf::animation::Property::MorphTargetWeights => {
                    ChannelProperty::MorphTargetWeights
                }
            };

            let sampler = channel.sampler();
            let times = buffers::read_timestamps(&channel, buffers);
            let interpolation = map_interpolation(sampler.interpolation());
            let values = read_values(&channel, buffers, property, interpolation);
            channels.push(AnimationChannel {
                target_node,
                property,
                sampler: KeyframeSampler {
                    times,
                    interpolation,
                    values,
                },
            });
        }

        let duration = channels
            .iter()
            .flat_map(|channel| channel.sampler.times.iter().copied())
            .fold(0.0_f32, f32::max);
        let name = animation
            .name()
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| format!("animation_{}", handles.len()));
        handles.push(store.add_animation_clip(AnimationClip {
            name,
            duration,
            looping: true,
            channels,
        }));
    }
    Ok(handles)
}

fn map_interpolation(interpolation: gltf::animation::Interpolation) -> Interpolation {
    match interpolation {
        gltf::animation::Interpolation::Step => Interpolation::Step,
        gltf::animation::Interpolation::Linear => Interpolation::Linear,
        gltf::animation::Interpolation::CubicSpline => Interpolation::CubicSpline,
    }
}

fn read_values(
    channel: &gltf::animation::Channel<'_>,
    buffers: &[gltf::buffer::Data],
    property: ChannelProperty,
    interpolation: Interpolation,
) -> KeyframeValues {
    match (property, interpolation) {
        (ChannelProperty::Translation, Interpolation::CubicSpline) => {
            KeyframeValues::CubicTranslations(buffers::read_anim_cubic_translations(
                channel, buffers,
            ))
        }
        (ChannelProperty::Rotation, Interpolation::CubicSpline) => {
            KeyframeValues::CubicRotations(buffers::read_anim_cubic_rotations(channel, buffers))
        }
        (ChannelProperty::Scale, Interpolation::CubicSpline) => {
            KeyframeValues::CubicScales(buffers::read_anim_cubic_scales(channel, buffers))
        }
        (ChannelProperty::MorphTargetWeights, Interpolation::CubicSpline) => {
            KeyframeValues::CubicMorphWeights(buffers::read_anim_cubic_morph_weight_frames(
                channel, buffers,
            ))
        }
        (ChannelProperty::Translation, _) => {
            KeyframeValues::Translations(buffers::read_anim_translations(channel, buffers))
        }
        (ChannelProperty::Rotation, _) => {
            KeyframeValues::Rotations(buffers::read_anim_rotations(channel, buffers))
        }
        (ChannelProperty::Scale, _) => {
            KeyframeValues::Scales(buffers::read_anim_scales(channel, buffers))
        }
        (ChannelProperty::MorphTargetWeights, _) => {
            KeyframeValues::MorphWeights(buffers::read_anim_morph_weight_frames(channel, buffers))
        }
    }
}
