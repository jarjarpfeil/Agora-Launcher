---
description: "Compile the registry database and run a sanity check."
---
cd compiler && python compile.py --skip-sign --out ../registry.db && python ../scripts/verify_db.py
