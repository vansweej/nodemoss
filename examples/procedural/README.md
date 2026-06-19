# Procedural

Procedural geometry — runtime mesh generation from implicit surface functions.

## Examples

| Example | Run command | Description | Key concepts |
|---------|-------------|-------------|--------------|
| `metaballs/` | `cargo run -p metaballs` | 4 bouncing metaballs rendered via marching cubes | DynamicMesh, field functions, Blinn-Phong, fly-camera |
| `voice_metaballs/` | `cargo run -p voice_metaballs` | Audio-reactive metaballs driven by live voice input | signal processing pipeline, real-time mesh update |

## Suggested order

1. Start with `metaballs` for the base implicit-surface technique
2. Then `voice_metaballs` extends it with live audio reactivity via the graphynx pipeline

## Notes

Run all examples from the workspace root:

```bash
cargo run -p metaballs
cargo run -p voice_metaballs
```

`voice_metaballs` depends on the external `graphynx` project, which is provided by
the Nix flake at `vendor/graphynx` (pinned in `flake.lock`). Enter the dev shell
with `nix develop --impure` to provision it automatically.
