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
 * `public/` is copied verbatim into `dist/`, so the WASM build output, the
 * bundled demo track and the page backdrop all ride along on desktop builds
 * too — weight the desktop app never loads, plus the `.d.ts` files wasm-pack
 * emits. Drop all three unless this is the Pages build, the only target with a
 * worklet, a startup track and a wallpaper behind the player.
 *
 * The backdrop is referenced from CSS, but only under `html.web`, which the
 * desktop never sets — so the missing file is never requested there.
 */
function stripWebOnlyAssets(): Plugin {
  return {
    name: "misima-strip-web-only-assets",
    apply: "build",
    async closeBundle() {
      if (isPages) return;
      for (const path of [
        "dist/wasm",
        "dist/music",
        "dist/misima-background.png",
      ]) {
        await rm(resolve(__dirname, path), { recursive: true, force: true });
      }
    },
  };
}

export default defineConfig({
  base,
  plugins: [stripWebOnlyAssets()],
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

