/**
 * Polyfills for `AudioWorkletGlobalScope`.
 *
 * Imported for its side effects by `dspWorklet.js`. The wasm-bindgen glue
 * constructs a `TextDecoder` at module top level, and the worklet global scope
 * does not provide one — verified absent in Chrome's AudioWorkletGlobalScope.
 * Without this, module evaluation throws and `registerProcessor` never runs.
 *
 * Attach to `globalThis`, **not** `self`: `AudioWorkletGlobalScope` has no
 * `self` binding (measured: `typeof self === "undefined"` there, while
 * `globalThis` is an object). Touching `self` throws a ReferenceError at module
 * scope, which kills the whole worklet and surfaces as the deeply misleading
 * "node name 'dsp-processor' is not defined" from a later `AudioWorkletNode`
 * construction — with `addModule()` having resolved successfully throughout.
 */

/* eslint-disable no-undef */
if (typeof TextDecoder === "undefined") {
  // The DSP glue only ever decodes UTF-8 error strings produced by Rust, so a
  // byte-to-char expansion over the code point range is sufficient. Real
  // multi-byte sequences are not needed here; correctness of audio does not
  // depend on this path.
  globalThis.TextDecoder = class TextDecoder {
    constructor() {}
    decode(input) {
      const bytes = input ? new Uint8Array(input) : new Uint8Array(0);
      let out = "";
      for (let i = 0; i < bytes.length; i++) {
        out += String.fromCharCode(bytes[i]);
      }
      return out;
    }
  };
}

if (typeof TextEncoder === "undefined") {
  globalThis.TextEncoder = class TextEncoder {
    constructor() {}
    encode(input) {
      const str = String(input ?? "");
      const bytes = new Uint8Array(str.length);
      for (let i = 0; i < str.length; i++) bytes[i] = str.charCodeAt(i) & 0xff;
      return bytes;
    }
  };
}

export {};
