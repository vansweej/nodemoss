//! Runtime shader loading demo.
//!
//! Loads WGSL source from `assets/shaders/phong.wgsl` rather than embedding a
//! Rust string constant, then renders a procedural cube.
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
    run_loading_example(ExampleKind::ShaderLoad)
}
