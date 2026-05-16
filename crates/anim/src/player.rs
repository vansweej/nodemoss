//! Animation player implementation.

use std::collections::HashMap;

use rig_assets::{
    AnimationClip, AnimationClipHandle, AssetStore, ChannelProperty, KeyframeSampler,
    KeyframeValues,
};
use rig_math::interpolation::{
    cubic_hermite_quat, cubic_hermite_vec3, find_keyframe_index, sample_quat_linear,
    sample_quat_step, sample_vec3_linear, sample_vec3_step,
};
use rig_math::{Interpolation, Quat, Vec3};
use rig_scene::{NodeId, SceneGraph};

use crate::AnimError;

/// Per-instance animation playback controller.
///
/// Holds current playback state plus a binding table that maps clip channel
/// indices to scene graph nodes. Call [`bind`](Self::bind) once after assigning
/// a clip, then call [`advance`](Self::advance) and [`evaluate`](Self::evaluate)
/// each frame.
pub struct AnimationPlayer {
    clip: AnimationClipHandle,
    time: f32,
    duration: f32,
    looping: bool,
    speed: f32,
    playing: bool,
    /// Channel index → scene node. `None` means unresolved and skipped.
    binding: Vec<Option<NodeId>>,
    /// Per-channel cached keyframe index hint.
    last_indices: Vec<usize>,
    /// Last evaluated morph weights per target node.
    morph_weights: HashMap<NodeId, Vec<f32>>,
}

#[derive(Clone, Copy, Debug, Default)]
struct SampledTransform {
    translation: Option<Vec3>,
    rotation: Option<Quat>,
    scale: Option<Vec3>,
}

enum SampledValue {
    Translation(Vec3),
    Rotation(Quat),
    Scale(Vec3),
    MorphWeights(Vec<f32>),
}

impl AnimationPlayer {
    pub fn new(clip: AnimationClipHandle) -> Self {
        Self {
            clip,
            time: 0.0,
            duration: 0.0,
            looping: false,
            speed: 1.0,
            playing: true,
            binding: Vec::new(),
            last_indices: Vec::new(),
            morph_weights: HashMap::new(),
        }
    }

    /// Resolve channel target names to scene graph nodes and cache clip metadata.
    ///
    /// Unresolved target names store `None` and are skipped during evaluation.
    pub fn bind(&mut self, assets: &AssetStore, scene: &SceneGraph) -> Result<(), AnimError> {
        let clip = self.clip_asset(assets)?;

        self.duration = clip.duration;
        self.looping = clip.looping;
        self.binding = clip
            .channels
            .iter()
            .map(|channel| scene.find_node_by_name(&channel.target_node))
            .collect();
        self.last_indices = vec![0; clip.channels.len()];
        Ok(())
    }

    /// Advance playback time by `dt * speed`.
    ///
    /// Uses clip duration and looping mode cached by [`bind`](Self::bind).
    pub fn advance(&mut self, dt: f32) {
        if !self.playing {
            return;
        }

        if self.duration <= 0.0 {
            self.time = 0.0;
            if !self.looping {
                self.playing = false;
            }
            return;
        }

        self.time += dt * self.speed;

        if self.looping {
            self.time = self.time.rem_euclid(self.duration);
        } else {
            self.time = self.time.clamp(0.0, self.duration);
            if self.time >= self.duration {
                self.playing = false;
            }
        }
    }

    /// Sample the bound clip at the current time and write local transforms.
    pub fn evaluate(
        &mut self,
        assets: &AssetStore,
        scene: &mut SceneGraph,
    ) -> Result<(), AnimError> {
        let clip = self.clip_asset(assets)?;
        let mut accumulated: HashMap<NodeId, SampledTransform> = HashMap::new();
        let mut morph_weights = HashMap::new();

        for (channel_index, channel) in clip.channels.iter().enumerate() {
            let Some(node) = self.binding.get(channel_index).copied().flatten() else {
                continue;
            };
            let Some(last_index) = self.last_indices.get_mut(channel_index) else {
                continue;
            };

            let sampled = sample_channel(
                channel.property,
                &channel.sampler,
                self.time,
                last_index,
                channel_index,
            )?;

            let entry = accumulated.entry(node).or_default();
            match sampled {
                SampledValue::Translation(value) => entry.translation = Some(value),
                SampledValue::Rotation(value) => entry.rotation = Some(value),
                SampledValue::Scale(value) => entry.scale = Some(value),
                SampledValue::MorphWeights(value) => {
                    morph_weights.insert(node, value);
                }
            }
        }

        self.morph_weights = morph_weights;

        for (node, sampled) in accumulated {
            let mut transform = scene.local_transform(node)?;
            if let Some(translation) = sampled.translation {
                transform.translation = translation;
            }
            if let Some(rotation) = sampled.rotation {
                transform.rotation = rotation;
            }
            if let Some(scale) = sampled.scale {
                transform.scale = scale;
            }
            scene.set_local_transform(node, transform)?;
        }

        Ok(())
    }

    pub fn time(&self) -> f32 {
        self.time
    }

    pub fn duration(&self) -> f32 {
        self.duration
    }

    pub fn set_time(&mut self, time: f32) {
        self.time = time;
        self.last_indices.fill(0);
    }

    pub fn speed(&self) -> f32 {
        self.speed
    }

    pub fn set_speed(&mut self, speed: f32) {
        self.speed = speed;
    }

    pub fn is_playing(&self) -> bool {
        self.playing
    }

    pub fn play(&mut self) {
        self.playing = true;
    }

    pub fn pause(&mut self) {
        self.playing = false;
    }

    pub fn toggle(&mut self) {
        self.playing = !self.playing;
    }

    pub fn clip_handle(&self) -> AnimationClipHandle {
        self.clip
    }

    /// Return the last evaluated morph weights for `node`, if the current clip drives them.
    pub fn morph_weights(&self, node: NodeId) -> Option<&[f32]> {
        self.morph_weights.get(&node).map(Vec::as_slice)
    }

    fn clip_asset<'a>(&self, assets: &'a AssetStore) -> Result<&'a AnimationClip, AnimError> {
        assets
            .animation_clip(self.clip)
            .map_err(|_| AnimError::InvalidClip)
    }
}

fn sample_channel(
    property: ChannelProperty,
    sampler: &KeyframeSampler,
    time: f32,
    last_index: &mut usize,
    channel_index: usize,
) -> Result<SampledValue, AnimError> {
    match property {
        ChannelProperty::Translation => {
            sample_translation(sampler, time, last_index, channel_index)
                .map(SampledValue::Translation)
        }
        ChannelProperty::Rotation => {
            sample_rotation(sampler, time, last_index, channel_index).map(SampledValue::Rotation)
        }
        ChannelProperty::Scale => {
            sample_scale(sampler, time, last_index, channel_index).map(SampledValue::Scale)
        }
        ChannelProperty::MorphTargetWeights => {
            sample_morph_weights(sampler, time, last_index, channel_index)
                .map(SampledValue::MorphWeights)
        }
    }
}

fn sample_translation(
    sampler: &KeyframeSampler,
    time: f32,
    last_index: &mut usize,
    channel_index: usize,
) -> Result<Vec3, AnimError> {
    match (&sampler.values, sampler.interpolation) {
        (KeyframeValues::Translations(values), _) if sampler.times.len() <= 1 => {
            first_vec3(values, channel_index)
        }
        (KeyframeValues::CubicTranslations(values), Interpolation::CubicSpline)
            if sampler.times.len() <= 1 =>
        {
            first_cubic_vec3(values, channel_index)
        }
        (KeyframeValues::Translations(values), Interpolation::Step) => {
            let (index, _) = find_keyframe_index(&sampler.times, time, last_index);
            checked_vec3_step(values, index, channel_index)
        }
        (KeyframeValues::Translations(values), Interpolation::Linear) => {
            let (index, t) = find_keyframe_index(&sampler.times, time, last_index);
            checked_vec3_linear(values, index, t, channel_index)
        }
        (KeyframeValues::CubicTranslations(values), Interpolation::CubicSpline) => {
            let (index, t) = find_keyframe_index(&sampler.times, time, last_index);
            checked_cubic_vec3(values, &sampler.times, index, t, channel_index)
        }
        _ => Err(AnimError::InvalidSampler { channel_index }),
    }
}

fn sample_rotation(
    sampler: &KeyframeSampler,
    time: f32,
    last_index: &mut usize,
    channel_index: usize,
) -> Result<Quat, AnimError> {
    match (&sampler.values, sampler.interpolation) {
        (KeyframeValues::Rotations(values), _) if sampler.times.len() <= 1 => {
            first_quat(values, channel_index)
        }
        (KeyframeValues::CubicRotations(values), Interpolation::CubicSpline)
            if sampler.times.len() <= 1 =>
        {
            first_cubic_quat(values, channel_index)
        }
        (KeyframeValues::Rotations(values), Interpolation::Step) => {
            let (index, _) = find_keyframe_index(&sampler.times, time, last_index);
            checked_quat_step(values, index, channel_index)
        }
        (KeyframeValues::Rotations(values), Interpolation::Linear) => {
            let (index, t) = find_keyframe_index(&sampler.times, time, last_index);
            checked_quat_linear(values, index, t, channel_index)
        }
        (KeyframeValues::CubicRotations(values), Interpolation::CubicSpline) => {
            let (index, t) = find_keyframe_index(&sampler.times, time, last_index);
            checked_cubic_quat(values, &sampler.times, index, t, channel_index)
        }
        _ => Err(AnimError::InvalidSampler { channel_index }),
    }
}

fn sample_scale(
    sampler: &KeyframeSampler,
    time: f32,
    last_index: &mut usize,
    channel_index: usize,
) -> Result<Vec3, AnimError> {
    match (&sampler.values, sampler.interpolation) {
        (KeyframeValues::Scales(values), _) if sampler.times.len() <= 1 => {
            first_vec3(values, channel_index)
        }
        (KeyframeValues::CubicScales(values), Interpolation::CubicSpline)
            if sampler.times.len() <= 1 =>
        {
            first_cubic_vec3(values, channel_index)
        }
        (KeyframeValues::Scales(values), Interpolation::Step) => {
            let (index, _) = find_keyframe_index(&sampler.times, time, last_index);
            checked_vec3_step(values, index, channel_index)
        }
        (KeyframeValues::Scales(values), Interpolation::Linear) => {
            let (index, t) = find_keyframe_index(&sampler.times, time, last_index);
            checked_vec3_linear(values, index, t, channel_index)
        }
        (KeyframeValues::CubicScales(values), Interpolation::CubicSpline) => {
            let (index, t) = find_keyframe_index(&sampler.times, time, last_index);
            checked_cubic_vec3(values, &sampler.times, index, t, channel_index)
        }
        _ => Err(AnimError::InvalidSampler { channel_index }),
    }
}

fn sample_morph_weights(
    sampler: &KeyframeSampler,
    time: f32,
    last_index: &mut usize,
    channel_index: usize,
) -> Result<Vec<f32>, AnimError> {
    match (&sampler.values, sampler.interpolation) {
        (KeyframeValues::MorphWeights(values), _) if sampler.times.len() <= 1 => {
            first_morph_weights(values, channel_index)
        }
        (KeyframeValues::CubicMorphWeights(values), Interpolation::CubicSpline)
            if sampler.times.len() <= 1 =>
        {
            first_cubic_morph_weights(values, channel_index)
        }
        (KeyframeValues::MorphWeights(values), Interpolation::Step) => {
            let (index, _) = find_keyframe_index(&sampler.times, time, last_index);
            checked_morph_weights_step(values, index, channel_index)
        }
        (KeyframeValues::MorphWeights(values), Interpolation::Linear) => {
            let (index, t) = find_keyframe_index(&sampler.times, time, last_index);
            checked_morph_weights_linear(values, index, t, channel_index)
        }
        (KeyframeValues::CubicMorphWeights(values), Interpolation::CubicSpline) => {
            let (index, t) = find_keyframe_index(&sampler.times, time, last_index);
            checked_cubic_morph_weights(values, &sampler.times, index, t, channel_index)
        }
        _ => Err(AnimError::InvalidSampler { channel_index }),
    }
}

fn first_vec3(values: &[Vec3], channel_index: usize) -> Result<Vec3, AnimError> {
    values
        .first()
        .copied()
        .ok_or(AnimError::InvalidSampler { channel_index })
}

fn first_quat(values: &[Quat], channel_index: usize) -> Result<Quat, AnimError> {
    values
        .first()
        .copied()
        .ok_or(AnimError::InvalidSampler { channel_index })
}

fn first_cubic_vec3(values: &[[Vec3; 3]], channel_index: usize) -> Result<Vec3, AnimError> {
    values
        .first()
        .map(|key| key[1])
        .ok_or(AnimError::InvalidSampler { channel_index })
}

fn first_cubic_quat(values: &[[Quat; 3]], channel_index: usize) -> Result<Quat, AnimError> {
    values
        .first()
        .map(|key| key[1])
        .ok_or(AnimError::InvalidSampler { channel_index })
}

fn first_morph_weights(values: &[Vec<f32>], channel_index: usize) -> Result<Vec<f32>, AnimError> {
    values
        .first()
        .cloned()
        .ok_or(AnimError::InvalidSampler { channel_index })
}

fn first_cubic_morph_weights(
    values: &[[Vec<f32>; 3]],
    channel_index: usize,
) -> Result<Vec<f32>, AnimError> {
    values
        .first()
        .map(|key| key[1].clone())
        .ok_or(AnimError::InvalidSampler { channel_index })
}

fn checked_vec3_step(
    values: &[Vec3],
    index: usize,
    channel_index: usize,
) -> Result<Vec3, AnimError> {
    values
        .get(index)
        .map(|_| sample_vec3_step(values, index))
        .ok_or(AnimError::InvalidSampler { channel_index })
}

fn checked_quat_step(
    values: &[Quat],
    index: usize,
    channel_index: usize,
) -> Result<Quat, AnimError> {
    values
        .get(index)
        .map(|_| sample_quat_step(values, index))
        .ok_or(AnimError::InvalidSampler { channel_index })
}

fn checked_vec3_linear(
    values: &[Vec3],
    index: usize,
    t: f32,
    channel_index: usize,
) -> Result<Vec3, AnimError> {
    values
        .get(index + 1)
        .map(|_| sample_vec3_linear(values, index, t))
        .ok_or(AnimError::InvalidSampler { channel_index })
}

fn checked_quat_linear(
    values: &[Quat],
    index: usize,
    t: f32,
    channel_index: usize,
) -> Result<Quat, AnimError> {
    values
        .get(index + 1)
        .map(|_| sample_quat_linear(values, index, t))
        .ok_or(AnimError::InvalidSampler { channel_index })
}

fn checked_cubic_vec3(
    values: &[[Vec3; 3]],
    times: &[f32],
    index: usize,
    t: f32,
    channel_index: usize,
) -> Result<Vec3, AnimError> {
    let current = values
        .get(index)
        .ok_or(AnimError::InvalidSampler { channel_index })?;
    let next = values
        .get(index + 1)
        .ok_or(AnimError::InvalidSampler { channel_index })?;
    let delta = times
        .get(index + 1)
        .zip(times.get(index))
        .map(|(next, current)| next - current)
        .ok_or(AnimError::InvalidSampler { channel_index })?;
    Ok(cubic_hermite_vec3(
        current[1],
        current[2] * delta,
        next[1],
        next[0] * delta,
        t,
    ))
}

fn checked_cubic_quat(
    values: &[[Quat; 3]],
    times: &[f32],
    index: usize,
    t: f32,
    channel_index: usize,
) -> Result<Quat, AnimError> {
    let current = values
        .get(index)
        .ok_or(AnimError::InvalidSampler { channel_index })?;
    let next = values
        .get(index + 1)
        .ok_or(AnimError::InvalidSampler { channel_index })?;
    let delta = times
        .get(index + 1)
        .zip(times.get(index))
        .map(|(next, current)| next - current)
        .ok_or(AnimError::InvalidSampler { channel_index })?;
    Ok(cubic_hermite_quat(
        current[1],
        current[2] * delta,
        next[1],
        next[0] * delta,
        t,
    ))
}

fn checked_morph_weights_step(
    values: &[Vec<f32>],
    index: usize,
    channel_index: usize,
) -> Result<Vec<f32>, AnimError> {
    values
        .get(index)
        .cloned()
        .ok_or(AnimError::InvalidSampler { channel_index })
}

fn checked_morph_weights_linear(
    values: &[Vec<f32>],
    index: usize,
    t: f32,
    channel_index: usize,
) -> Result<Vec<f32>, AnimError> {
    let left = values
        .get(index)
        .ok_or(AnimError::InvalidSampler { channel_index })?;
    let right = values
        .get(index + 1)
        .ok_or(AnimError::InvalidSampler { channel_index })?;
    if left.len() != right.len() {
        return Err(AnimError::InvalidSampler { channel_index });
    }
    Ok(left
        .iter()
        .zip(right)
        .map(|(left, right)| *left + (*right - *left) * t)
        .collect())
}

fn checked_cubic_morph_weights(
    values: &[[Vec<f32>; 3]],
    times: &[f32],
    index: usize,
    t: f32,
    channel_index: usize,
) -> Result<Vec<f32>, AnimError> {
    let current = values
        .get(index)
        .ok_or(AnimError::InvalidSampler { channel_index })?;
    let next = values
        .get(index + 1)
        .ok_or(AnimError::InvalidSampler { channel_index })?;
    let delta = times
        .get(index + 1)
        .zip(times.get(index))
        .map(|(next, current)| next - current)
        .ok_or(AnimError::InvalidSampler { channel_index })?;
    let [in_tangent, value, out_tangent] = current;
    let [next_in_tangent, next_value, _] = next;
    if value.len() != out_tangent.len()
        || value.len() != next_value.len()
        || value.len() != next_in_tangent.len()
        || value.len() != in_tangent.len()
    {
        return Err(AnimError::InvalidSampler { channel_index });
    }
    Ok(value
        .iter()
        .zip(out_tangent)
        .zip(next_value)
        .zip(next_in_tangent)
        .map(|(((p0, m0), p1), m1)| cubic_hermite_scalar(*p0, *m0 * delta, *p1, *m1 * delta, t))
        .collect())
}

fn cubic_hermite_scalar(p0: f32, m0: f32, p1: f32, m1: f32, t: f32) -> f32 {
    let t2 = t * t;
    let t3 = t2 * t;
    (2.0 * t3 - 3.0 * t2 + 1.0) * p0
        + (t3 - 2.0 * t2 + t) * m0
        + (-2.0 * t3 + 3.0 * t2) * p1
        + (t3 - t2) * m1
}

#[cfg(test)]
mod tests {
    use super::*;
    use rig_assets::{AnimationChannel, AnimationClip, ChannelProperty, KeyframeValues};
    use rig_math::Transform;
    use rig_scene::SceneGraph;

    fn approx_eq_vec3(left: Vec3, right: Vec3) {
        assert!(
            left.abs_diff_eq(right, 1e-5),
            "left={left:?} right={right:?}"
        );
    }

    fn sample_clip(
        target: &str,
        property: ChannelProperty,
        values: KeyframeValues,
        duration: f32,
        looping: bool,
    ) -> AnimationClip {
        AnimationClip {
            name: "clip".to_string(),
            duration,
            looping,
            channels: vec![AnimationChannel {
                target_node: target.to_string(),
                property,
                sampler: KeyframeSampler {
                    times: vec![0.0, duration],
                    interpolation: Interpolation::Linear,
                    values,
                },
            }],
        }
    }

    fn store_with_clip(clip: AnimationClip) -> (AssetStore, AnimationClipHandle) {
        let mut assets = AssetStore::new();
        let handle = assets.add_animation_clip(clip);
        (assets, handle)
    }

    fn scene_with_node(name: &str) -> (SceneGraph, NodeId) {
        let mut scene = SceneGraph::new();
        let node = scene.create_node(name);
        (scene, node)
    }

    #[test]
    fn advance_wraps_looping_clip() {
        let clip = sample_clip(
            "node",
            ChannelProperty::Translation,
            KeyframeValues::Translations(vec![Vec3::ZERO, Vec3::X]),
            2.0,
            true,
        );
        let (assets, handle) = store_with_clip(clip);
        let (scene, _node) = scene_with_node("node");
        let mut player = AnimationPlayer::new(handle);
        player.bind(&assets, &scene).unwrap();

        player.advance(2.5);

        assert!((player.time() - 0.5).abs() <= 1e-5);
    }

    #[test]
    fn advance_clamps_non_looping_clip() {
        let clip = sample_clip(
            "node",
            ChannelProperty::Translation,
            KeyframeValues::Translations(vec![Vec3::ZERO, Vec3::X]),
            2.0,
            false,
        );
        let (assets, handle) = store_with_clip(clip);
        let (scene, _node) = scene_with_node("node");
        let mut player = AnimationPlayer::new(handle);
        player.bind(&assets, &scene).unwrap();

        player.advance(3.0);

        assert!((player.time() - 2.0).abs() <= 1e-5);
        assert!(!player.is_playing());
    }

    #[test]
    fn advance_does_nothing_when_paused() {
        let clip = sample_clip(
            "node",
            ChannelProperty::Translation,
            KeyframeValues::Translations(vec![Vec3::ZERO, Vec3::X]),
            2.0,
            true,
        );
        let (assets, handle) = store_with_clip(clip);
        let (scene, _node) = scene_with_node("node");
        let mut player = AnimationPlayer::new(handle);
        player.bind(&assets, &scene).unwrap();
        player.pause();

        player.advance(1.0);

        assert_eq!(player.time(), 0.0);
    }

    #[test]
    fn speed_multiplies_delta() {
        let clip = sample_clip(
            "node",
            ChannelProperty::Translation,
            KeyframeValues::Translations(vec![Vec3::ZERO, Vec3::X]),
            4.0,
            true,
        );
        let (assets, handle) = store_with_clip(clip);
        let (scene, _node) = scene_with_node("node");
        let mut player = AnimationPlayer::new(handle);
        player.bind(&assets, &scene).unwrap();
        player.set_speed(2.0);

        player.advance(1.0);

        assert!((player.time() - 2.0).abs() <= 1e-5);
    }

    #[test]
    fn bind_resolves_known_nodes() {
        let clip = AnimationClip {
            name: "clip".to_string(),
            duration: 1.0,
            looping: true,
            channels: vec![
                AnimationChannel {
                    target_node: "a".to_string(),
                    property: ChannelProperty::Translation,
                    sampler: KeyframeSampler {
                        times: vec![0.0, 1.0],
                        interpolation: Interpolation::Linear,
                        values: KeyframeValues::Translations(vec![Vec3::ZERO, Vec3::X]),
                    },
                },
                AnimationChannel {
                    target_node: "b".to_string(),
                    property: ChannelProperty::Translation,
                    sampler: KeyframeSampler {
                        times: vec![0.0, 1.0],
                        interpolation: Interpolation::Linear,
                        values: KeyframeValues::Translations(vec![Vec3::ZERO, Vec3::Y]),
                    },
                },
            ],
        };
        let (assets, handle) = store_with_clip(clip);
        let mut scene = SceneGraph::new();
        let a = scene.create_node("a");
        let b = scene.create_node("b");
        let mut player = AnimationPlayer::new(handle);

        player.bind(&assets, &scene).unwrap();

        assert_eq!(player.binding, vec![Some(a), Some(b)]);
        assert_eq!(player.last_indices, vec![0, 0]);
    }

    #[test]
    fn bind_stores_none_for_unknown_nodes() {
        let clip = sample_clip(
            "missing",
            ChannelProperty::Translation,
            KeyframeValues::Translations(vec![Vec3::ZERO, Vec3::X]),
            1.0,
            true,
        );
        let (assets, handle) = store_with_clip(clip);
        let scene = SceneGraph::new();
        let mut player = AnimationPlayer::new(handle);

        player.bind(&assets, &scene).unwrap();

        assert_eq!(player.binding, vec![None]);
    }

    #[test]
    fn bind_returns_error_for_invalid_clip_handle() {
        let assets = AssetStore::new();
        let scene = SceneGraph::new();
        let mut player = AnimationPlayer::new(AnimationClipHandle::from_raw(99));

        assert!(matches!(
            player.bind(&assets, &scene),
            Err(AnimError::InvalidClip)
        ));
    }

    #[test]
    fn evaluate_writes_translation() {
        let clip = sample_clip(
            "node",
            ChannelProperty::Translation,
            KeyframeValues::Translations(vec![Vec3::ZERO, Vec3::new(2.0, 0.0, 0.0)]),
            2.0,
            true,
        );
        let (assets, handle) = store_with_clip(clip);
        let (mut scene, node) = scene_with_node("node");
        let mut player = AnimationPlayer::new(handle);
        player.bind(&assets, &scene).unwrap();
        player.set_time(1.0);

        player.evaluate(&assets, &mut scene).unwrap();

        approx_eq_vec3(scene.local_transform(node).unwrap().translation, Vec3::X);
    }

    #[test]
    fn evaluate_writes_rotation() {
        let end = Quat::from_rotation_y(std::f32::consts::FRAC_PI_2);
        let clip = sample_clip(
            "node",
            ChannelProperty::Rotation,
            KeyframeValues::Rotations(vec![Quat::IDENTITY, end]),
            2.0,
            true,
        );
        let (assets, handle) = store_with_clip(clip);
        let (mut scene, node) = scene_with_node("node");
        let mut player = AnimationPlayer::new(handle);
        player.bind(&assets, &scene).unwrap();
        player.set_time(1.0);

        player.evaluate(&assets, &mut scene).unwrap();

        let expected = Quat::from_rotation_y(std::f32::consts::FRAC_PI_4);
        assert!(
            scene
                .local_transform(node)
                .unwrap()
                .rotation
                .abs_diff_eq(expected, 1e-5)
        );
    }

    #[test]
    fn evaluate_writes_scale() {
        let clip = sample_clip(
            "node",
            ChannelProperty::Scale,
            KeyframeValues::Scales(vec![Vec3::ONE, Vec3::splat(3.0)]),
            2.0,
            true,
        );
        let (assets, handle) = store_with_clip(clip);
        let (mut scene, node) = scene_with_node("node");
        let mut player = AnimationPlayer::new(handle);
        player.bind(&assets, &scene).unwrap();
        player.set_time(1.0);

        player.evaluate(&assets, &mut scene).unwrap();

        approx_eq_vec3(scene.local_transform(node).unwrap().scale, Vec3::splat(2.0));
    }

    #[test]
    fn evaluate_stores_morph_weights() {
        let clip = sample_clip(
            "node",
            ChannelProperty::MorphTargetWeights,
            KeyframeValues::MorphWeights(vec![vec![0.0, 0.0], vec![1.0, 0.5]]),
            2.0,
            true,
        );
        let (assets, handle) = store_with_clip(clip);
        let (mut scene, node) = scene_with_node("node");
        let mut player = AnimationPlayer::new(handle);
        player.bind(&assets, &scene).unwrap();
        player.set_time(1.0);

        player.evaluate(&assets, &mut scene).unwrap();

        let weights = player.morph_weights(node).unwrap();
        assert_eq!(weights.len(), 2);
        assert!((weights[0] - 0.5).abs() <= 1.0e-5);
        assert!((weights[1] - 0.25).abs() <= 1.0e-5);
    }

    #[test]
    fn evaluate_accumulates_multiple_channels_for_same_node() {
        let clip = AnimationClip {
            name: "clip".to_string(),
            duration: 2.0,
            looping: true,
            channels: vec![
                AnimationChannel {
                    target_node: "node".to_string(),
                    property: ChannelProperty::Translation,
                    sampler: KeyframeSampler {
                        times: vec![0.0, 2.0],
                        interpolation: Interpolation::Linear,
                        values: KeyframeValues::Translations(vec![Vec3::ZERO, Vec3::X * 2.0]),
                    },
                },
                AnimationChannel {
                    target_node: "node".to_string(),
                    property: ChannelProperty::Scale,
                    sampler: KeyframeSampler {
                        times: vec![0.0, 2.0],
                        interpolation: Interpolation::Linear,
                        values: KeyframeValues::Scales(vec![Vec3::ONE, Vec3::splat(3.0)]),
                    },
                },
            ],
        };
        let (assets, handle) = store_with_clip(clip);
        let (mut scene, node) = scene_with_node("node");
        scene
            .set_local_transform(
                node,
                Transform {
                    translation: Vec3::ZERO,
                    rotation: Quat::from_rotation_z(0.25),
                    scale: Vec3::ONE,
                },
            )
            .unwrap();
        let initial_rotation = scene.local_transform(node).unwrap().rotation;
        let mut player = AnimationPlayer::new(handle);
        player.bind(&assets, &scene).unwrap();
        player.set_time(1.0);

        player.evaluate(&assets, &mut scene).unwrap();

        let transform = scene.local_transform(node).unwrap();
        approx_eq_vec3(transform.translation, Vec3::X);
        approx_eq_vec3(transform.scale, Vec3::splat(2.0));
        assert!(transform.rotation.abs_diff_eq(initial_rotation, 1e-5));
    }

    #[test]
    fn evaluate_skips_unbound_channels() {
        let clip = sample_clip(
            "missing",
            ChannelProperty::Translation,
            KeyframeValues::Translations(vec![Vec3::ZERO, Vec3::X]),
            1.0,
            true,
        );
        let (assets, handle) = store_with_clip(clip);
        let (mut scene, node) = scene_with_node("node");
        let mut player = AnimationPlayer::new(handle);
        player.bind(&assets, &scene).unwrap();

        player.evaluate(&assets, &mut scene).unwrap();

        assert_eq!(scene.local_transform(node).unwrap(), Transform::IDENTITY);
    }

    #[test]
    fn evaluate_returns_error_for_invalid_clip() {
        let clip = sample_clip(
            "node",
            ChannelProperty::Translation,
            KeyframeValues::Translations(vec![Vec3::ZERO, Vec3::X]),
            1.0,
            true,
        );
        let (assets, handle) = store_with_clip(clip);
        let (mut scene, _node) = scene_with_node("node");
        let mut player = AnimationPlayer::new(handle);
        player.bind(&assets, &scene).unwrap();
        let empty_assets = AssetStore::new();

        assert!(matches!(
            player.evaluate(&empty_assets, &mut scene),
            Err(AnimError::InvalidClip)
        ));
    }

    #[test]
    fn set_time_resets_hint_indices() {
        let clip = sample_clip(
            "node",
            ChannelProperty::Translation,
            KeyframeValues::Translations(vec![Vec3::ZERO, Vec3::X]),
            1.0,
            true,
        );
        let (assets, handle) = store_with_clip(clip);
        let (scene, _node) = scene_with_node("node");
        let mut player = AnimationPlayer::new(handle);
        player.bind(&assets, &scene).unwrap();
        player.last_indices = vec![7];

        player.set_time(0.0);

        assert_eq!(player.last_indices, vec![0]);
    }

    #[test]
    fn toggle_flips_playing_state() {
        let mut player = AnimationPlayer::new(AnimationClipHandle::from_raw(0));
        assert!(player.is_playing());
        player.toggle();
        assert!(!player.is_playing());
        player.toggle();
        assert!(player.is_playing());
    }
}
