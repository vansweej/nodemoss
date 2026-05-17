//! Asset loading showcase.
//!
//! Combines OBJ, texture, and runtime shader loading in one scene and reports a
//! startup summary through the debug HUD.
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
    run_loading_example(ExampleKind::AssetShowcase)
}
