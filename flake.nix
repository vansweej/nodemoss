{
  description = "rig — personal 3D & physics research framework in Rust";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    architecture-prompts = {
      url = "github:vansweej/architecture_prompts";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    nixgl = {
      url = "github:nix-community/nixGL";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    {
      self,
      nixpkgs,
      rust-overlay,
      architecture-prompts,
      nixgl,
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

          # nixGL requires allowUnfree to evaluate the NVIDIA wrapper package
          nixglPkgs = import nixpkgs {
            inherit system;
            config.allowUnfree = true;
            overlays = [ nixgl.overlay ];
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
            nixglPkgs.nixgl.auto.nixGLDefault
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

            # Audio (cpal / live-audio feature)
            alsa-lib
            pipewire # provides libpipewire-0.3.so.0 needed by the ALSA PipeWire plugin
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
              pkgs.git-lfs
              architecture-prompts.packages.${system}.default
            ]
            ++ pkgs.lib.optionals isLinux linuxNativeBuildInputs;

            buildInputs =
              pkgs.lib.optionals isLinux linuxBuildInputs ++ pkgs.lib.optionals isDarwin darwinBuildInputs;

            shellHook = pkgs.lib.optionalString isLinux ''
              # wgpu loads libvulkan.so.1 via dlopen at runtime
              export LD_LIBRARY_PATH="${pkgs.lib.makeLibraryPath buildInputs}:$LD_LIBRARY_PATH"
              # Vulkan validation layers for debug builds
              export VK_LAYER_PATH="${pkgs.vulkan-validation-layers}/share/vulkan/explicit_layer.d"
              # Point ALSA at the system PipeWire plugin so cpal can capture audio.
              # The Nix alsa-lib does not bundle libasound_module_pcm_pipewire.so;
              # the system copy (installed by pipewire-alsa) provides it.
              export ALSA_PLUGIN_DIR="/usr/lib/x86_64-linux-gnu/alsa-lib"
            '';
          };
        }
      );
    };
}
