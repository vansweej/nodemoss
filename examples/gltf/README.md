# glTF

glTF 2.0 model loading — static PBR materials and runtime CPU skinning.

## Examples

| Example | Run command | Description | Key concepts |
|---------|-------------|-------------|--------------|
| `demo/` | `cargo run -p gltf_demo` | Static/PBR model viewer (DamagedHelmet.glb) | rig-gltf, PBR material mapping, cameras, lights, multi-scene |
| `skinned/` | `cargo run -p gltf_skinned_demo` | CPU skinning runtime validation (BrainStem.glb) | skin descriptors, joint transforms, morph target loading |

## Suggested order

1. Start with `demo` for static model loading with full PBR materials
2. Then `skinned` adds runtime skeletal animation via CPU skinning descriptors

## Notes

Run all examples from the workspace root:

```bash
cargo run -p gltf_demo
cargo run -p gltf_skinned_demo
```

glTF assets are stored under `assets/gltf/` and tracked via Git LFS.
Run `git lfs pull` if models appear missing.
