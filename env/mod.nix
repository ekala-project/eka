let
  inherit (deps) pins;
  system = cfg.platform;
in
{
  Shell = mod.shell;
  Pkgs = mod.pkgs;
  pkgs = pins.nixpkgs.import "" { inherit system; };
  fenix = pins.fenix.import "" {
    inherit system;
    inherit (mod) pkgs;
  };
}
