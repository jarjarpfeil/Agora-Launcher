# Pack Override Zips

Override zips are **configuration and asset overrides only**. They are applied on top of a freshly installed instance after all mods from the manifest have been resolved and downloaded.

## Allowed contents

Only files under the following directories may be included:

- `config/` — mod configuration files
- `defaultconfigs/` — default server-side configs
- `resourcepacks/` — embedded resource packs
- `kubejs/` — KubeJS scripts

## Forbidden contents

Override zips must **never** contain:

- A `mods/` directory
- Any executable file (`.jar`, `.class`, `.exe`, `.bat`, `.cmd`, `.sh`, `.ps1`, `.dll`, `.so`, `.dylib`, `.msi`, `.dmg`, etc.)
- Any installer, launcher, or self-extracting archive

All `.jar` files must enter the instance through the platform manifest and verified download pipeline, never through overrides. This is a structural security requirement, not a suggestion.
