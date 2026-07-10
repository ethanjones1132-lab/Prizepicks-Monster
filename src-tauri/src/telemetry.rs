//! OpenTelemetry distributed tracing integration for PrizePicks Monster.
//!
//! Wires up [`opentelemetry`] with a [`opentelemetry_stdout`] exporter.
//! Every span produced by `opentelemetry::global::tracer("prizepicks-monster")`
//! is exported to stdout as a one-line summary — ideal for development and
//! debugging.
//!
//! ## Architecture
//!
//! ```text
//! opentelemetry::global::tracer(...)
//!       │
//!       ▼
//!   opentelemetry_sdk::TracerProvider
//!       │
//!       ▼
//!   SimpleSpanProcessor (synchronous, no background task needed)
//!       │
//!       ▼
//!   opentelemetry_stdout::SpanExporter
//!       │
//!       ▼
//!   stdout (one-line span summaries)
//! ```
//!
//! ## Current status
//!
//! ✅ SDK wired — `init_otel()` creates a real `SdkTracerProvider` with a
//!    `SimpleSpanProcessor` + stdout exporter. Spans are printed to stdout.
//!
//! ✅ `tracing-opentelemetry` bridge — wired into both the JSON and Human
//!    subscriber configurations in `logging.rs`. Every `tracing::info_span!(...)`
//!    and `tracing::info!(...)` event flows through the OTel span pipeline
//!    automatically. No trait-bound workaround was needed with the current
//!    dependency versions (`tracing-opentelemetry 0.33` + `opentelemetry 0.32`).
//!
//! ## Environment variables
//!
//! | Variable | Default | Description |
//! |----------|---------|-------------|
//! | `OTEL_SERVICE_NAME` | `prizepicks-monster` | Resource attribute for the service |

use opentelemetry_sdk::trace::{SdkTracerProvider, SimpleSpanProcessor};
use std::sync::OnceLock;

/// Whether OTel has been initialized (prevents double-init).
static OTEL_INITIALIZED: OnceLock<bool> = OnceLock::new();

/// Initialize the OpenTelemetry tracer provider with a stdout exporter.
///
/// Creates a [`SdkTracerProvider`] with a [`SimpleSpanProcessor`] wrapping
/// the stdout exporter. Sets the global tracer provider so any code using
/// `opentelemetry::global::tracer()` gets a real tracer.
///
/// Uses `SimpleSpanProcessor` (synchronous) because `init_otel` runs before
/// the tokio runtime is started (see `lib.rs::run`). A `BatchSpanProcessor`
/// would require a background async task.
///
/// Idempotent — safe to call multiple times. Subsequent calls are no-ops.
///
/// # Returns
///
/// An [`OtelGuard`] whose `Drop` impl shuts down the tracer provider on
/// process exit.
pub fn init_otel() -> OtelGuard {
    if OTEL_INITIALIZED.set(true).is_err() {
        return OtelGuard { inner: None };
    }

    let service_name =
        std::env::var("OTEL_SERVICE_NAME").unwrap_or_else(|_| "prizepicks-monster".to_string());

    // Build the stdout exporter — writes span summaries to stdout.
    // `opentelemetry_stdout::SpanExporter::default()` creates a writer that
    // prints to standard output.
    let exporter = opentelemetry_stdout::SpanExporter::default();

    // `SimpleSpanProcessor` exports synchronously on each span end.
    // Requires no background task, safe to use before tokio runtime init.
    let processor = SimpleSpanProcessor::new(exporter);

    let provider = SdkTracerProvider::builder()
        .with_resource(
            opentelemetry_sdk::Resource::builder()
                .with_attribute(opentelemetry::KeyValue::new(
                    "service.name",
                    service_name,
                ))
                .build(),
        )
        .with_span_processor(processor)
        .build();

    // Set the global tracer provider. Code from any module can then call
    // `opentelemetry::global::tracer("...")` and get this provider's tracer.
    opentelemetry::global::set_tracer_provider(provider.clone());

    tracing::info!("otel: stdout exporter active; set OTEL_SERVICE_NAME to change the resource name");

    OtelGuard {
        inner: Some(provider),
    }
}

/// Guard that shuts down the OpenTelemetry tracer provider on drop.
///
/// When dropped (process exit), [`SdkTracerProvider::shutdown`] flushes all
/// in-flight spans and stops the export pipeline.
#[derive(Debug)]
pub struct OtelGuard {
    inner: Option<SdkTracerProvider>,
}

impl Drop for OtelGuard {
    fn drop(&mut self) {
        if let Some(provider) = &self.inner {
            if let Err(e) = provider.shutdown() {
                // Logging may already be shut down at this point, so use
                // eprintln as a last-resort diagnostic.
                eprintln!("otel: tracer provider shutdown failed: {e}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_otel_returns_guard() {
        let _guard = init_otel();
    }

    #[test]
    fn init_otel_is_idempotent() {
        let _g1 = init_otel();
        let _g2 = init_otel();
    }

    #[test]
    fn otel_guard_drop_does_not_panic() {
        let guard = init_otel();
        drop(guard);
    }

    #[test]
    fn otel_guard_send_sync() {
        fn assert_send<T: Send>() {}
        fn assert_sync<T: Sync>() {}
        assert_send::<OtelGuard>();
        assert_sync::<OtelGuard>();
    }

    #[test]
    fn otel_guard_noop_early_return_is_send_sync() {
        // Early-return (second-call) guards are also Send + Sync.
        let guard = OtelGuard { inner: None };
        fn assert_send<T: Send>() {}
        fn assert_sync<T: Sync>() {}
        assert_send::<OtelGuard>();
        assert_sync::<OtelGuard>();
        drop(guard);
    }
}
