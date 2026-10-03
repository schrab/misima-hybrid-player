/**
 * File acquisition for the web build: a hidden `<input type=file>` plus
 * whole-window drag-and-drop, both yielding `File[]`.
 *
 * The desktop build uses Tauri's native dialog and gets filesystem paths. Here
 * the `File` objects are the only handle we ever have on the audio, which is
 * why paths are not a thing on the web side of `Transport`.
 */

const EXTENSIONS = ["mp3", "flac", "wav", "ogg", "m4a", "aac", "opus"];

let input: HTMLInputElement | null = null;

/** Lazily created, reused across calls — one hidden input per page. */
function fileInput(): HTMLInputElement {
  if (input) return input;
  input = document.createElement("input");
  input.type = "file";
  input.multiple = true;
  input.accept = EXTENSIONS.map((e) => `.${e}`).join(",");
  input.style.display = "none";
  document.body.appendChild(input);
  return input;
}

/**
 * Show the file picker. Resolves with the chosen files, or an empty array if
 * the user cancelled.
 */
export function pickFiles(): Promise<File[]> {
  const el = fileInput();
  return new Promise((resolve) => {
    const onChange = () => {
      el.removeEventListener("change", onChange);
      resolve(Array.from(el.files ?? []));
      // Reset so choosing the same file twice in a row still fires `change`.
      el.value = "";
    };
    // `cancel` is not universal; without it a dismissed dialog simply never
    // resolves, which is preferable to resolving with a phantom empty pick on
    // browsers that fire neither event.
    el.addEventListener("change", onChange);
    el.click();
  });
}

/** True when a drag carries files, as opposed to text or a page drag. */
function hasFiles(ev: DragEvent): boolean {
  const types = ev.dataTransfer?.types;
  if (!types) return false;
  return Array.from(types).includes("Files");
}

/**
 * Accept dropped files anywhere on the page. Calls `onFiles` with the audio
 * files the drop carried.
 */
export function installDropTarget(onFiles: (files: File[]) => void): () => void {
  const over = (ev: DragEvent) => {
    if (hasFiles(ev)) ev.preventDefault();
  };
  const drop = (ev: DragEvent) => {
    if (!hasFiles(ev)) return;
    // Required, or the browser navigates to the dropped file.
    ev.preventDefault();
    const files = Array.from(ev.dataTransfer?.files ?? []).filter(isAudio);
    if (files.length > 0) onFiles(files);
  };

  window.addEventListener("dragover", over);
  window.addEventListener("drop", drop);
  return () => {
    window.removeEventListener("dragover", over);
    window.removeEventListener("drop", drop);
  };
}

function isAudio(file: File): boolean {
  const ext = file.name.split(".").pop()?.toLowerCase() ?? "";
  return EXTENSIONS.includes(ext) || file.type.startsWith("audio/");
}
