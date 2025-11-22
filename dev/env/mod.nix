let
  inherit (deps) pins;
in
rec {
  Main = Shell;
  Shell = mod.shell mod.pkgs;
  Static = mod.shell mod.pkgs.pkgsStatic;
  Pkgs = mod.pkgs;
  pkgs = pins.nixpkgs.import "" {
    system = cfg.platforms.build;
    crossSystem = cfg.platforms.target or cfg.platforms.build;
  };
  fenix = pins.fenix.import "" {
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
