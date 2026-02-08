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
        in
        {
          shelly.shells.default = {
            packages =
              with pkgs;
              [
                cargo-edit
                lld
                hyperfine
                graphviz
                (fenix.complete.withComponents [
                  "cargo"
                  "clippy"
                  "rust-src"
                  "rustc"
                  "rustfmt"
                  "miri"
                ])
                rust-analyzer-nightly
                cargo-fuzz
                cargo-edit
              ]
              ++ lib.optionals (!stdenv.isDarwin) [
                valgrind
              ];
          };
        };
    };
}
