# Nightly Compiler

The nightly compiler reads the flat JSON manifests under `registry/` and `crash-signatures/` and compiles them into a signed SQLite database (`registry.db`) plus its Ed25519 signature (`registry.db.sig`).

## Local development

Create a virtual environment and install dependencies:

```bash
cd compiler
python -m venv .venv
source .venv/bin/activate  # .venv\Scripts\activate on Windows
pip install -r requirements.txt
```

Build the database:

```bash
python compile.py --out ../registry.db
```

The compiler also supports signing via the `ED25519_PRIVATE_KEY` environment variable (a 64-byte hex-encoded Ed25519 private key seed). If the key is missing or PyNaCl is not installed, the compiler emits an empty `.sig` placeholder and logs a warning.

## CI

`.github/workflows/compile.yml` runs the compiler every night at 02:00 UTC and on `workflow_dispatch`. It uploads `registry.db` and `registry.db.sig` as workflow artifacts. Release-asset deployment is left as a placeholder step requiring a release token secret.
