/**
 * Local-only static server for the `music/` library, with CORS enabled.
 *
 * The browser refuses to `fetch` audio from another origin without an
 * `Access-Control-Allow-Origin` header, so a plain `python -m http.server`
 * cannot be used to test the hosted-MP3 path against the app served by Vite.
 * This server exists purely so that path can be exercised during development;
 * it is not part of the build or the deploy.
 *
 *   node scripts/serve-music.mjs [--port 4174]
 */
import { createReadStream, existsSync, statSync } from "node:fs";
import { createServer } from "node:http";
import { dirname, extname, join, normalize } from "node:path";
import { fileURLToPath } from "node:url";

const appDir = join(dirname(fileURLToPath(import.meta.url)), "..");
const musicDir = join(appDir, "..", "music");

const portArg = process.argv.indexOf("--port");
const port = portArg > -1 ? Number(process.argv[portArg + 1]) : 4174;

if (!existsSync(musicDir)) {
  console.error(`no music library at ${musicDir}`);
  process.exit(1);
}

const TYPES = {
  ".mp3": "audio/mpeg",
  ".flac": "audio/flac",
  ".wav": "audio/wav",
  ".ogg": "audio/ogg",
  ".m4a": "audio/mp4",
};

createServer((req, res) => {
  const name = decodeURIComponent((req.url ?? "/").split("?")[0]).replace(/^\/+/, "");
  // normalize() collapses any ../ segments before the join, so a crafted
  // path cannot escape the music directory.
  const file = join(musicDir, normalize(name));
  if (!file.startsWith(musicDir) || !existsSync(file) || !statSync(file).isFile()) {
    res.writeHead(404, { "Access-Control-Allow-Origin": "*" });
    res.end("not found");
    return;
  }
  const type = TYPES[extname(file).toLowerCase()] ?? "application/octet-stream";
  res.writeHead(200, {
    "Content-Type": type,
    "Content-Length": statSync(file).size,
    "Access-Control-Allow-Origin": "*",
    "Accept-Ranges": "bytes",
  });
  createReadStream(file).pipe(res);
}).listen(port, () => {
  console.log(`music library on http://localhost:${port}/ (CORS enabled)`);
});
