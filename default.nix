{
  lib,
  rustPlatform,
  cargo,
  pkg-config,
  systemd,
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
    # TODO check islinux system for systemd
    systemd
    libusb1
    cargo-readme
  ];

  propagatedBuildInputs = [
  #  openocd
  #  qemu
  ];

  # buildPhase = ''
  #  ${cargo}/bin/cargo build
  # '';

  # installPhase = ''
  #   ${cargo}/bin/cargo install --path .
  # '';

  meta = with lib; {
    description = "Humility is the debugger for Hubris";
    homepage = "https://github.com/oxidecomputer/humility";
    license = licenses.mpl20;
  };
}
