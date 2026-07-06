import { useEffect, useMemo, useState } from 'react';
import {
  extractCorrelationId,
  LogEntry,
  LogLevel,
  logBuffer,
} from '../lib/logBuffer';

type LevelFilter = 'all' | LogLevel;

const LEVEL_ORDER: Record<LogLevel, number> = {
  error: 0,
  warn: 1,
  info: 2,
  debug: 3,
};

const LEVEL_LABELS: Record<LogLevel, string> = {
  error: '🔴 Error',
  warn: '🟡 Warn',
  info: '🔵 Info',
  debug: '⚪ Debug',
};

/**
 * In-app log viewer — shows the most recent entries from the
 * `logBuffer` ring buffer (populated by `lib/logger.ts`).
 *
 * Why this exists: the backend has `tracing` with structured
 * `correlation_id` fields; the frontend has a parallel `logger.ts`
 * that mirrors the shape. Neither is useful to a triaging user
 * without a viewing surface. This is the surface.
 *
 * Scope (intentionally minimal):
 *   - Filter by level (all/error/warn/info/debug)
 *   - Free-text search across the message + formatted extras
 *   - Highlight and "copy correlation_id" affordance when present
 *   - Pause/resume live tail (the buffer keeps accumulating; the
 *     viewer just stops re-rendering on new pushes)
 *   - Clear button
 *
 * Out of scope:
 *   - Persisting logs across reloads
 *   - Streaming Rust-side `tracing` events to the viewer (would need
 *     a stdout-capture Tauri command). The viewer ships the
 *     frontend-side scaffolding first; the backend wire is a future
 *     pass.
 */
export function LogViewer() {
  const [entries, setEntries] = useState<LogEntry[]>(() => logBuffer.snapshot());
  const [levelFilter, setLevelFilter] = useState<LevelFilter>('all');
  const [search, setSearch] = useState<string>('');
  const [paused, setPaused] = useState<boolean>(false);
  const [copyHint, setCopyHint] = useState<string | null>(null);

  useEffect(() => {
    // Subscribe to the ring buffer. The unsubscribe is returned by
    // `subscribe()` and called from the effect cleanup so the listener
    // is removed when the component unmounts (e.g. on tab switch).
    const unsubscribe = logBuffer.subscribe((entry) => {
      if (paused) {
        return;
      }
      setEntries((prev) => {
        // Append and cap at the buffer's effective length. We don't
        // re-snapshot on every push because that's O(n) per line and
        // adds up when a refresh emits 50 events in a burst.
        const next = prev.length >= 500 ? prev.slice(prev.length - 499) : prev.slice();
        next.push(entry);
        return next;
      });
    });
    return unsubscribe;
  }, [paused]);

  const filtered = useMemo(() => {
    const needle = search.trim().toLowerCase();
    return entries.filter((e) => {
      if (levelFilter !== 'all' && e.level !== levelFilter) {
        return false;
      }
      if (needle.length > 0) {
        const haystack = `${e.message} ${e.extras ? JSON.stringify(e.extras) : ''}`.toLowerCase();
        if (!haystack.includes(needle)) {
          return false;
        }
      }
      return true;
    });
  }, [entries, levelFilter, search]);

  // Counts per level for the level-filter chips. Computed off the
  // un-filtered `entries` so a search query doesn't hide the level
  // counts.
  const counts = useMemo(() => {
    const c: Record<LogLevel, number> = { error: 0, warn: 0, info: 0, debug: 0 };
    for (const e of entries) {
      c[e.level] += 1;
    }
    return c;
  }, [entries]);

  const handleCopyCid = async (cid: string) => {
    try {
      if (navigator.clipboard && typeof navigator.clipboard.writeText === 'function') {
        await navigator.clipboard.writeText(cid);
      }
      setCopyHint(`copied ${cid}`);
    } catch {
      setCopyHint(`copy failed (${cid})`);
    }
    // Auto-clear the hint after a short delay.
    setTimeout(() => setCopyHint(null), 2000);
  };

  return (
    <section className="page prizepicksPage">
      <header className="prizepicksHeader">
        <div>
          <h2>🪵 Log viewer</h2>
          <p className="muted">
            In-app ring buffer of the last <strong>500</strong> frontend
            <code>[PrizePicks]</code> log lines. Use the level filter and
            search to find a specific event; click a{' '}
            <code>correlation_id</code> chip to copy it for cross-referencing
            with the Rust <code>tracing</code> output.
          </p>
        </div>
      </header>

      <div className="logViewerToolbar">
        <div className="logViewerLevels">
          {(['all', 'error', 'warn', 'info', 'debug'] as LevelFilter[]).map((lvl) => {
            const count =
              lvl === 'all'
                ? entries.length
                : counts[lvl as LogLevel];
            return (
              <button
                key={lvl}
                className={`logViewerLevelChip ${lvl} ${levelFilter === lvl ? 'active' : ''}`}
                onClick={() => setLevelFilter(lvl)}
                aria-pressed={levelFilter === lvl}
              >
                {lvl === 'all' ? 'All' : LEVEL_LABELS[lvl as LogLevel]}
                <span className="logViewerCount">{count}</span>
              </button>
            );
          })}
        </div>

        <div className="logViewerSearchWrap">
          <input
            type="search"
            className="logViewerSearch"
            placeholder="🔍 search message / extras"
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            aria-label="Search log entries"
          />
        </div>

        <button
          className={`logViewerPause ${paused ? 'paused' : ''}`}
          onClick={() => setPaused((p) => !p)}
          aria-pressed={paused}
        >
          {paused ? '▶ resume' : '⏸ pause'}
        </button>
        <button
          className="logViewerClear"
          onClick={() => {
            logBuffer.clear();
            setEntries([]);
          }}
        >
          🗑 clear
        </button>
      </div>

      {copyHint && <div className="logViewerCopyHint">{copyHint}</div>}

      {filtered.length === 0 ? (
        <div className="logViewerEmpty">
          {entries.length === 0
            ? 'No log lines yet. Trigger a dashboard load or click around — frontend `logger.info` / `logger.warn` calls will appear here.'
            : 'No entries match the current filter. Try a lower minimum level or clear the search box.'}
        </div>
      ) : (
        <ol className="logViewerList" data-testid="log-list">
          {filtered
            // Sort DESC by timestamp so the newest is on top — matches
            // devtools console default. The buffer appends in
            // insertion order, so a reverse walk is the cheaper
            // alternative to a per-push sort.
            .slice()
            .sort((a, b) => b.ts - a.ts)
            .map((entry, idx) => {
              const cid = extractCorrelationId(entry);
              const tsLabel = formatTs(entry.ts);
              const levelClass = `logViewerRow ${entry.level}`;
              return (
                <li key={`${entry.ts}-${idx}`} className={levelClass}>
                  <span className="logViewerTs">{tsLabel}</span>
                  <span className={`logViewerLevel ${entry.level}`}>
                    {entry.level.toUpperCase().padEnd(5)}
                  </span>
                  <span className="logViewerMessage">
                    {entry.message}
                    {entry.extras ? (
                      <span className="logViewerExtras">
                        {' '}
                        {formatExtrasDisplay(entry.extras)}
                      </span>
                    ) : null}
                  </span>
                  {cid ? (
                    <button
                      className="logViewerCidChip"
                      onClick={() => handleCopyCid(cid)}
                      title={`Click to copy correlation_id "${cid}"`}
                    >
                      cid: {cid}
                    </button>
                  ) : null}
                </li>
              );
            })}
        </ol>
      )}

      {paused && entries.length > 0 ? (
        <div className="logViewerPausedBanner">
          ⏸ Paused — new log lines are buffered but not displayed. Click
          resume to see them.
        </div>
      ) : null}
    </section>
  );
}

function formatTs(ts: number): string {
  const d = new Date(ts);
  // `HH:MM:SS.mmm` — 12 chars, fixed-width so columns line up.
  const hh = String(d.getHours()).padStart(2, '0');
  const mm = String(d.getMinutes()).padStart(2, '0');
  const ss = String(d.getSeconds()).padStart(2, '0');
  const ms = String(d.getMilliseconds()).padStart(3, '0');
  return `${hh}:${mm}:${ss}.${ms}`;
}

function formatExtrasDisplay(extras: Record<string, unknown> | unknown[]): string {
  // Mirror the logger's `formatExtras` rendering so the viewer's
  // display matches what devtools shows for the same call. Keeps
  // the line compact and one-liner.
  if (Array.isArray(extras)) {
    return extras
      .map((a) =>
        typeof a === 'object' && a !== null ? JSON.stringify(a) : String(a)
      )
      .join(' ');
  }
  return Object.entries(extras)
    .map(
      ([k, v]) =>
        `${k}=${typeof v === 'object' && v !== null ? JSON.stringify(v) : String(v)}`
    )
    .join(' ');
}

// Re-export the `LEVEL_ORDER` so a future test that wants to assert the
// sort order can `import { LEVEL_ORDER } from './LogViewer'`. Not used
// at runtime.
export { LEVEL_ORDER };
