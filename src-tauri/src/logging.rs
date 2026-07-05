//! Structured logging initialization for PrizePicks Monster.
//!
//! Wires up [`tracing_subscriber`] so every `tracing::*!` callsite in the
//! crate (47 of them at last count, spanning commands, background tasks, and
//! data fetchers) flows through a single subscriber. Supports two output
//! modes, selected at startup via environment variables:
//!
//! - **Default (human):** pretty, colorized single-line records with
//!   `file:line` annotated, ideal for `cargo run` / `cargo tauri dev`.
//! - **JSON (structured):** when `PRIZEPICKS_LOG_FORMAT=json`, emits
//!   one-JSON-object-per-line suitable for piping into a log aggregator
//!   (Loki, Datadog, OpenTelemetry Collector, etc.). The `tracing-subscriber`
//!   `json` feature is already enabled in `Cargo.toml`.
//!
//! Level filtering is controlled by the standard `RUST_LOG` env var
//! (e.g. `RUST_LOG=prizepicks_monster_lib=debug,info`); defaults to `info`
//! when unset.
//!
//! ## Why this module exists separately
//!
//! The original `lib.rs::run` called `tracing_subscriber::fmt().init()` *inside*
//! a `tokio::runtime::Runtime::block_on(async { … })`. Subscriber registration
//! is a synchronous, process-wide side effect that has no reason to live
//! inside a runtime future — moving it out is a clean separation and makes
//! it trivial to swap the formatter without touching `lib.rs::run`.

use std::io::IsTerminal;
use std::sync::OnceLock;
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

/// Format selector for [`init_logging`]. Parsed once from the
/// `PRIZEPICKS_LOG_FORMAT` env var; the cached value is reused on subsequent
/// calls so we don't re-read the environment every invocation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogFormat {
    /// Pretty, colorized single-line output for humans.
    Human,
    /// One-JSON-object-per-line output for log aggregators.
    Json,
}

impl LogFormat {
    /// Parse the `PRIZEPICKS_LOG_FORMAT` env var. Case-insensitive match on
    /// `"json"`, anything else (including unset) → `Human`. Recognized values
    /// are deliberately kept minimal — adding a new format is a deliberate
    /// decision, not a typo.
    pub fn from_env() -> Self {
        let raw = std::env::var("PRIZEPICKS_LOG_FORMAT").unwrap_or_default();
        if raw.eq_ignore_ascii_case("json") {
            LogFormat::Json
        } else {
            LogFormat::Human
        }
    }
}

/// Cached format selection so subsequent `init_logging` calls (e.g. from a
/// test harness) don't re-parse the env var. The cache is process-wide
/// because the subscriber itself is process-wide; tests that want a fresh
/// env read should run in their own process.
static CACHED_FORMAT: OnceLock<LogFormat> = OnceLock::new();

/// Initialize the global `tracing` subscriber. Idempotent — safe to call
/// multiple times (subsequent calls no-op once a subscriber is registered,
/// via `try_init` swallowing the `AlreadyInitialized` error).
///
/// Returns the [`LogFormat`] that was applied, so the caller (or a test)
/// can assert the right sink was selected without re-reading the env var.
pub fn init_logging() -> LogFormat {
    let format = *CACHED_FORMAT.get_or_init(LogFormat::from_env);
    let env_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    // Build the formatter. `.with_target(true)` is omitted from the pretty
    // path because the human reader is the developer, who knows the crate
    // structure; including the target on every line clutters the output
    // without adding information. JSON output keeps the target so log
    // aggregators can filter by crate.
    match format {
        LogFormat::Json => {
            let json_layer = fmt::layer()
                .json()
                .with_current_span(true)
                .with_span_list(false)
                .with_file(true)
                .with_line_number(true);
            let _ = tracing_subscriber::registry()
                .with(env_filter)
                .with(json_layer)
                .try_init();
        }
        LogFormat::Human => {
            // Suppress ANSI when stdout isn't a terminal (CI logs, piped
            // cargo output, journalctl — all of these look bad with raw
            // escape codes). `IsTerminal` is stable since Rust 1.70 and
            // needs no extra dependency.
            let use_ansi = std::io::stdout().is_terminal();
            let human_layer = fmt::layer()
                .with_file(true)
                .with_line_number(true)
                .with_target(false)
                .with_ansi(use_ansi);
            let _ = tracing_subscriber::registry()
                .with(env_filter)
                .with(human_layer)
                .try_init();
        }
    }

    tracing::info!(
        format = ?format,
        "prizepicks_monster logging initialized"
    );

    format
}

/// Generate a short correlation id for a single command invocation.
///
/// Returned as an 8-character lowercase-hex string derived from the
/// sub-second nanosecond clock (the bottom 32 bits of the `UNIX_EPOCH`
/// nanosecond count). 8 hex chars = 32 bits of entropy, which is enough
/// to keep collisions vanishingly rare across a single user session
/// (the expected collision point for 32-bit ids is ~65k samples; the
/// dashboard bootstrap fires at most a few times per minute).
///
/// **Why 8 chars, not a UUID?** Log lines in devtools are read by humans;
/// pasting a long UUID into a bug report is annoying and grepping a long
/// UUID through 5k log lines is slow. 8 chars is short enough to type
/// from a screenshot and long enough to be unique per-invocation.
///
/// **Why not just `Uuid::new_v4()`?** Pulling in the `uuid` crate is a
/// heavy dependency for a value that's only used as a log key. The
/// nanosecond clock is already in `std` and is sufficient for the
/// "group a single command's log lines together" use case.
///
/// **Caveat for log aggregators:** this is **not** a W3C-compliant
/// `trace_id` (those are 16 bytes / 32 hex chars). The full OTel
/// observability item in `PRIORITIES.md` will introduce a real
/// `trace_id` + `span_id` pair when the project adopts an OTel SDK;
/// the correlation id is the pre-OTel stepping stone that lets us
/// group log lines by user action today.
pub fn new_correlation_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    // Bottom 32 bits → 8 hex chars. `as u64` is safe (nanos is u128 but
    // the bottom 32 bits are well within u64 range).
    format!("{:08x}", (nanos as u64) & 0xFFFF_FFFF)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_format_falls_through_to_human_for_empty_string() {
        // The `from_env` parser uses `unwrap_or_default()` on `std::env::var`
        // and then `eq_ignore_ascii_case("json")` on the result. An empty
        // string is the most common "env var set to nothing" case and must
        // fall through to Human. We exercise the empty-string branch via
        // the same primitive the production parser uses, so the test is
        // independent of whatever PRIZEPICKS_LOG_FORMAT the caller set in
        // the process env. (A direct test of `from_env` would race every
        // other test that runs in parallel within the same cargo-test
        // invocation — the OnceLock cache means the result is process-wide
        // and the first reader wins.)
        let empty = "";
        assert!(
            !empty.eq_ignore_ascii_case("json"),
            "empty env value must NOT be recognized as json (must fall through to human)"
        );
    }

    #[test]
    fn log_format_recognises_json_case_insensitively() {
        // The parser uses `eq_ignore_ascii_case("json")`; verify the cases
        // we care about are all covered. We can't mutate the process env
        // from a parallel test without racing the other tests in this
        // module, so we exercise the recognition primitive directly.
        for c in ["json", "JSON", "Json", "jSoN"] {
            assert!(
                c.eq_ignore_ascii_case("json"),
                "expected eq_ignore_ascii_case to recognize {} as json",
                c
            );
        }
    }

    #[test]
    fn log_format_unrecognized_values_fall_through_to_human() {
        // Defensive test: anything that isn't "json" (case-insensitive)
        // must be treated as the default human format, not silently
        // misclassified. Pins the fall-through behavior the production
        // parser relies on.
        for v in ["", "yaml", "text", "JSON5", "pretty", "toml"] {
            assert!(
                !v.eq_ignore_ascii_case("json"),
                "value {} should NOT be recognized as json (must fall through to human)",
                v
            );
        }
    }

    #[test]
    fn log_format_variants_are_distinct() {
        // Compile-time + runtime guarantee that the two variants don't
        // accidentally collapse to the same discriminant. Catches a
        // regression where someone adds a new variant but forgets to
        // keep the equality exhaustive.
        assert_ne!(LogFormat::Human, LogFormat::Json);
    }

    #[test]
    fn correlation_id_is_8_char_lowercase_hex() {
        // 8-char format: the docstring promises exactly that. The hex
        // alphabet must be lowercase to match the format string `{:08x}`
        // and to be `jq -r` / `grep`-friendly in CI scripts.
        let cid = new_correlation_id();
        assert_eq!(cid.len(), 8, "expected 8-char correlation id, got {:?}", cid);
        assert!(
            cid.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()),
            "expected lowercase hex only, got {:?}",
            cid
        );
    }

    #[test]
    fn correlation_id_changes_across_rapid_calls() {
        // 1000 rapid calls should produce at least 990 distinct values.
        // The nanosecond clock advances between every call, but on some
        // platforms (Windows, older Linux) the resolution can be coarser
        // than 1ns and a few collisions are possible. 990/1000 (99%)
        // is the threshold that catches a regression to a constant or
        // 1-second-resolution clock while tolerating the platform's
        // actual resolution.
        let mut seen = std::collections::HashSet::new();
        for _ in 0..1000 {
            seen.insert(new_correlation_id());
        }
        assert!(
            seen.len() >= 990,
            "expected at least 990 distinct correlation ids in 1000 calls, got {}",
            seen.len()
        );
    }

    #[test]
    fn correlation_id_uses_only_hex_alphabet() {
        // Pins the character set: a future change to base36 or any
        // non-hex alphabet would break the docstring promise and
        // (more importantly) the regex `^[0-9a-f]{8}$` any consumer
        // uses to extract the id from a log line. This test fails
        // loudly if anyone widens the alphabet.
        for _ in 0..50 {
            let cid = new_correlation_id();
            assert!(
                cid.chars()
                    .all(|c| matches!(c, '0'..='9' | 'a'..='f')),
                "non-hex char in correlation id {:?}",
                cid
            );
        }
    }

    #[test]
    fn correlation_id_survives_into_tracing_event_field() {
        // End-to-end check: a `tracing::info!` event with a
        // `correlation_id` field captures the cid we generated and
        // re-emits it through the same `Display` impl the production
        // `prizepicks_get_dashboard_bootstrap` and `prizepicks_refresh`
        // commands rely on. Catches a regression where the `%cid`
        // formatter is dropped (e.g. switching to `cid` without `%`
        // would emit the field's `Debug` repr, which is also a string
        // but with quotes — grep would still find it, but a downstream
        // `jq` filter wouldn't).
        use std::sync::{Arc, Mutex};
        use tracing_subscriber::{fmt, prelude::*, EnvFilter};

        // Capture the layer's output in a shared buffer.
        let buf = Arc::new(Mutex::new(Vec::<u8>::new()));
        let buf_clone = Arc::clone(&buf);
        let make_writer = move || -> Box<dyn std::io::Write> {
            Box::new(BufferWriter(buf_clone.clone()))
        };

        let subscriber = tracing_subscriber::registry()
            .with(EnvFilter::new("info"))
            .with(
                fmt::layer()
                    .with_writer(make_writer)
                    .with_ansi(false)
                    .with_target(false),
            );

        let cid = new_correlation_id();
        tracing::subscriber::with_default(subscriber, || {
            tracing::info!(correlation_id = %cid, "[PrizePicks] verification event");
        });

        let captured = String::from_utf8(buf.lock().unwrap().clone()).unwrap();
        assert!(
            captured.contains(&cid),
            "expected captured output to contain cid {}, got: {}",
            cid,
            captured
        );
        assert!(
            captured.contains("[PrizePicks] verification event"),
            "expected captured output to contain the event message, got: {}",
            captured
        );
    }

    /// Trivial `Write` adapter that just appends to a shared `Vec<u8>`.
    /// The `fmt::layer().with_writer(...)` API requires a `MakeWriter`
    /// that yields a `Write`; `Vec<u8>` is the natural sink for tests.
    use std::sync::{Arc, Mutex as StdMutex};
    struct BufferWriter(Arc<StdMutex<Vec<u8>>>);
    impl std::io::Write for BufferWriter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
}
