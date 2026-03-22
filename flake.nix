{
  description = "Gcmodule";
  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/release-25.11";
    fenix = {
      url = "github:nix-community/fenix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    flake-parts = {
      url = "github:hercules-ci/flake-parts";
      inputs.nixpkgs-lib.follows = "nixpkgs";
    };
    crane.url = "github:ipetkov/crane";
    shelly.url = "github:CertainLach/shelly";
  };
  outputs =
    inputs:
    inputs.flake-parts.lib.mkFlake { inherit inputs; } {
      imports = [ inputs.shelly.flakeModule ];
      systems = inputs.nixpkgs.lib.systems.flakeExposed;
      perSystem =
        {
          config,
          system,
          ...
        }:
        let
          pkgs = import inputs.nixpkgs {
            inherit system;
            overlays = [ inputs.fenix.overlays.default ];
          };
          toolchain = pkgs.fenix.combine [
            (pkgs.fenix.stable.withComponents [
              "cargo"
              "clippy"
              "rustc"
              "rust-src"
            ])
            pkgs.fenix.complete.rustfmt
          ];
          miriToolchain = pkgs.fenix.complete.withComponents [
            "cargo"
            "rustc"
            "rust-src"
            "miri"
          ];
          craneLib = (inputs.crane.mkLib pkgs).overrideToolchain toolchain;
          miriCraneLib = (inputs.crane.mkLib pkgs).overrideToolchain miriToolchain;
          src = craneLib.cleanCargoSource ./.;
        in
        {
          # Won't pass with the updated tree borrows implementation
          checks.miri = miriCraneLib.mkCargoDerivation ({
            inherit src;
            name = "miri-tests";
            buildPhaseCargoCommand = ''
              cargo miri test --features=nightly
            '';
            env.MIRIFLAGS = "-Zmiri-tree-borrows";

            cargoArtifacts = null;
            doInstallCargoArtifacts = false;

            MIRI_SYSROOT = miriCraneLib.mkCargoDerivation {
              pname = "miri-sysroot";
              version = "0.0.0";
              dontUnpack = true;
              buildPhaseCargoCommand = ''
                MIRI_SYSROOT=$out cargo miri setup
              '';
              dontFixup = true;
              cargoLock = pkgs.runCommand "sysroot-cargoLock" { nativeBuildInputs = [ miriToolchain ]; } ''
                cp `rustc --print sysroot`/lib/rustlib/src/rust/library/Cargo.lock $out
              '';
              cargoArtifacts = null;
            };
          });
          shelly.shells = {
            verification = {
              factory = miriCraneLib.devShell;
              packages =
                with pkgs;
                [
                  cargo-edit
                  lld
                  hyperfine
                  graphviz
                  rust-analyzer-nightly
                  cargo-fuzz
                  cargo-edit
                ]
                ++ lib.optionals (!stdenv.isDarwin) [
                  valgrind
                ];
            };
            default = {
              factory = craneLib.devShell;
              packages = with pkgs; [
                cargo-edit
                rust-analyzer-nightly
                cargo-edit
              ];
            };
          };
        };
    };
}
