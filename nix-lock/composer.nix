{
  lock,
  errors,
  dep-key,
}:
Scoped: deps:
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
  composeKind = lock.compose.use or (if lock.static then "static" else null);
in
if composeKind == "atom" then
  deps.${dep-key (lock.compose // { type = "atom"; })}
else if composeKind == "nix" then
  trvialComposer
else if composeKind == "static" then
  staticComposer
else
  abort errors.unknownErr
