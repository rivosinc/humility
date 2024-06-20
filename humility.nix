{
  lib,
  stdenv,
  rustPlatform,
  cargo,
  pkg-config,
  systemd ? null,
  AppKit ? null,
  libusb1,
  cargo-readme,
  qemu,
  src,
  version,
  doCheck ? false,
}:
rustPlatform.buildRustPackage rec {
  inherit src version doCheck;

  name = "humility";

  cargoSha256 = "sha256-ecMEVTi1uFBZToyDhczzu6QuKzjcXI2so4draQKwXmk";

  nativeBuildInputs =
    [
      pkg-config
      cargo-readme
      cargo
    ]
    ++ lib.optionals stdenv.isDarwin [AppKit];

  buildInputs =
    [
      libusb1
      cargo-readme
    ]
    ++ lib.optionals stdenv.isLinux [systemd];

  checkInputs = [
    qemu
  ];

  checkPhase = ''
    ${cargo}/bin/cargo fmt --all --check
    ${cargo}/bin/cargo clippy --all --profile=ci -- -D warnings
    ${cargo}/bin/cargo test
  '';

  meta = with lib; {
    description = "Humility is the debugger for Hubris";
    homepage = "https://github.com/oxidecomputer/humility";
    license = licenses.mpl20;
  };
}
