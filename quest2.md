# Running nodemoss on Meta Quest 2

Research notes from brainstorming session — April 2026.

## TL;DR

**Yes, it's technically possible.** The core crates (`rig-math`, `rig-scene`,
`rig-assets`, `rig-render`) are platform-agnostic and would work on Quest 2
hardware. The blockers are the windowing/session layer (`rig-gpu`, `rig-app`),
which are tightly coupled to `winit`. Quest 2 requires OpenXR for VR session
management instead.

---

## Quest 2 hardware

| Spec            | Value                                  |
|-----------------|----------------------------------------|
| SoC             | Qualcomm Snapdragon XR2                |
| GPU             | Adreno 650                             |
| Graphics APIs   | OpenGL ES 3.2, **Vulkan 1.1**         |
| Display         | 1832×1920 per eye, 72/90/120 Hz       |
| OS              | Android 10 (custom Meta fork)          |
| VR Runtime      | OpenXR (Meta's implementation)         |

The Vulkan 1.1 support is the key enabler — wgpu uses Vulkan as its backend
on Android.

---

## What's already Quest-compatible

### Portable crates (no windowing dependencies)

- **`rig-math`** — pure glam math, works anywhere.
- **`rig-scene`** — arena-based scene graph with generational `NodeId` handles.
  No platform ties.
- **`rig-assets`** — immutable `MeshAsset`, `MaterialAsset`, `ShaderAsset`.
  Pure data, fully portable.
- **`rig-render`** — the renderer uses wgpu abstractions internally. The
  rendering logic (pipeline creation, bind groups, draw calls) is
  platform-agnostic. Would need minor changes for stereo rendering.

### Conservative GPU limits

`GpuContext::new()` in `crates/gpu/src/lib.rs` (line 102) already requests:

```rust
required_limits: wgpu::Limits::downlevel_webgl2_defaults()
    .using_resolution(adapter.limits()),
```

This is conservative enough for mobile GPUs like the Adreno 650.

### WGSL shaders

All shaders are WGSL, which wgpu compiles to SPIR-V for Vulkan. No
platform-specific shader work needed.

---

## What blocks it

### 1. `rig-gpu` is coupled to winit windows

`GpuContext` in `crates/gpu/src/lib.rs` takes `Arc<winit::window::Window>`:

```rust
// line 81
pub async fn new(window: Arc<Window>) -> Result<Self> {
```

It creates a wgpu surface from the window (line 85), and owns the swapchain.

**On Quest 2, there is no window.** OpenXR owns:
- The VR session lifecycle
- Swapchain image allocation (one set per eye)
- Frame timing (`wait_frame` / `begin_frame` / `end_frame`)
- The Vulkan instance and device (or at minimum, requires specific Vulkan
  extensions)

### 2. `rig-app` runner is winit-only

`Runner` in `crates/app/src/runner.rs` (line 39–53) implements
`winit::ApplicationHandler`. The entire event loop, input handling, and frame
cadence is winit-driven.

VR requires:
- OpenXR's frame loop (`wait_frame` → `begin_frame` → `end_frame`)
- Input from OpenXR action sets (controllers, hand tracking), not
  keyboard/mouse
- Session state management (focused, visible, stopping, etc.)

### 3. Stereo rendering

The current renderer outputs to **one swapchain texture** per frame. VR
requires rendering **two views** (one per eye) with different
projection/view matrices each frame, at 72–90 Hz.

The `Frame` struct (line 44–51 in `crates/gpu/src/lib.rs`) holds a single
`view: wgpu::TextureView`. For VR, you need two views (or a single wide
texture with two viewports).

### 4. Android cross-compilation

Quest 2 runs Android. Building requires:
- Cross-compiling for `aarch64-linux-android` using the Android NDK
- Packaging as an APK with NativeActivity entry point
- The `cargo-apk` or `xbuild` tools to automate this
- Nix flake changes to include the Android NDK toolchain

---

## Directions explored

### Direction 1: OpenXR backend as a parallel runner

Add a `rig-xr` crate that **replaces** `rig-gpu` + `rig-app` for VR. It uses
the [`openxr` crate](https://github.com/Ralith/openxrs) to manage the VR
session, and creates wgpu devices from the Vulkan instance that OpenXR
provides.

Scene graph, assets, and render crate stay unchanged — just wired to a
different frame source.

**Trade-off:** Clean separation and preserves desktop path. Requires
abstracting `GpuContext` into a trait so both winit and OpenXR paths can share
the renderer.

**Key changes needed:**
- Extract a `GpuBackend` trait from `GpuContext` (device, queue, frame
  acquisition)
- `rig-xr` implements this trait using OpenXR + Vulkan
- `rig-app` keeps winit; a new `rig-xr-app` provides the VR runner
- Renderer needs a "render to provided texture view" mode for stereo

### Direction 2: PCVR streaming first (SteamVR / ALVR)

Instead of running natively on Quest, run the engine on your **desktop** and
stream to Quest 2 via ALVR or SteamVR Link.

You'd still need OpenXR integration, but on the desktop side (SteamVR
provides an OpenXR runtime for Linux). This skips Android cross-compilation
entirely and uses your existing desktop GPU.

**Trade-off:** Much easier first step with lower latency to results. Adds
display latency and requires Wi-Fi / USB connection to PC. Not standalone.

### Direction 3: Android flat-screen app first

Before tackling VR, get the engine running as a **flat 2D Android app** on
Quest 2 (it runs Android and can run non-VR apps).

Use winit's Android support (`android-activity` crate) to cross-compile for
`aarch64-linux-android`. This proves the wgpu + Vulkan path works on Quest
hardware without any OpenXR complexity.

**Trade-off:** Cheaply validates the mobile GPU path (shaders compile, limits
are sufficient, textures fit in memory). Doesn't give you VR — it's a
stepping stone.

### Direction 4: Study Bevy's OpenXR plumbing

The Bevy community has experimented with OpenXR integration. Rather than
building your own XR runtime layer from scratch, study or borrow from
projects like `bevy_openxr` to understand the Vulkan ↔ OpenXR ↔ wgpu
interop.

The tricky part is creating a wgpu device from an OpenXR-provided Vulkan
instance — this requires wgpu's unsafe "from raw" APIs.

**Trade-off:** Leverages community work. But Bevy's architecture (ECS) is
very different from this project's scene graph, so it's reference material,
not drop-in code.

---

## Architectural sketch (Direction 1)

If pursuing the OpenXR backend, the crate graph would extend like this:

```
rig-math
  ^
rig-scene         rig-assets
  ^                 ^
rig-gpu  ──────── rig-xr          (new: OpenXR session, Vulkan interop)
  ^                 ^
rig-render ────────┘
  ^
rig-overlay
  ^
rig-app          rig-xr-app       (new: VR runner, OpenXR frame loop)
  ^                 ^
examples/        vr-examples/
```

The key abstraction point is `rig-gpu`. Today it does two things:
1. **Device management** — creating wgpu device/queue (portable)
2. **Frame management** — acquiring swapchain textures from a window (not
   portable)

Splitting these concerns would let `rig-xr` provide frames from OpenXR
swapchains while reusing the same device/queue/renderer.

### Stereo rendering approach

The simplest approach for your renderer:

```
for eye in [LEFT, RIGHT] {
    let view_matrix = openxr_view[eye].pose;
    let proj_matrix = openxr_view[eye].fov;
    let target = openxr_swapchain[eye].acquire_image();

    // Existing render path, just with different camera + target
    renderer.render_scene(scene, assets, view_matrix, proj_matrix, target);
}
```

Your existing `extract_renderables_culled` and scene traversal would run
once per eye, or once with a frustum union and then per-eye draw submission.

---

## Key Rust crates needed

| Crate                | Purpose                                    | URL                                         |
|----------------------|--------------------------------------------|---------------------------------------------|
| `openxr`             | OpenXR bindings (session, swapchain, input)| https://github.com/Ralith/openxrs           |
| `openxr-sys`         | Raw OpenXR FFI types                       | (dependency of `openxr`)                    |
| `ash`                | Raw Vulkan bindings (for interop)          | https://github.com/ash-rs/ash               |
| `wgpu::hal`          | wgpu's HAL layer for unsafe Vulkan interop | (part of wgpu)                              |
| `android-activity`   | Android NativeActivity glue                | https://github.com/rust-mobile/android-activity |
| `ndk` / `ndk-glue`   | Android NDK bindings                       | https://github.com/rust-mobile/ndk          |

---

## Open questions

1. **wgpu from OpenXR Vulkan instance** — Can wgpu create a device from an
   externally-provided Vulkan instance/device? This requires `wgpu::hal`
   unsafe APIs. How mature is this path?

2. **Android NDK in Nix** — The current flake uses nixGL for desktop GPU
   access. Adding Android NDK cross-compilation to the Nix flake is
   non-trivial.

3. **Performance budget** — Quest 2 needs 72 fps × 2 eyes = 144 renders/sec
   effective. The Adreno 650 is capable but the geometry/shader budget is
   much tighter than desktop.

4. **Overlay system** — `rig-overlay` uses glyphon for text. Does glyphon
   work on Android/Vulkan? Would the overlay render to a world-space quad
   in VR instead of a screen-space HUD?

5. **Controller input mapping** — The current `InputState` handles
   keyboard/mouse. VR controllers use OpenXR action sets with spatial poses.
   This is a completely different input model.

6. **glam compatibility** — OpenXR uses its own `Posef`, `Quaternionf`,
   `Vector3f` types. Need conversion utilities to/from glam types.

---

## Recommended next steps

1. **Spike: flat Android build** (Direction 3) — Prove wgpu + Vulkan works
   on Quest hardware. Minimal effort, high signal.

2. **Spike: PCVR via SteamVR** (Direction 2) — If you have SteamVR on
   Linux, try the OpenXR desktop path first. Avoids Android complexity.

3. **Abstract `GpuContext`** — Extract the frame-acquisition interface into
   a trait. This is useful regardless of VR — it also enables offscreen
   rendering and testing.

4. **Add `rig-xr` crate** (Direction 1) — Once the abstraction exists,
   implement the OpenXR backend.

---

## References

- OpenXR Rust bindings: https://github.com/Ralith/openxrs
- OpenXR crate docs: https://docs.rs/openxr/0.21.1/openxr/
- wgpu repository: https://github.com/gfx-rs/wgpu
- Android activity crate: https://github.com/rust-mobile/android-activity
- Quest 2 Vulkan support: Vulkan 1.1 via Adreno 650
