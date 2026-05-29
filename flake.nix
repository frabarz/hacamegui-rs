{
  description = "hacamegui-rs: GUI for HACAM";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-parts.url = "github:hercules-ci/flake-parts";
    rust-overlay.url = "github:oxalica/rust-overlay";
    crane.url = "github:ipetkov/crane";
  };

  outputs = inputs @ {
    flake-parts,
    rust-overlay,
    crane,
    ...
  }:
    flake-parts.lib.mkFlake {inherit inputs;} {
      systems = ["x86_64-linux" "aarch64-linux"];

      perSystem = {system, ...}: let
        # Use rust-overlay for the toolchain (needed for stable Rust ≥1.85 for edition 2024)
        pkgs = import inputs.nixpkgs {
          inherit system;
          overlays = [rust-overlay.overlays.default];
        };
        rustToolchain = pkgs.rust-bin.stable.latest.default.override {
          extensions = ["rust-src"];
        };
        craneLib = (crane.mkLib pkgs).overrideToolchain rustToolchain;

        commonArgs = {
          # Custom source filter: crane's default cleanCargoSource excludes .wgsl shader files
          src = pkgs.lib.cleanSourceWith {
            src = ./.;
            filter = path: type: let
              base = baseNameOf path;
            in
              if type == "directory"
              then !(builtins.elem base ["target" ".git" "result"])
              else builtins.match ".*\.(rs|toml|lock|wgsl)$" base != null;
          };
          # libclang needed so bindgen (used by some crate deps) can find libclang.so
          nativeBuildInputs = with pkgs; [pkg-config clang libclang];
          # buildInputs are linked at compile time (do NOT need LD_LIBRARY_PATH)
          buildInputs = with pkgs; [
            vulkan-loader
            libxkbcommon
            wayland
            libGL
            libX11
            libXcursor
            libXi
            libXrandr
            udev
            alsa-lib
          ];
          # Separate env var for bindgen — it dlopen's libclang at build time
          LIBCLANG_PATH = "${pkgs.libclang.lib}/lib";
        };

        cargoArtifacts = craneLib.buildDepsOnly commonArgs;
      in {
        packages.default = craneLib.buildPackage (commonArgs
          // {
            inherit cargoArtifacts;
            nativeBuildInputs = commonArgs.nativeBuildInputs ++ [pkgs.makeWrapper];
            # wrapProgram creates a wrapper that sets runtime environment.
            # Many Rust GUI libs (winit, wgpu, rfd) dlopen shared libraries,
            # so RPATH from buildInputs is insufficient — need LD_LIBRARY_PATH.
            postInstall = ''
              wrapProgram $out/bin/hacamegui-rs \
                --prefix LD_LIBRARY_PATH : ${pkgs.lib.makeLibraryPath (with pkgs; [vulkan-loader libxkbcommon])} \
                --set XKB_CONFIG_ROOT ${pkgs.xkeyboard_config}/share/X11/xkb \
                --prefix PATH : ${pkgs.lib.makeBinPath (with pkgs; [xkbcomp zenity])} \
                --unset WAYLAND_DISPLAY
            '';
          });
        devShells.default = craneLib.devShell {
          packages = with pkgs; [rustToolchain cargo-watch];
        };
      };
    };
}
