//! OBJ loading demo.
//!
//! Loads a hand-authored geometry-only cube from `assets/models/cube.obj` using
//! `rig-import`, generates smooth normals, and renders it with the Phong shader.
//!
//! # Controls
//!
//! | Key(s)     | Action                      |
//! |------------|-----------------------------|
//! | W / S      | Move forward / backward     |
//! | A / D      | Strafe left / right         |
//! | Q / E      | Move up / down              |
//! | Arrow keys | Rotate camera               |
//! | Escape     | Close window                |

use example_shared::{ExampleKind, run_loading_example};

fn main() -> anyhow::Result<()> {
    env_logger::init();
    run_loading_example(ExampleKind::ObjLoad)
}
