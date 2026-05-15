//! Animation playback for the rig framework.
//!
//! Owns [`AnimationPlayer`] — the per-instance playback controller that samples
//! [`rig_assets::AnimationClip`] assets and writes transforms into the scene graph.

mod error;
mod player;

pub use error::AnimError;
pub use player::AnimationPlayer;
