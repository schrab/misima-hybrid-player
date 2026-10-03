/**
 * Build the DSP crate to WASM with SIMD and drop it next to the worklet.
 *
 * `simd128` is not optional: without it rustfft falls back to scalar kernels
 * that are 3-4x slower, which is the difference between fitting the 128-sample
 * render quantum comfortably and dropping buffers. See spec §4.9.
 *
 * Output lands in `public/wasm/`, which Vite copies verbatim into `dist/`, so
 * the worklet can `import` the wasm-bindgen glue at runtime with no bundler
 * involvement.
 */
import { spawnSync } from "node:child_process";
import { existsSync } from "node:fs";
import { homedir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const crate = join(here, "..", "wasm-dsp");
const outDir = join(here, "..", "public", "wasm");

// Resolve wasm-pack from PATH first, then from the cargo bin directory. A
// fresh `cargo install wasm-pack` lands in ~/.cargo/bin, which is not on PATH
// for GUI-launched processes on Windows (it is for shells that source the
// profile). Without this the build dies with a bare "not recognized" that
// looks like a missing dependency rather than a PATH problem.
const cargoBin = join(homedir(), ".cargo", "bin");
const isWin = process.platform === "win32";
const exe = (name) => (isWin ? `${name}.exe` : name);
const candidates = ["wasm-pack", join(cargoBin, exe("wasm-pack"))];

let wasmPack = null;
for (const candidate of candidates) {
  const probe = spawnSync(candidate, ["--version"], { stdio: "ignore", shell: isWin });
  if (!probe.error && probe.status === 0) {
    wasmPack = candidate;
    break;
  }
}
if (!wasmPack) {
  console.error(
    "wasm-pack not found.\n" +
      `  looked in PATH and in ${cargoBin}\n` +
      "  install it with: cargo install wasm-pack --locked",
  );
  process.exit(1);
}

// The worklet imports this by relative path; a build that produced no wasm
// would fail at runtime with a confusing import error instead.
const wasmFile = join(outDir, "misima_wasm_dsp_bg.wasm");
if (existsSync(wasmFile)) {
  console.log("wasm already built — delete public/wasm/misima_wasm_dsp_bg.wasm to rebuild");
  process.exit(0);
}

const result = spawnSync(wasmPack, ["build", "--target", "web", "--release", "--out-dir", outDir], {
  cwd: crate,
  stdio: "inherit",
  env: {
    ...process.env,
    RUSTFLAGS: `${process.env.RUSTFLAGS ?? ""} -C target-feature=+simd128`.trim(),
  },
});

if (result.error) {
  console.error("failed to run wasm-pack:", result.error.message);
  process.exit(1);
}
if (result.status !== 0) {
  console.error(`wasm-pack exited with ${result.status}`);
  process.exit(result.status ?? 1);
}
