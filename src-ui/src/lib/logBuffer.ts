// Lightweight in-process ring buffer + pub-sub for `[PrizePicks]` log lines.
//
// Why this exists:
//   The Rust backend has a structured `tracing` subscriber and a
//   per-command `correlation_id` (see `src-tauri/src/logging.rs`). The
//   frontend has a parallel `logger.ts` that mirrors the same shape on
//   the JS side. Neither is useful to a user triaging a bug without a
//   way to see the lines together — they have to open devtools, switch
//   to the Console tab, and visually scan for a cid, which is exactly
//   the friction a small in-app log viewer removes.
//
// What this module does:
//   1. Defines a typed `LogEntry` (timestamp / level / message / extras).
//   2. Holds a fixed-size ring buffer (default 500 entries) so memory
//      can't grow unbounded if the viewer is left open for hours.
//   3. Exposes a `subscribe(listener) -> unsubscribe` API so the viewer
//      can re-render on every new line. The subscription set is process-
//      wide; multiple listeners (e.g. a future tray icon + the viewer)
//      can subscribe concurrently.
//   4. Exposes a `push(entry)` API the `logger.ts` adapter calls into.
//
// What this module does NOT do:
//   - Persist logs across reloads (would need a Tauri command + SQLite).
//     Out of scope for the in-app viewer; the backend can be configured
//     with `PRIZEPICKS_LOG_FORMAT=json` for that.
//   - Stream Rust-side `tracing` events to the UI. The Rust subscriber
//     writes to stdout; bridging it to the webview would need a Tauri
//     event channel plus a stdout capture (e.g. a tee that writes to
//     a SQLite ring table the UI polls). That's a future pass — this
//     viewer ships the frontend-side scaffolding first.
//
// The module is intentionally framework-agnostic (no React imports) so
// it can be unit-tested in isolation if/when the project adds Vitest.

export type LogLevel = 'debug' | 'info' | 'warn' | 'error';

export interface LogEntry {
  /** Monotonic millisecond timestamp (Date.now()). */
  ts: number;
  /** Log level — mirrors the `LogLevel` union in `logger.ts`. */
  level: LogLevel;
  /** The `[PrizePicks]`-prefixed message body (prefix stripped). */
  message: string;
  /**
   * Optional structured key/value pairs or positional args, exactly as
   * the caller passed them. Kept as a string for fast ring-buffer reads
   * — the formatted form is already in `message` so consumers can
   * either re-parse or display the raw form verbatim.
   */
  extras?: Record<string, unknown> | unknown[];
}

export type LogListener = (entry: LogEntry) => void;

const DEFAULT_CAPACITY = 500;

class LogBuffer {
  private readonly capacity: number;
  private readonly entries: LogEntry[] = [];
  private readonly listeners: Set<LogListener> = new Set();

  constructor(capacity: number = DEFAULT_CAPACITY) {
    // Defensive floor so a caller passing 0 doesn't end up with a
    // ring that drops every entry.
    this.capacity = Math.max(1, capacity | 0);
  }

  /** Add a new entry to the buffer and notify all listeners. */
  push(entry: LogEntry): void {
    this.entries.push(entry);
    if (this.entries.length > this.capacity) {
      // `shift` is O(n) but n is bounded by `capacity` (500 by default),
      // and this only runs on the trim path — once per push once full.
      // A circular index would be faster but harder to reason about for
      // a 500-entry buffer; the trade is fine.
      this.entries.shift();
    }
    for (const listener of this.listeners) {
      try {
        listener(entry);
      } catch {
        // A misbehaving listener must not poison the others. Swallow
        // the error silently — the next push still flows to the
        // remaining listeners.
      }
    }
  }

  /**
   * Subscribe to every new entry. Returns an unsubscribe function —
   * callers should call it from a React `useEffect` cleanup so the
   * listener is removed when the viewer unmounts.
   */
  subscribe(listener: LogListener): () => void {
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  }

  /** Snapshot the current buffer (in insertion order). Cheap clone. */
  snapshot(): LogEntry[] {
    return this.entries.slice();
  }

  /** Current number of buffered entries. */
  size(): number {
    return this.entries.length;
  }

  /** Clear the buffer (listeners are NOT notified — clearing is silent). */
  clear(): void {
    this.entries.length = 0;
  }
}

/**
 * Process-wide ring buffer. Imported by `logger.ts` (writer side) and
 * by `LogViewer.tsx` (reader side). The singleton is intentional — the
 * logger has no per-component state, and the viewer is the only reader
 * in practice, so a module-level instance keeps the API simple.
 */
export const logBuffer = new LogBuffer();

/**
 * Extract a `correlation_id` value from a log entry's extras, if one
 * is present. Returns `null` for entries without the field, for
 * non-object extras, and for non-string `correlation_id` values
 * (defensive — the field is always a string on the Rust side, but the
 * frontend could call `logger.info('foo', { correlation_id: 42 })`
 * with a number and the viewer shouldn't blow up).
 */
export function extractCorrelationId(entry: LogEntry): string | null {
  const extras = entry.extras;
  if (!extras || Array.isArray(extras) || typeof extras !== 'object') {
    return null;
  }
  const raw = (extras as Record<string, unknown>).correlation_id;
  if (typeof raw === 'string' && raw.length > 0) {
    return raw;
  }
  if (typeof raw === 'number' && Number.isFinite(raw)) {
    return String(raw);
  }
  return null;
}
