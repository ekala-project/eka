let
  inherit (atom) pkgs;
  toolchain = atom.fenix.fromToolchainFile { file = "${mod}/rust-toolchain.toml"; };

  protos = pkgs.fetchFromGitHub {
    owner = "nrdxp";
    repo = "snix";
    rev = "protos";
    hash = "sha256-1ZsRIY/n8r9eJvRRO71+cENsdwTolVH7ACWUr0cfncI=";
  };
in
pkgs.mkShell.override { stdenv = pkgs.clangStdenv; } {
  RUST_SRC_PATH = "${toolchain}/lib/rustlib/src/rust/library";
  PROTO_ROOT = protos;
  SNIX_BUILD_SANDBOX_SHELL = "/bin/sh";
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
      protobuf
      cargo-insta
      cargo-shear
    ]
    ++ pkgs.lib.optionals pkgs.stdenv.isDarwin [
      darwin.apple_sdk.frameworks.SystemConfiguration
      darwin.apple_sdk.frameworks.DiskArbitration
      libiconv
    ];
}
