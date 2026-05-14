//! Runtime texture loading demo.
//!
//! Loads `assets/textures/checker.png` from disk and applies it to a procedural
//! sphere to isolate the texture import path from OBJ loading.
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

include!("../../shared_loading_example.rs");

fn main() -> anyhow::Result<()> {
    env_logger::init();
    run_loading_example(ExampleKind::TextureLoad)
}
