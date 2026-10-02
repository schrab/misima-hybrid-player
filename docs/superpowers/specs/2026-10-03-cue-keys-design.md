# Number-key cue points and transport hotkeys

Date: 2026-10-03
Status: approved design, not yet implemented

## Problem

The player has no way to jump to a point inside the current track. Scrubbing
means dragging the position fader by hand, which is imprecise and awkward
during live listening. There is also no keyboard transport at all — play,
pause and skip are pointer-only.

## Goal

Bare number keys `1`–`9` cue playback to 10 %…90 % of the loaded track, so a
single keypress moves the playhead to that fraction and starts playing from
there. `0` returns to the start. A small set of classic Winamp transport keys
(`Space`, `←`, `→`, `z`, `x`) makes the keyboard usable for the rest of
playback.

## Scope

In scope:

- `1`…`9` → 10 %…90 % of the loaded track; `0` → 0 %.
- Cueing starts playback if the player is paused or stopped.
- `Space` play/pause, `←`/`→` seek ∓10 s, `z`/`x` prev/next track.
- Status-line feedback for every action.

Out of scope:

- User-configurable cue percentages. The 10 % step is fixed.
- System-wide (OS-level) hotkeys. The keys work only while the player window
  has focus, so they never hijack typing in another application.
- Any change to the DSP chain. Cueing reuses the existing `seek_secs` path,
  which already bumps `seek_gen` to flush the stretcher tail.
- Track-relative chording (for example "next cue point after 7"). Keys are
  absolute percentages of the track.

## Behaviour decisions

- **Paused or stopped player.** A cue key always ends up playing from the cue
  point. It moves the playhead via the normal seek path and then starts the
  stream if it was not already running.
- **Key scope.** Window focus only, via a DOM `keydown` listener. Only bare
  keys are handled; any Ctrl/Cmd/Alt-modified press is ignored, so the
  existing `Ctrl+0` zoom reset and `Ctrl+D` zoom toggle keep working and
  modified presses are never swallowed.
- **Both number row and numpad.** `ev.key` alone identifies the digit, so the
  numpad works without extra code.

## Key table

| Key | Action | Status line |
|---|---|---|
| `1`…`9` | cue to 10 %…90 % | `CUE 40% 1:23` |
| `0` | cue to 0 % | `CUE 0% 0:00` |
| `Space` | toggle play/pause | `PLAYING` / `PAUSED` |
| `←` | seek −10 s (clamped at 0) | `SEEK -10 0:45` |
| `→` | seek +10 s | `SEEK +10 2:03` |
| `z` | previous track | `PREV <title>` |
| `x` | next track | `NEXT <title>` |

## Backend

### `app/src-tauri/src/audio/player.rs`

Two functions next to the existing `position_secs()`:

- `duration_secs() -> f64` — length of the loaded track in seconds, derived
  from the decoded buffer: `samples.len() / channels / sample_rate`. Returns
  `0.0` when nothing is loaded or the sample rate is unset. Reading the real
  decoded buffer avoids depending on the playlist's `mm:ss` display string,
  which is minutes-only.
- `cue_fraction(f: f64) -> f64` — clamps `f` to `0.0..=0.9`, multiplies by
  `duration_secs()`, backs the target off 50 ms from the end of the track so
  it always lands inside the buffer, calls the existing `seek_secs(target)`,
  then calls `play()` when the player was not already running. Returns the
  target position in seconds, or `-1.0` when no track is loaded.

The 50 ms back-off matters: seeking to exactly the last sample leaves nothing
for the decoder to deliver and the player can appear stuck at the end.

`seek_secs` also gains an upper clamp against the loaded buffer (last valid
frame, minus one frame of margin). It currently clamps only the lower bound,
so a seek past the end would park the cursor beyond the samples; the clamp
covers keyboard cues, fader drags that overshoot, and any future caller.

### `app/src-tauri/src/commands.rs`

- `cue_percent(fraction: f64) -> f64` — wrapper over `cue_fraction`, returns
  the achieved target in seconds.
- `toggle_play() -> bool` — flips `shared.playing` and returns the new state.
  The frontend asks the backend rather than tracking play state itself, so
  the two can never disagree.

Both are registered in `lib.rs` alongside the existing command list.

## Frontend

### `app/src/main.ts`

A second `window.addEventListener("keydown", …)` beside the zoom listener at
line 106. The zoom listener is left untouched — it is capture-phase and
modifier-gated, so there is no overlap.

The new listener:

1. Returns immediately when `ctrlKey`, `metaKey` or `altKey` is set.
2. Dispatches on `ev.key` per the key table above.
3. Calls `ev.preventDefault()` for every key it handles, so `Space` and the
   arrows do not scroll the page.
4. Updates the `status` string after the backend round-trip resolves, using a
   new `fmtTime(secs)` helper for `m:ss` formatting, and writes `NO TRACK` when
   `cue_percent` returns `-1.0`.

`←`/`→` read the current position with the existing `get_position` command
before seeking, so the nudge is relative to where the playhead actually is.

## Error handling

- Cue with nothing loaded: `cue_fraction` returns `-1.0`, the status line reads
  `NO TRACK`, and no state is touched.
- `←` past the start of the track: the target is clamped at zero, so the
  playhead lands at 0:00 rather than going negative.
- `→` past the end: `seek_secs` gains an upper clamp against the loaded buffer
  (last valid frame, minus one frame of margin). Today it clamps only the
  lower bound, so a seek past the end would park the cursor beyond the
  samples. The clamp applies to every caller, including fader drags that
  overshoot.
- A cue keypress during a track transition is safe: `seek_secs` operates on
  whichever track is loaded at that moment, and `load_gen`/`seek_gen` keep the
  background decoder race-free.

## Verification

Automated, per `AGENTS.md` §5:

```bash
cd app/src-tauri && cargo test -- --nocapture   # 39 DSP tests must stay green
cd app/src-tauri && cargo check                 # zero warnings
cd app && npm run build                         # TypeScript typecheck + build
```

No DSP code changes, so the existing test suite is a regression gate rather
than a target.

Manual, in `tauri dev`:

- Load a track of known length, press `4`, and confirm playback resumes at 40 %
  of that length with the status line reading `CUE 40% m:ss`.
- Press `4` while paused: it should start playing from the cue point.
- Press `1`, `9`, `0` and check the percentages land correctly.
- Press `Space` twice, `←`/`→`, and `z`/`x`.
- With no track loaded, press `4`: status reads `NO TRACK`, nothing crashes.
- Press `Ctrl+0` and `Ctrl+D`: zoom reset and zoom toggle still work.
- Press digits while another application has focus: that application receives
  them unchanged.
