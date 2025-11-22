{
  platforms ? { },
  ...
}:
let
  Import = scopedImport pureScope;
  Scoped = args: scopedImport (args // pureScope);
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
        platforms.build
          or (abort "Accessing the current system is impure. Set the platform in the config instead");
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
in
{
  inherit Scoped Import;
}
