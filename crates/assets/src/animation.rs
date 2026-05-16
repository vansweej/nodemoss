//! Animation clip asset types stored in [`AssetStore`](crate::AssetStore).

use rig_math::{Interpolation, Quat, Vec3};

/// Which transform property an animation channel drives.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChannelProperty {
    Translation,
    Rotation,
    Scale,
    MorphTargetWeights,
}

/// Keyframe values — the variant must match the channel's [`ChannelProperty`].
#[derive(Clone, Debug)]
pub enum KeyframeValues {
    Translations(Vec<Vec3>),
    Rotations(Vec<Quat>),
    Scales(Vec<Vec3>),
    /// Cubic spline: `[in_tangent, value, out_tangent]` per keyframe.
    CubicTranslations(Vec<[Vec3; 3]>),
    CubicRotations(Vec<[Quat; 3]>),
    CubicScales(Vec<[Vec3; 3]>),
    MorphWeights(Vec<Vec<f32>>),
    /// Cubic spline: `[in_tangent, value, out_tangent]` per keyframe.
    CubicMorphWeights(Vec<[Vec<f32>; 3]>),
}

/// Keyframe times and values for one animated property.
#[derive(Clone, Debug)]
pub struct KeyframeSampler {
    /// Keyframe timestamps in seconds, strictly increasing.
    pub times: Vec<f32>,
    /// Interpolation mode applied between keyframes.
    pub interpolation: Interpolation,
    /// Keyframe values — variant must match the channel's property.
    pub values: KeyframeValues,
}

/// One channel of an animation clip: targets a named node's transform property.
#[derive(Clone, Debug)]
pub struct AnimationChannel {
    /// Name of the target scene node. Resolved to a `NodeId` at bind time.
    pub target_node: String,
    /// Which transform property this channel drives.
    pub property: ChannelProperty,
    /// Keyframe data with interpolation mode.
    pub sampler: KeyframeSampler,
}

/// Immutable keyframe animation clip asset.
///
/// Stored in [`AssetStore`](crate::AssetStore) and referenced by
/// [`AnimationClipHandle`](crate::AnimationClipHandle).
#[derive(Clone, Debug)]
pub struct AnimationClip {
    /// Human-readable name for debugging and UI display.
    pub name: String,
    /// Total duration in seconds.
    pub duration: f32,
    /// Whether playback loops when it reaches `duration`.
    pub looping: bool,
    /// One channel per animated node property.
    pub channels: Vec<AnimationChannel>,
}
