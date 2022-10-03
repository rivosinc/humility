{
  lib,
  stdenv,
  rustPlatform,
  cargo,
  pkg-config,
  systemd ? null,
  libusb1,
  cargo-readme,
  openocd,
  qemu,
  src,
  version,
  doCheck ? false,
}:
rustPlatform.buildRustPackage rec {
  inherit src version doCheck;

  name = "humility";

  cargoSha256 = "sha256-/ez7pOAqkOlYnkE7mSoAzcErctpgbstqym0rlAsvqVw";

  nativeBuildInputs = [
    pkg-config
    cargo-readme
    cargo
  ];

  buildInputs = [
    libusb1
    cargo-readme
  ] ++ lib.optionals stdenv.isLinux [ systemd ];

  propagatedBuildInputs = [
  #  openocd
  #  qemu
  ];

  checkPhase = ''
    ${cargo}/bin/cargo fmt --all --check
    ${cargo}/bin/cargo clippy --profile=ci -- -D warnings
    ${cargo}/bin/cargo test
  '';

  meta = with lib; {
    description = "Humility is the debugger for Hubris";
    homepage = "https://github.com/oxidecomputer/humility";
    license = licenses.mpl20;
  };
}
