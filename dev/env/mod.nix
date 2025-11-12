let
  inherit (deps) pins;
  system = cfg.platform;
in
{
  Shell = mod.shell mod.pkgs;
  Static = mod.shell mod.pkgs.pkgsStatic;
  Pkgs = mod.pkgs;
  pkgs = pins.nixpkgs.import "" { inherit system; };
  fenix = pins.fenix.import "" {
    inherit system;
    inherit (mod) pkgs;
  };
  Toolchain = mod.fenix.fromToolchainFile { file = "${mod}/rust-toolchain.toml"; };
  Protos = mod.pkgs.fetchFromGitHub {
    owner = "nrdxp";
    repo = "snix";
    rev = "protos";
    hash = "sha256-1ZsRIY/n8r9eJvRRO71+cENsdwTolVH7ACWUr0cfncI=";
  };
}
