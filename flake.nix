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
    graphynx = {
      url = "github:vansweej/graphynx";
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
      graphynx,
    }:
    let
      supportedSystems = [
        "x86_64-linux"
        "aarch64-linux"
        "x86_64-darwin"
        "aarch64-darwin"
      ];
      forAllSystems = nixpkgs.lib.genAttrs supportedSystems;

      # ------------------------------------------------------------------
      # Workspace member enumeration (system-independent)
      # ------------------------------------------------------------------
      # Read the workspace Cargo.toml to discover all member packages.
      workspaceCargoToml = builtins.fromTOML (builtins.readFile ./Cargo.toml);

      # Expand a member pattern like "crates/*" or "examples/shared" into a
      # list of concrete relative paths (e.g. "crates/math", "examples/shared").
      expandMemberPattern = pattern:
        let
          # Does the pattern end with "/*"?
          m = builtins.match "(.+)/\\*" pattern;
        in
          if m != null then
            let
              dir = builtins.head m;
              fullDir = toString ./. + "/${dir}";
              entries = if builtins.pathExists fullDir then builtins.readDir fullDir else {};
              subdirs = builtins.filter (n: entries.${n} == "directory") (builtins.attrNames entries);
            in
              builtins.map (sub: "${dir}/${sub}") subdirs
          else
            [ pattern ];

      memberPaths = builtins.concatMap expandMemberPattern workspaceCargoToml.workspace.members;

      # Read a workspace member's Cargo.toml and return { name, path, hasBinary }.
      readMemberInfo = path:
        let
          cargoToml = builtins.fromTOML (builtins.readFile (toString ./. + "/${path}/Cargo.toml"));
          name = cargoToml.package.name;
          hasBinary = builtins.pathExists (toString ./. + "/${path}/src/main.rs");
        in
          { inherit name path hasBinary; };

      allMemberInfos = builtins.map readMemberInfo memberPaths;

      # Only create separate Nix package outputs for members with binary targets.
      # Library-only crates are compiled implicitly as dependencies.
      binaryMembers = builtins.filter (m: m.hasBinary) allMemberInfos;

      # ------------------------------------------------------------------
      # perSystem — shared per-platform environment for devShells & packages
      # ------------------------------------------------------------------
      perSystem = system:
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

          rustPlatform = pkgs.makeRustPlatform {
            cargo = rustToolchain;
            rustc = rustToolchain;
          };

          # Build a single workspace member (or the whole workspace) as a
          # Nix package.  All builds are sandbox-only (no GPU/display), so
          # doCheck is off — unit tests must run in the dev shell or on
          # hardware CI.
          #
          # Usage:
          #   mkRigPackage { pname = "voice_metaballs"; }
          #   mkRigPackage { pname = "workspace"; cargoBuildFlags = [ "--workspace" ]; }
          mkRigPackage =
            { pname
            , cargoBuildFlags ? [ "--package" pname ]
            }:
            rustPlatform.buildRustPackage {
              inherit pname cargoBuildFlags;
              version = "0.1.0";
              src = ./.;
              cargoLock.lockFile = ./Cargo.lock;
              doCheck = false;

              nativeBuildInputs = pkgs.lib.optionals isLinux [ pkgs.pkg-config ];

              buildInputs =
                pkgs.lib.optionals isLinux [ pkgs.alsa-lib ]
                ++ pkgs.lib.optionals isDarwin [ pkgs.libiconv pkgs.apple-sdk ];

              postPatch = ''
                mkdir -p vendor
                cp -r ${graphynx} vendor/graphynx
              '';
            };
        in
        {
          inherit pkgs rustToolchain isDarwin isLinux rustPlatform mkRigPackage;
        };
    in
    {
      devShells = forAllSystems (
        system:
        let
          s = perSystem system;
          pkgs = s.pkgs;

          # nixGL requires allowUnfree to evaluate the NVIDIA wrapper package
          nixglPkgs = import nixpkgs {
            inherit system;
            config.allowUnfree = true;
            overlays = [ nixgl.overlay ];
          };

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
              s.rustToolchain
              pkgs.cargo-tarpaulin
              pkgs.git-lfs
              architecture-prompts.packages.${system}.default
            ]
            ++ pkgs.lib.optionals s.isLinux linuxNativeBuildInputs;

            buildInputs =
              pkgs.lib.optionals s.isLinux linuxBuildInputs ++ pkgs.lib.optionals s.isDarwin darwinBuildInputs;

            shellHook =
              ''
                # Configure local LFS filters and fetch any un-smudged objects.
                # This is idempotent — on a hot shell re-entry it completes instantly.
                git lfs install --local --skip-smudge 2>/dev/null || true
                git lfs pull 2>/dev/null || true

                # Provide graphynx at a stable in-workspace path, pinned by flake.lock.
                # Use the flake input (Nix store) — no fallback; the sibling checkout
                # approach caused Cargo workspace confusion.
                mkdir -p vendor
                ln -sfn "${graphynx}" vendor/graphynx
              ''
              + pkgs.lib.optionalString s.isLinux ''
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

      # ------------------------------------------------------------------
      # packages — sandbox-buildable workspace members
      # ------------------------------------------------------------------
      packages = forAllSystems (
        system:
        let
          s = perSystem system;

          # Auto-generate per-package outputs for every binary workspace member.
          perPackage = builtins.listToAttrs (builtins.map (m: {
            name = m.name;
            value = s.mkRigPackage { pname = m.name; };
          }) binaryMembers);
        in
        perPackage // {
          # Build the whole workspace in one derivation.
          workspace = s.mkRigPackage {
            pname = "rig-workspace";
            cargoBuildFlags = [ "--workspace" ];
          };

          default = self.packages.${system}.workspace;
        }
      );
    };
}
