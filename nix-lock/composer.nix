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
  kind = lock.compose.use or (if lock.static or false then "static" else "");
  key = builtins.hashString "sha256" (kind + lock.compose.at or "");
in
if kind == "nix" then
  trvialComposer
else if kind == "static" then
  staticComposer
else if kind != "" && lock.compose ? at && deps ? "${key}" then
  deps.${key}
else
  abort errors.unknownErr
