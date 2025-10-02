let
  inherit (atom) pkgs;
  toolchain = atom.fenix.fromToolchainFile { file = "${mod}/rust-toolchain.toml"; };
in
pkgs.mkShell.override { stdenv = pkgs.clangStdenv; } {
  RUST_SRC_PATH = "${toolchain}/lib/rustlib/src/rust/library";
  packages =
    with pkgs;
    [
      treefmt
      npins
      nixfmt-rfc-style
      shfmt
      taplo
      nodePackages.prettier
      atom.fenix.default.rustfmt
      toolchain
      mold
      cargo-insta
      cargo-shear
    ]
    ++ pkgs.lib.optionals pkgs.stdenv.isDarwin [
      darwin.apple_sdk.frameworks.SystemConfiguration
      libiconv
    ];
}
