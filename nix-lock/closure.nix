root:
let
  rootLock = locker root;

  LocalSet = import ./locals.nix root;
  isLocalSet = builtins.mapAttrs (_: v: builtins.elem "::" v.mirrors) rootLock.toml.sets or { };

  fetchers = import ./fetch.nix { inherit LocalSet isLocalSet; };

  dep-key = import ./dep-key.nix;
  locker = import ./locker.nix;

  top-level =
    let
      lock = rootLock.toml;
    in
    {
      inherit (lock) deps;
      node = { inherit root; };
      key = "root";
      type = "atom";
    };

in
builtins.genericClosure {
  startSet = [ top-level ];
  operator =
    item:
    map (
      dep:
      let
        node = fetchers.${dep.type} dep;
      in
      {
        inherit node;
        inherit (dep) type;
        key = dep-key dep;
      }
      // (
        let
          lock = locker node.root;
        in
        if dep.type == "atom" && lock.toml ? deps then { inherit (lock.toml) deps; } else { }
      )
    ) item.deps or [ ];
}
