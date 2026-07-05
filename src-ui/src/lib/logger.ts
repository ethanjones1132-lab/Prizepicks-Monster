// Lightweight structured logger for the React side.
//
// The Rust backend uses `tracing` and the entire ~50 callsite inventory
// flows through one subscriber (see `src-tauri/src/logging.rs`). The
// frontend doesn't have an equivalent — every component just calls
// `console.log("something happened")` and the dev gets an unscoped,
// unprefixed stream that's hard to grep when triaging a UI bug.
//
// This module provides a tiny façade that:
//   1. Tags every line with `[PrizePicks]` so devtools console output is
//      greppable the same way Rust tracing is (the backend already uses
//      `[PrizePicks]` in many of its own log messages — see
//      `predictions/clv.rs::spawn_clv_capture_task`).
//   2. Routes to the right `console.*` level (`.error` for error, `.warn`
//      for warn, etc.) so the devtools filter works.
//   3. Coerces structured key/value pairs onto the same line as a
//      JSON-serialized suffix — readable in devtools, parseable with `jq`
//      if the dev redirects `console` output to a file via a devtools
//      extension. Full OpenTelemetry export is out of scope for this pass;
//      the foundation (Rust tracing subscriber + a parallel TS logger
//      that emits the same shape) is what lands here.
//
// If the project later adopts `tauri-plugin-log` for full Rust ↔ JS log
// bridging, this module is the natural frontend side of that wire — the
// shape is already there.

const PREFIX = '[PrizePicks]';

type LogLevel = 'debug' | 'info' | 'warn' | 'error';

function formatExtras(extras?: Record<string, unknown> | unknown[]): string {
  if (!extras) return '';
  if (Array.isArray(extras)) {
    if (extras.length === 0) return '';
    // Simple positional args: serialize compactly so the line stays one
    // line in devtools. Objects get JSON-stringified; primitives print
    // via String() (consistent with how console.log treats them).
    return ' ' + extras.map((a) => (typeof a === 'object' && a !== null ? JSON.stringify(a) : String(a))).join(' ');
  }
  if (typeof extras === 'object' && extras !== null) {
    const entries = Object.entries(extras as Record<string, unknown>);
    if (entries.length === 0) return '';
    return ' ' + entries.map(([k, v]) => `${k}=${typeof v === 'object' && v !== null ? JSON.stringify(v) : String(v)}`).join(' ');
  }
  return ' ' + String(extras);
}

function emit(level: LogLevel, message: string, extras?: Record<string, unknown> | unknown[]): void {
  const line = `${PREFIX} ${message}${formatExtras(extras)}`;
  switch (level) {
    case 'debug':
      // `console.debug` is hidden by default in many browser devtools
      // configs; fall back to `console.log` if the level is unavailable
      // so a developer with default filters still sees the message.
      if (typeof console.debug === 'function') {
        console.debug(line);
      } else {
        console.log(line);
      }
      return;
    case 'info':
      console.log(line);
      return;
    case 'warn':
      console.warn(line);
      return;
    case 'error':
      console.error(line);
      return;
  }
}

/** Debug-level message; hidden by default in browser devtools. */
export function debug(message: string, extras?: Record<string, unknown> | unknown[]): void {
  emit('debug', message, extras);
}

/** Informational message; visible in devtools by default. */
export function info(message: string, extras?: Record<string, unknown> | unknown[]): void {
  emit('info', message, extras);
}

/** Warning message; surfaced by devtools as yellow. */
export function warn(message: string, extras?: Record<string, unknown> | unknown[]): void {
  emit('warn', message, extras);
}

/** Error message; surfaced by devtools as red. */
export function error(message: string, extras?: Record<string, unknown> | unknown[]): void {
  emit('error', message, extras);
}

/** Default export bundles all four levels for callers that prefer
 *  `import logger from '...'` over four named imports. */
export default { debug, info, warn, error };
