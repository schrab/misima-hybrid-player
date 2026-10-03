/**
 * Build the static site for GitHub Pages.
 *
 * Runs the same gate as `npm run build` — typecheck, tests, then `vite build` —
 * with `GH_PAGES=1` set so `vite.config.ts` switches the asset base to the
 * repo subpath. A failing test fails the deploy.
 *
 * Two portability notes, both learned the hard way:
 *
 * - `GH_PAGES=1 vite build` is a POSIX shell prefix and fails under cmd.exe,
 *   which is what npm uses on Windows. The env var is set in-process instead.
 * - `npm` / `npx` are `.cmd` shims on Windows, and recent Node refuses to spawn
 *   those without a shell (EINVAL). The tools are therefore invoked as
 *   `node <their JS entry point>`, which needs no shell on any platform.
 */
import { spawnSync } from "node:child_process";
import { existsSync, statSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const appDir = join(dirname(fileURLToPath(import.meta.url)), "..");
const mod = (p) => join(appDir, "node_modules", p);

/** The test files `npm test` runs, kept in step with package.json's script. */
const TEST_FILES = ["src/sprite/spectrumLayout.test.ts", "src/transport.test.ts"];

function step(label, args) {
  console.log(`\n--- ${label}`);
  const result = spawnSync(process.execPath, args, {
    cwd: appDir,
    stdio: "inherit",
    env: { ...process.env, GH_PAGES: "1" },
  });
  if (result.error) {
    console.error(`${label} failed to start:`, result.error.message);
    process.exit(1);
  }
  if (result.status !== 0) {
    console.error(`${label} exited with ${result.status}`);
    process.exit(result.status ?? 1);
  }
}

step("typecheck", [mod("typescript/bin/tsc")]);
for (const file of TEST_FILES) {
  step(`test ${file}`, [mod("tsx/dist/cli.mjs"), file]);
}
step("vite build", [mod("vite/bin/vite.js"), "build"]);

// Fail loudly rather than deploying a site whose worklet cannot load.
for (const f of ["dist/index.html", "dist/wasm/dspWorklet.js", "dist/wasm/misima_wasm_dsp_bg.wasm"]) {
  const p = join(appDir, f);
  if (!existsSync(p)) {
    console.error(`expected build output missing: ${f}`);
    process.exit(1);
  }
  if (statSync(p).size === 0) {
    console.error(`build output is empty: ${f}`);
    process.exit(1);
  }
}
console.log("\npages build complete");
