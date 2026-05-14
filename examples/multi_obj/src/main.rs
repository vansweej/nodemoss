//! Multi-OBJ cache demo.
//!
//! Imports the same textured OBJ three times through one local importer. The
//! texture dependency is registered once thanks to the importer cache.
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
    run_loading_example(ExampleKind::MultiObj)
}
