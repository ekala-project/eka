{ LocalSet, isLocalSet }:
{
  atom =
    dep:
    let
      root =
        if isLocalSet."${dep.set}" or false && LocalSet ? ${dep.label} then
          LocalSet.${dep.label}
        else
          builtins.fetchGit {
            name = dep.label;
            inherit (dep) rev;
            ref = "refs/eka/atoms/${dep.label}/${dep.version}";
            url = dep.mirror;
          };
    in
    {
      inherit root;
    };
  "nix+git" =
    dep:
    let
      fetch = builtins.fetchGit {
        inherit (dep) rev url;
        shallow = true;
      };
    in
    {
      root = fetch;
    };
  "nix+tar" =
    dep:
    let
      fetch = builtins.fetchTarball {
        inherit (dep) url;
        sha256 = dep.hash;
      };
    in
    {
      root = fetch;
    };
  "nix" =
    dep:
    let
      fetch = builtins.fetchurl {
        inherit (dep) url;
        sha256 = dep.hash;
      };
    in
    {
      root = fetch;
    };
  "nix+src" =
    dep:
    let
      fetch = import <nix/fetchurl.nix> {
        inherit (dep) url hash;
      };
    in
    {
      root = fetch;
    };
}
