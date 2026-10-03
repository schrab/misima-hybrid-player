/**
 * `import.meta.env.BASE_URL` with a fallback.
 *
 * Vite injects `import.meta.env` at build time, but the same modules are
 * imported by `tsx` when running the unit tests, where `import.meta.env` is
 * `undefined` — and reading a property off `undefined` throws at module load.
 * Guarding here keeps `player.ts` importable from plain Node.
 */
export function baseUrl(): string {
  const env = (import.meta as unknown as { env?: { BASE_URL?: string } }).env;
  return env?.BASE_URL ?? "/";
}

/** Join `BASE_URL` with an app-relative path. */
export function assetUrl(path: string): string {
  return `${baseUrl()}${path.replace(/^\/?/, "")}`;
}
