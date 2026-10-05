/**
 * Unit tests for the transport abstraction and the web player's pure logic.
 *
 * Plain `.ts` run by `tsx` (see the `test` script in package.json) — no test
 * framework. Anything needing a real `AudioContext`, `fetch`, or DOM is out of
 * scope here and is exercised in the browser instead; these cover the parts
 * where a silent regression would be invisible until someone opened the page.
 */

import { WebTransport } from "./transportWeb";
import { assetUrl, baseUrl } from "./web/base";

let passed = 0;
let failed = 0;
function check(label: string, cond: boolean) {
  if (!cond) {
    console.error(`FAIL: ${label}`);
    failed++;
    return;
  }
  console.log(`ok: ${label}`);
  passed++;
}

async function main() {
  const t = new WebTransport();

  check("web transport reports isWeb", t.isWeb === true);

  // Window control must be inert on the web, not throw — main.ts calls these
  // unconditionally from the same handlers on both platforms.
  await t.startDragging();
  await t.minimize();
  await t.close();
  check("window controls are no-ops on web", true);

  // Zoom commands are inert; they must still resolve to a usable shape.
  const scale = await t.getUiScale();
  check(
    "getUiScale resolves with a UiScaleInfo shape",
    typeof scale.scale === "number" && typeof scale.w === "number",
  );
  await t.setUiScale(0.75);
  await t.cycleUiScale(1);

  // Playlist is empty before anything is loaded.
  const rows = await t.getPlaylist();
  check("empty playlist on a fresh web transport", rows.length === 0);

  check("position is 0 with nothing loaded", (await t.getPosition()) === 0);

  // Cue with no track loaded reports -1, matching the desktop contract that
  // main.ts turns into the "No track" status line.
  check("cuePercent with no track returns -1", (await t.cuePercent(0.5)) === -1);

  // Seeking with no track must clamp to 0 and not throw.
  await t.seek(10);
  check("seek with no track stays at 0", (await t.getPosition()) === 0);
  await t.seek(-5);
  check("negative seek clamps to 0", (await t.getPosition()) === 0);

  // Pause/stop/clear are all safe with no engine started.
  await t.pause();
  await t.stop();
  await t.clearPlaylist();
  check("transport ops are safe before the engine starts", true);

  // setParams before the worklet exists must not throw — main.ts pushes
  // initial fader values during init, long before any user gesture.
  await t.setParams({
    cutoff: 20000,
    speed: 1,
    pitch: 0,
    reverb: 0.2,
    // Deliberately over-length: exercises the binding's pad/truncate — the
    // wasm `set_params` binding must tolerate payloads that predate the
    // 8-band contract instead of panicking on the audio thread.
    eq: [0, 0, 0, 0, 6, 0, 0, 0, 0, 0],
    shift: 12,
    tone: 0.65,
  });
  check("setParams before engine start is safe", true);

  // Events: registering a handler before the worklet exists must not throw.
  let got: Float32Array | null = null;
  await t.on("spectrum", (payload) => {
    got = payload as Float32Array;
  });
  check("on() registers without an engine", got === null);

  // baseUrl falls back to "/" outside Vite so tests can import these modules.
  check("baseUrl falls back outside Vite", baseUrl() === "/");
  check("assetUrl joins without doubling slashes", assetUrl("wasm/x.wasm") === "/wasm/x.wasm");
  check(
    "assetUrl tolerates a leading slash",
    assetUrl("/sprite/skin.json") === "/sprite/skin.json",
  );

  console.log(`\n${passed} checks passed`);
  if (failed > 0) {
    console.log(`${failed} checks FAILED`);
    throw new Error("transport smoke failed");
  }
}

void main();
