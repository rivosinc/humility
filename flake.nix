{
  description = "Flake shell to open Hubris and Humility dev environment";
  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
  inputs.rust-overlay.url = "github:oxalica/rust-overlay";
  inputs.flake-utils.url = "github:numtide/flake-utils";

  outputs = {
    self,
    nixpkgs,
    rust-overlay,
    flake-utils,
  }:
    flake-utils.lib.eachDefaultSystem (system: let

      overlays = [ (import rust-overlay) ];

      pkgs = import nixpkgs {
        inherit system overlays;
      };

      rust = pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;

      humility-overlay = final: prev: {
        cargo = rust;
        rustc = rust;
        humility = final.callPackage ./default.nix {
          cargo = rust;
          src = self;
          version = "0.8.0";
        };
      };
  
      humility-overlays = [ humility-overlay ];
      humility-pkgs = import nixpkgs {
        inherit system;
        overlays = humility-overlays;
      };


    in {
      # TODO this need some work so it is tucked under a system derivation
      overlays = {
        default = humility-overlay;
        inherit humility-overlay;
      };

      packages = flake-utils.lib.flattenTree {
        humility = humility-pkgs.humility;
      };

      defaultPackage = humility-pkgs.humility;

      devShell = pkgs.mkShell {

        shellHook = ''
          export CARGO_HOME=$(pwd)/.cargo
          export PATH="$(pwd)/.cargo/bin:$PATH"
        '';

        nativeBuildInputs = with pkgs; [
          rust
          openocd
          libusb1
        ] ++ lib.optionals pkgs.stdenv.isLinux[ pkgs.systemd ];
      };
    });
}
