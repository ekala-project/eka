{
  root ? ./., # assume we are called in the atom directory by default
  config ? { },
  # FIXME: strictly for compatibility until eka has a calling interface
  extraExtern ? { },
}:
let
  unknownErr = "unknown atom type encountered";
  lockstr = builtins.readFile (root + "/atom.lock");
  lock_toml = builtins.fromTOML lockstr;
  pureScope =
    let
      _builtins = builtins;
    in
    rec {
      import = Import;
      scopedImport = Scoped;
      __getEnv = _: "";
      __nixPath = [ ];
      __currentTime = 0;
      __currentSystem =
        config.platforms.build or abort
          "Accessing the current system is impure. Set the platform in the config instead";
      __storePath = abort "Making explicit dependencies on store paths is illegal.";
      builtins = _builtins // {
        inherit import scopedImport;
        getEnv = __getEnv;
        nixPath = __nixPath;
        currentTime = __currentTime;
        currentSystem = __currentSystem;
        builtins = builtins;
      };
    };
  Import = scopedImport pureScope;
  Scoped = args: scopedImport (args // pureScope);

  f =
    root: lock:
    let
      localSet =
        let
          builder =
            path:
            let
              ekala = path + "/ekala.toml";
              hasSet =
                builtins.readFileType path == "directory" && builtins.isPath ekala && builtins.pathExists ekala;
              toml = builtins.fromTOML (builtins.readFile ekala);
              locals = builtins.listToAttrs (
                map (
                  rel:
                  let
                    toml_path = path + "/${rel}/atom.toml";
                    atom = builtins.fromTOML (builtins.readFile toml_path);
                  in
                  {
                    name = atom.package.label;
                    value = dirOf toml_path;
                  }
                ) toml.set.packages
              );
            in
            if path == /. || path == "/" then
              { }
            else if hasSet then
              locals
            else
              builder (dirOf path);
        in
        builder root;
      entrypoint = lock.compose.entry or "";
      isLocalSet = builtins.mapAttrs (_: v: builtins.elem "::" v.mirrors) lock.sets;
      composer =
        let
          staticComposer =
            root: _:
            let
              tomlPath = root + "/atom.toml";
            in
            if builtins.pathExists tomlPath then builtins.fromTOML (builtins.readFile tomlPath) else { };
          trvialComposer =
            root: args:
            Scoped {
              atoms = args.extern or { };
              cfg = args.config or { };
            } root;
        in
        let
          composeKind = lock.compose.use or null;
        in
        if composeKind == "atom" then
          handlers.atom lock.compose
        else if composeKind == "nix" then
          trvialComposer
        else if composeKind == "static" then
          staticComposer
        else
          abort unknownErr;
      handlers = {
        atom =
          dep:
          let
            fetch =
              if isLocalSet."${dep.set}" && localSet ? ${dep.label} then
                localSet.${dep.label}
              else
                builtins.fetchGit {
                  name = dep.label;
                  inherit (dep) rev;
                  ref = "refs/eka/atoms/${dep.label}/${dep.version}";
                  url = dep.mirror;
                };
            lockPath = fetch + "/atom.lock";
            lockstr = if builtins.pathExists lockPath then builtins.readFile lockPath else "";
            thisLock = builtins.fromTOML lockstr;
          in
          f fetch thisLock;
        "nix+git" =
          dep:
          let
            fetch = builtins.fetchGit {
              inherit (dep) rev url;
              shallow = true;
            };
          in
          {
            import = path: Import (fetch + "/${path}");
            src = fetch;
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
            import = path: Import (fetch + "/${path}");
            src = fetch;
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
            import = path: Import (fetch + "/${path}");
            src = fetch;
          };
        "nix+src" =
          dep:
          let
            fetch = import <nix/fetchurl.nix> {
              inherit (dep) url hash;
            };
          in
          {
            src = fetch;
          };
      };
      keys = {
        atom = "from";
        "nix+src" = "get";
      };
    in
    if lock.version == 1 then
      let
        deps = builtins.foldl' (
          acc: dep:
          let
            handled = handlers."${dep.type or (abort unknownErr)}" dep;
            key = keys.${dep.type} or "pins";
            set = lock.sets."${dep.set or "nil"}".tag or null;
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
      in
      composer (root + "/${entrypoint}") {
        extern = extraExtern // {
          inherit deps;
        };
        config =
          let
            err = "passed configuration must be a json object";
            manifest_str = builtins.readFile (root + "/atom.toml");
            set =
              if builtins.isAttrs config then
                config
              else if builtins.isString config then
                let
                  json = builtins.fromJSON config;
                in
                if builtins.isAttrs json then json else abort err
              else
                abort err;
          in
          set // { inherit (builtins.fromTOML manifest_str) package; };
      }
    else
      abort "unsupported format version";
in
f root lock_toml
