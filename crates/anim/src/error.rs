//! Error types for animation playback.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum AnimError {
    #[error("invalid animation clip handle")]
    InvalidClip,
    #[error("invalid keyframe sampler for channel {channel_index}")]
    InvalidSampler { channel_index: usize },
    #[error("scene error: {0}")]
    Scene(#[from] rig_scene::SceneError),
}
