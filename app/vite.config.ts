import { defineConfig } from "vite";

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
const base = process.env.GH_PAGES === "1" ? "/misima-hybrid-player/" : "/";

export default defineConfig({
  base,
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

