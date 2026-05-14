//! Texture format loading demo.
//!
//! Loads PNG, JPEG, and TGA files side-by-side on three quads. Run
//! `cargo run -p gen_test_textures` once to generate the binary assets.
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
    run_loading_example(ExampleKind::TextureFormats)
}
