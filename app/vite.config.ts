import { defineConfig, type Plugin } from "vite";
import { rm } from "node:fs/promises";
import { resolve } from "node:path";

/**
 * GitHub Pages project sites serve from a subpath
 * (https://schrab.github.io/misima-hybrid-player/), not the domain root, so
 * every asset URL has to be prefixed. The desktop Tauri build must NOT be: it
 * serves `dist/` from the root of its own custom protocol, and a subpath base
 * would break every asset reference in the shipped desktop app.
 *
 * `GH_PAGES=1` opts a build into the subpath. The Pages workflow sets it;
 * `npm run build` and `npm run tauri build` do not, so the desktop default is
 * unchanged.
 */
const isPages = process.env.GH_PAGES === "1";
const base = isPages ? "/misima-hybrid-player/" : "/";

/**
 * `public/` is copied verbatim into `dist/`, so the WASM build output rides
 * along on desktop builds too — about 370 KB the desktop app never loads, plus
 * the `.d.ts` files wasm-pack emits. Drop the whole directory unless this is
 * the Pages build, which is the only target that has a worklet.
 */
function stripWasmForDesktop(): Plugin {
  return {
    name: "misima-strip-wasm-for-desktop",
    apply: "build",
    async closeBundle() {
      if (isPages) return;
      await rm(resolve(__dirname, "dist/wasm"), { recursive: true, force: true });
    },
  };
}

export default defineConfig({
  base,
  plugins: [stripWasmForDesktop()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    watch: {
      // Cargo writes lock the target dir on Windows; don't let Vite watch it.
      ignored: ["**/src-tauri/target/**", "**/wasm-dsp/target/**", "**/node_modules/**", "**/.git/**"],
    },
  },
  envPrefix: ["VITE_", "TAURI_"],
  build: {
    target: "es2022",
    minify: "esbuild",
    sourcemap: false,
  },
});

