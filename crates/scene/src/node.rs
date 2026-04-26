//! Node identity, slot storage, and scene error types.

use rig_math::{BoundingSphere, Mat4, Transform};
use thiserror::Error;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct NodeId {
    pub(crate) index: u32,
    pub(crate) generation: u32,
}

impl NodeId {
    /// Construct a `NodeId` directly from raw index and generation values.
    pub fn from_raw(index: u32, generation: u32) -> Self {
        Self { index, generation }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VisibilityMode {
    Inherit,
    AlwaysVisible,
    Hidden,
}

#[derive(Clone, Debug)]
pub struct SceneNode {
    pub(crate) name: String,
    pub(crate) parent: Option<NodeId>,
    pub(crate) first_child: Option<NodeId>,
    pub(crate) next_sibling: Option<NodeId>,
    pub(crate) local_transform: Transform,
    pub(crate) world_transform: Mat4,
    pub(crate) world_bound: BoundingSphere,
    pub(crate) visibility: VisibilityMode,
}

impl SceneNode {
    pub(crate) fn new(name: String) -> Self {
        Self {
            name,
            parent: None,
            first_child: None,
            next_sibling: None,
            local_transform: Transform::IDENTITY,
            world_transform: Mat4::IDENTITY,
            world_bound: BoundingSphere::ZERO,
            visibility: VisibilityMode::Inherit,
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct NodeSlot {
    pub(crate) generation: u32,
    pub(crate) node: Option<SceneNode>,
}

#[derive(Debug, Error)]
pub enum SceneError {
    #[error("invalid node handle")]
    InvalidNode,
    #[error("cannot attach a node to itself")]
    SelfParent,
    #[error("attaching parent to its own descendant would create a cycle")]
    CycleDetected,
    #[error("missing mesh asset for renderable node")]
    MissingMeshAsset,
    #[error("node does not have a camera component")]
    NotACamera,
}

pub type Result<T> = std::result::Result<T, SceneError>;
