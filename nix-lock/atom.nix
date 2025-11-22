{
  root ? ./.,
  config ? { },
  # FIXME: strictly for compatibility until eka has a calling interface
  extraExtern ? { },
}:
let
  inherit (import ./scope.nix config) Scoped Import;

  fix =
    f:
    let
      x = f x;
    in
    x;

  locker = import ./locker.nix;
  dep-key = import ./dep-key.nix;
  errors = import ./errors.nix;
  closure = import ./closure.nix root;

  keys = {
    atom = "from";
    "nix+src" = "get";
  };

  f =
    root': all-deps:
    let
      lock = (locker root').toml;
      entrypoint = lock.compose.entry or "";
      composer = import ./composer.nix { inherit lock errors dep-key; } Scoped all-deps;
    in
    if lock.version or { } == 1 || lock.static or false then
      let
        deps = builtins.foldl' (
          acc: dep:
          let
            handled = all-deps.${dep-key dep};
            key = keys.${dep.type} or "pins";
            set = lock.sets."${dep.set or ""}".tag or null;
          in
          acc
          // {
            ${key} =
              if set != null && dep ? label then
                acc.${key} or { }
                // {
                  ${set} = acc.${key}.${set} or { } // {
                    ${dep.label} = handled;
                  };
                }
              else
                acc.${key} or { } // { ${dep.name} = handled; };
          }
        ) { } lock.deps;
        atom = composer (root' + "/${entrypoint}") {
          extern = extraExtern // {
            inherit deps;
          };
          config =
            let
              manifest_str = builtins.readFile (root' + "/atom.toml");
              set =
                if builtins.isAttrs config then
                  config
                else if builtins.isString config then
                  let
                    json = builtins.fromJSON config;
                  in
                  if builtins.isAttrs json then json else abort errors.configError
                else
                  abort errors.configErr;
            in
            set // { inherit (builtins.fromTOML manifest_str) package; };
        };
      in
      atom
    else
      abort "unsupported format version";

in
(fix (
  deps:
  builtins.listToAttrs (
    map (dep: {
      name = dep.key;
      value =
        if dep.type == "atom" then
          f dep.node.root deps
        else if dep.type == "nix+src" then
          { src = dep.node.root; }
        else
          {
            import = path: Import (dep.node.root + "/${path}");
            src = dep.node.root;
          };

    }) closure
  )
)).root
