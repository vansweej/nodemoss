{
  description = "rig — personal 3D & physics research framework in Rust";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    {
      self,
      nixpkgs,
      rust-overlay,
    }:
    let
      supportedSystems = [
        "x86_64-linux"
        "aarch64-linux"
        "x86_64-darwin"
        "aarch64-darwin"
      ];
      forAllSystems = nixpkgs.lib.genAttrs supportedSystems;
    in
    {
      devShells = forAllSystems (
        system:
        let
          pkgs = import nixpkgs {
            inherit system;
            overlays = [ (import rust-overlay) ];
          };

          # Stable Rust: minimal profile + only the extensions we need
          rustToolchain = pkgs.rust-bin.stable.latest.minimal.override {
            extensions = [
              "clippy"
              "rustfmt"
              "rust-src"
              "rust-analyzer"
            ];
          };

          isDarwin = pkgs.stdenv.isDarwin;
          isLinux = pkgs.stdenv.isLinux;

          # --- Linux: Vulkan + X11 + Wayland for wgpu/winit ---
          linuxNativeBuildInputs = with pkgs; [
            pkg-config
          ];

          linuxBuildInputs = with pkgs; [
            # Vulkan runtime & debugging
            vulkan-loader
            vulkan-headers
            vulkan-tools
            vulkan-validation-layers

            # Wayland
            wayland
            wayland-protocols
            libxkbcommon

            # X11
            libx11
            libxcursor
            libxrandr
            libxi
            libxcb

            # EGL / OpenGL fallback (libglvnd provides libEGL, libGL)
            libglvnd
          ];

          # --- macOS: Apple SDK provides all frameworks (Metal, AppKit, QuartzCore, etc.) ---
          darwinBuildInputs = with pkgs; [
            apple-sdk
            libiconv
          ];

        in
        {
          default = pkgs.mkShell rec {
            nativeBuildInputs = [
              rustToolchain
              pkgs.cargo-tarpaulin
            ]
            ++ pkgs.lib.optionals isLinux linuxNativeBuildInputs;

            buildInputs =
              pkgs.lib.optionals isLinux linuxBuildInputs ++ pkgs.lib.optionals isDarwin darwinBuildInputs;

            shellHook = pkgs.lib.optionalString isLinux ''
              # wgpu loads libvulkan.so.1 via dlopen at runtime
              export LD_LIBRARY_PATH="${pkgs.lib.makeLibraryPath buildInputs}:$LD_LIBRARY_PATH"
              # Vulkan validation layers for debug builds
              export VK_LAYER_PATH="${pkgs.vulkan-validation-layers}/share/vulkan/explicit_layer.d"
            '';
          };
        }
      );
    };
}
