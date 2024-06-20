{
  description = "Flake shell to open Humility dev environment";
  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
  inputs.flake-utils.url = "github:numtide/flake-utils";

  inputs.rust-overlay.url = "github:oxalica/rust-overlay";
  inputs.rust-overlay.inputs.nixpkgs.follows = "nixpkgs";
  inputs.rust-overlay.inputs.flake-utils.follows = "flake-utils";

  inputs.qemuflake.url = "git+https://github.com/rivosinc/qemu?submodules=1&ref=dev/drew/nix";
  inputs.qemuflake.inputs.nixpkgs.follows = "nixpkgs";

  inputs.pre-commit-hooks.url = "github:cachix/pre-commit-hooks.nix";
  inputs.pre-commit-hooks.inputs.flake-utils.follows = "flake-utils";
  inputs.pre-commit-hooks.inputs.nixpkgs.follows = "nixpkgs";

  outputs = {
    self,
    nixpkgs,
    qemuflake,
    rust-overlay,
    pre-commit-hooks,
    flake-utils,
  }: (flake-utils.lib.eachDefaultSystem (system: let
    overlays = [(import rust-overlay)] ++ nixpkgs.lib.optional (system != "aarch64-darwin") qemuflake.overlays.default;

    pkgs = import nixpkgs {
      inherit system overlays;
    };

    rust = pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;

    humility-overlay = final: prev: {
      cargo = rust;
      rustc = rust;
      humility = final.callPackage ./humility.nix {
        qemu = pkgs.qemu;
        cargo = rust;
        src = self;
        version = "0.9.5";
        inherit (prev.darwin.apple_sdk.frameworks) AppKit;
      };
    };

    humility-overlays = [humility-overlay];
    humility-pkgs = import nixpkgs {
      inherit system;
      overlays = humility-overlays;
    };
    pre-commit-checks = pre-commit-hooks.lib.${system}.run {
      src = pkgs.lib.cleanSource ./.;
      hooks = {
        cargofmt = {
          enable = true;
          name = "cargo fmt";
          entry = "${rust}/bin/cargo fmt --check --all";
          files = "\\.rs$";
          pass_filenames = false;
        };
      };
    };
  in {
    packages = flake-utils.lib.flattenTree {
      humility = humility-pkgs.humility;
      default = humility-pkgs.humility;
    };

    devShells.default = pkgs.mkShell {
      shellHook = ''
        export CARGO_HOME=$(pwd)/.cargo
        export PATH="$(pwd)/.cargo/bin:$PATH"
        ${pre-commit-checks.shellHook}
      '';

      nativeBuildInputs = with pkgs;
        [
          rust
          cargo-readme
          openocd
          libusb1
        ]
        # this is needed for libudev
        ++ lib.optionals pkgs.stdenv.isLinux [pkgs.systemd]
        ++ lib.optionals stdenv.isDarwin [darwin.apple_sdk.frameworks.AppKit];
    };

    checks = {
      humility = humility-pkgs.humility.override {doCheck = true;};
    };
  }));
}
