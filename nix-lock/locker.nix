root: rec {
  lockstr =
    let
      path = root + "/atom.lock";
    in
    if builtins.pathExists path then builtins.readFile path else "static = true";

  toml = builtins.fromTOML lockstr;
}
