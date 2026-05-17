# Animation

Skeleton animation and CPU linear blend skinning.

## Examples

| Example | Run command | Description | Key concepts |
|---------|-------------|-------------|--------------|
| `skeleton_demo/` | `cargo run -p skeleton_demo` | Rigid skeleton animation — procedural robot arm | AnimationPlayer, AnimationClip, keyframe sampling, binding table |
| `tentacle_demo/` | `cargo run -p tentacle_demo` | CPU skinning — 4-bone animated cylinder | SkinEvaluator, SkinAsset, SkinWeights, linear blend skinning |

## Suggested order

1. Start with `skeleton_demo` for rigid (non-deforming) skeleton animation
2. Then `tentacle_demo` adds per-vertex skinning with bone weights and inverse-transpose normals

## Notes

Run all examples from the workspace root:

```bash
cargo run -p skeleton_demo
cargo run -p tentacle_demo
```
