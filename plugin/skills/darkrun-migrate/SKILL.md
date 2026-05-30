---
name: darkrun-migrate
description: Migrate legacy lifecycle state into darkrun's .darkrun/ StateStore — always dry-run first, get explicit approval, then apply one Run at a time
---

Convert legacy state with `darkrun migrate <slug>` — dry-run is the default. Show the dry-run output and get the user's explicit OK before `darkrun migrate <slug> --apply`. One slug at a time unless the user approves `--all`; never pass `--allow-dirty` without approval. Then suggest `/darkrun:darkrun-pickup <slug>` to resume. See `darkrun migrate --help` for flags.
