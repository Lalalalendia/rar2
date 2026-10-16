# Chaptera Rescue Desktop Batch V1 (local CLI engine)

This is a **local, offline batch orchestration layer** over the existing
`tools.rescue_salvage_bundle_v1`. It is not yet the packaged Windows GUI and does
not perform semantic text recovery, malformed-CFB tolerant salvage, or native
PUB repair. It requires Python 3.12+ and a checkout of `Lalalalendia/rar2`.

Run from the repository root, with source folders **outside** the output tree:

```powershell
python -m tools.rescue_desktop_batch_v1 scan --input 'D:\PUBCorpus' --output 'D:\RescueOutput'
python -m tools.rescue_desktop_batch_v1 run --output 'D:\RescueOutput' --workers 2
python -m tools.rescue_desktop_batch_v1 status --output 'D:\RescueOutput'
```

`scan` recursively indexes `.pub` files, verifies SHA-256 and identifies duplicate
contents regardless of name. It does not extract. Use `scan --append` to add
another folder to the existing queue without resetting completed jobs. Default
concurrency is two; `--workers 1..4` bounds concurrent file jobs. Each file is
handled in a separate subprocess with a wall-time limit (`--file-timeout`, seconds);
the child runs strict physical CFB extraction and independent validation, using
the already-landed Salvage Bundle V1. Optional `--pub-cli PATH` enables the
existing Chaptera exact-image exporter when a suitable `pub` binary is installed.
`--retain-original` retains exact opaque input bytes inside the **private** bundle.

To suspend a running batch after current jobs finish (no newly scheduled jobs):

```powershell
python -m tools.rescue_desktop_batch_v1 pause --output 'D:\RescueOutput'
```

To terminate active file processes and stop scheduling:

```powershell
python -m tools.rescue_desktop_batch_v1 cancel --output 'D:\RescueOutput'
```

A later `run` continues pending work. Explicitly reset failures and cancellations
before retrying them:

```powershell
python -m tools.rescue_desktop_batch_v1 retry --output 'D:\RescueOutput'
python -m tools.rescue_desktop_batch_v1 run --output 'D:\RescueOutput' --workers 2
```

Files under `output/bundles/<sha256>/` are private and content-bearing.
`queue-private.json` stores **absolute source paths** and is private. Do not
publish or upload either of these to public CI, Notion or a bug report.
`summary.json` and `entries.csv` contain only source SHA, byte lengths, typed
statuses, counts and reasons; these are suitable as source-free evidence.

All completed bundles are independently revalidated when `run` is invoked again;
a corrupted output or changed source is marked `failed` and cannot silently pass.
After a process crash, `running` jobs revert to `pending`, and exact existing
SHA-addressed bundles are re-verified before reuse. The kernel lock file may
remain on disk; **do not delete it**: the lock itself is automatically released
by the OS when a process exits. Only one active runner may write an output root.

Exit code `0` from `run` means every selected source has a **terminal checked
outcome**, including `done_unsupported`, not that full content was recovered;
`1` means unfinished or failed jobs; `2` means invalid/unsafe input. The result
never claims visual equivalence or native Publisher repair. This V1 runner still
needs hardened Windows Job Object memory limits and a packaged, in-app executor
before exposure to arbitrary untrusted PUB files.

Acceptance for the finished desktop product additionally requires a real Windows
GUI batch workflow, independent packing of Python/Rust dependencies into an
offline portable distribution, and a physically mounted, SHA-checked 1,556 PUB
corpus benchmark (a hash-list alone is insufficient).
