pkgs:
let
  inherit (mod) toolchain;

in
pkgs.mkShell.override { stdenv = pkgs.clangStdenv; } {
  RUST_SRC_PATH = "${toolchain}/lib/rustlib/src/rust/library";
  PROTO_ROOT = mod.protos;
  SNIX_BUILD_SANDBOX_SHELL = "/bin/sh";
  packages =
    with pkgs;
    [
      treefmt
      npins
      nixfmt-rfc-style
      shfmt
      taplo
      zstd
      pkg-config
      nodePackages.prettier
      mod.fenix.default.rustfmt
      crate2nix
      nil
      toolchain
      mold
      protobuf
      cargo-insta
      cargo-shear
    ]
    ++ pkgs.lib.optionals pkgs.stdenv.isDarwin [
      apple-sdk
      libiconv
    ];
}
