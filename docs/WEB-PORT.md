# Web Port — Implementation Log

Running record of the static-browser port described in
[`compose/spec/web-app.md`](compose/spec/web-app.md). One section per phase,
committed as each phase lands, so the history shows what shipped and what it
was verified against.

The spec is the plan of record. This file is the diary: what actually
happened, what verification really reported, and where the plan turned out to
be wrong or optimistic.

---

## Phase 0 — pre-port refactor (desktop only) — **complete**

Extract the two platform-free types out of `player.rs` so both crates can
`#[path]`-include them. No behavioural change intended.

**Done:**

- New `src/audio/reverb_mix.rs`: `Reverb`, `follow()`, `mix_reverb_frame()`.
- New `src/audio/stretcher.rs`: `Stretcher` (engine selector).
- `player.rs` now imports both instead of defining them; `mod.rs` registers
  the new modules.
- Tests moved with their code: `reverb_dry_ish_stability` and
  `reverb_mix_loudness_constant` → `reverb_mix.rs`; `wsola_pipeline_integration`
  → `stretcher.rs`. Added `select_swaps_engine_on_pitch_up` to pin the
  vocoder/WSOLA crossover, which had no direct test before.
- `docs/DSP.md`: module map updated, plus a new *Shared vs desktop-only*
  section naming the eight shared modules and the two that stay desktop-side.

**Why these two, specifically:** `Reverb` and `Stretcher` are the only types in
`player.rs` that touch no cpal, tauri, or parking_lot API. `player.rs` also
holds `SharedPlay`, the stream-owner thread, and `resample_interleaved`, which
all do, and which the worklet reimplements rather than shares.

**Verified:** `cargo test` 43 passed / 1 ignored / 0 failed (was 42 — the new
crossover test is the delta). `cargo check` zero warnings. `npm test` green,
`npm run build` green. Desktop behaviour unchanged: the extracted code is
byte-identical, only relocated.

**Note for whoever runs this next:** a stale `target/` cache in this checkout
made `cargo check` fail with `failed to read plugin permissions ... misima-hybrid-winamp\...`
— a path from a previous, renamed checkout. `cargo clean -p tauri -p tauri-build
-p tauri-plugin-dialog -p tauri-plugin-fs` clears it. Unrelated to any source
change.

---

## Phase 1 — WASM crate + worklet plumbing + EQ + reverb — **pending**
