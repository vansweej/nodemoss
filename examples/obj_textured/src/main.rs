//! Textured OBJ loading demo.
//!
//! Loads `assets/models/textured_cube.obj`, resolves its MTL diffuse texture,
//! and renders the registered texture through the standard material path.
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
    run_loading_example(ExampleKind::ObjTextured)
}
