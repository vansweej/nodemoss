# Basics

Getting started — from raw wgpu usage to the first full framework application.

## Examples

| Example | Run command | Description | Key concepts |
|---------|-------------|-------------|--------------|
| `hello_triangle/` | `cargo run -p hello_triangle` | Minimal colored triangle using raw wgpu + winit | wgpu device/queue/surface, render pipeline, vertex buffer |
| `triangle_scenegraph/` | `cargo run -p triangle_scenegraph` | Same triangle rendered through the full framework | Application trait, SceneGraph, AssetStore, Renderer |
| `trackball_demo/` | `cargo run -p trackball_demo` | Interactive camera with trackball orbit and dolly | TrackBall, CameraRig, mouse input |

## Suggested order

1. Start with `hello_triangle` to understand the raw wgpu/winit foundation with no framework
2. Then `triangle_scenegraph` shows how the framework abstracts the same result
3. Finally `trackball_demo` introduces interactive camera controls used in all later examples

## Notes

Run all examples from the workspace root so that `assets/` resolves correctly:

```bash
cargo run -p hello_triangle
cargo run -p triangle_scenegraph
cargo run -p trackball_demo
```
