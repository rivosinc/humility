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
  src,
  version,
  doCheck ? false,
}:
rustPlatform.buildRustPackage rec {
  inherit src version doCheck;

  name = "humility";

  cargoSha256 = "sha256-dsS0NNAaRnnJHcWzUwDrCcl28F/tZI6EmM7M1fUOJwU";

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
