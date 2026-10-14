# JPEG decoder 0.3.2: Chaptera single-threaded security profile

Vendored from https://github.com/image-rs/jpeg-decoder at pinned upstream commit `eb2d7c0f6a2d0298aba7a7f8b9ca1440353e8f8c`.
Original package `jpeg-decoder` version `0.3.2`, dual-licensed MIT / Apache-2.0; original license texts copied unchanged.

Only Chaptera modifications to the upstream sources:
- Remove obsolete upstream `target_arch = "asmjs"` cfg probes (8 references in worker/mod.rs); preserve `wasm32` exclusion while allowing strict Rust 1.94 `-D unexpected-cfgs`.
- A named cargo feature `single_thread_only`, kept outside the outer workspace.
- `Decoder::select_worker` selects `PreferWorkerKind::Immediate` unconditionally when that feature is enabled.
- MPSC worker and Rayon module/dispatch are cfg-excluded under `single_thread_only`.
- Export `SINGLE_THREAD_ONLY` so consuming crate CI can assert its selected runtime profile.

Why: real PUB022 JPEG (3508x2480) was accepted as source input but its decoder emitted `jpeg_pixels_io_permission` after Chaptera's post-read seccomp BPF blocked `SYS_clone` and `SYS_clone3`; raising RLIMIT_AS 768/1536/3072MiB did not help. The upstream JPEG parser tries `std::thread::Builder::spawn` for sufficiently large frames, which is forbidden. Security fix must not loosen seccomp.

All parsing and bounding code is otherwise copied from pinned source; this change only selects the decoder's existing serial execution path. That is a candidate fix, not yet evidence of successful hosted Chromium or Publisher A/B: exact-head worker, hostile-image negative controls and same-baseline browser tests remain required. Raw original JPEG/PNG must never become browser-delivery data.
