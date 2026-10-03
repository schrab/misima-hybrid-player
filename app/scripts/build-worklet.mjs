/**
 * Bundle the AudioWorklet into one self-contained, import-free file.
 *
 * Why this exists: `AudioWorkletGlobalScope` does not reliably support static
 * `import`. A worklet module that imports anything still resolves
 * `addModule()`, but its top-level statements never run — so `registerProcessor`
 * never fires and `new AudioWorkletNode` throws `InvalidStateError` with a
 * misleading "node is not defined" message. Bundling removes every import and
 * with it the whole failure mode.
 *
 * `dsp-glue` is aliased to the wasm-bindgen glue `wasm-pack` writes beside the
 * `.wasm` binary, so the generated code is inlined rather than fetched at
 * runtime.
 */
import { existsSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import * as esbuild from "esbuild";

const appDir = join(dirname(fileURLToPath(import.meta.url)), "..");
const glue = join(appDir, "public", "wasm", "misima_wasm_dsp.js");
const outFile = join(appDir, "public", "wasm", "dspWorklet.js");

if (!existsSync(glue)) {
  console.error(
    `missing wasm-bindgen glue at ${glue}\nrun "npm run build:wasm" first.`,
  );
  process.exit(1);
}

await esbuild.build({
  entryPoints: [join(appDir, "src", "worklet", "dspWorklet.js")],
  outfile: outFile,
  bundle: true,
  // IIFE, not ESM: an ESM output could still carry an import if a future
  // dependency slipped in, and the whole point is that there are none.
  format: "iife",
  target: "es2022",
  platform: "browser",
  alias: { "dsp-glue": glue },
  // The glue's unused async initializer does `new URL(..., import.meta.url)`.
  // We never call it (we use initSync with a pre-compiled module), but the
  // reference still has to resolve for the IIFE to build.
  define: { "import.meta.url": JSON.stringify(join(appDir, "public", "wasm", "misima_wasm_dsp_bg.wasm")) },
  legalComments: "none",
  logLevel: "info",
});

// Fail the build rather than shipping a worklet that cannot register.
const built = await import("node:fs").then((fs) => fs.readFileSync(outFile, "utf8"));
if (/\bimport\s*[({'"]/.test(built)) {
  console.error("bundled worklet still contains an import statement");
  process.exit(1);
}
if (!built.includes('registerProcessor("dsp-processor"')) {
  console.error("bundled worklet does not register dsp-processor");
  process.exit(1);
}
console.log("worklet bundled ->", outFile);
