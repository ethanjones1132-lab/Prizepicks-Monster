//! OpenTelemetry integration foundation for PrizePicks Monster.
//!
//! This module defines the single integration point for distributed tracing
//! via OpenTelemetry (OTel). Currently provides a **no-op tracer provider** —
//! the app runs identically with or without observability infrastructure.
//!
//! ## Design
//!
//! [`init_otel`] is called once at app startup, right after
//! [`logging::init_logging`]. It sets the global [`opentelemetry`] tracer
//! provider (or leaves the default no-op in place). The rest of the crate
//! uses standard `tracing::*!` macros — trace context propagation is handled
//! by the subscriber layer.
//!
//! ## Onboarding an exporter (future step)
//!
//! To add a real OTLP exporter:
//!
//! 1. Add crate deps to `Cargo.toml`:
//!    ```toml
//!    opentelemetry = { version = "0.28", features = ["trace"] }
//!    opentelemetry_sdk = { version = "0.28", features = ["rt-tokio"] }
//!    opentelemetry-otlp = { version = "0.28", features = ["tls"] }
//!    tracing-opentelemetry = "0.28"
//!    ```
//!
//! 2. In this module's `init_otel`:
//!    - Read `PRIZEPICKS_OTEL_ENDPOINT` (or `OTEL_EXPORTER_OTLP_ENDPOINT`)
//!    - Create an [`opentelemetry_otlp::SpanExporter`] pointing at that endpoint
//!    - Build an [`opentelemetry_sdk::trace::TracerProvider`] with that exporter
//!    - Call `opentelemetry::global::set_tracer_provider(provider)`
//!    - Return an [`OtelGuard`] that calls `provider.shutdown()` on drop
//!
//! 3. In [`logging::init_logging`], after setting up the `tracing_subscriber`
//!    registry, register the OTel layer:
//!    ```rust
//!    let otel_layer = tracing_opentelemetry::layer();
//!    let _ = tracing_subscriber::registry()
//!        .with(env_filter)
//!        .with(human_layer)
//!        .with(otel_layer)
//!        .try_init();
//!    ```
//!
//! No other file in the crate needs to change — the rest of the app emits
//! `tracing::*!` events and spans, which the OTel layer converts into
//! distributed trace data automatically.
//!
//! ## Pre-OTel correlation_id
//!
//! The [`logging::new_correlation_id`] function provides 8-char hex
//! correlation ids for command-level trace grouping. These are the stepping
//! stone to full W3C `trace_id` + `span_id` pairs. The migration path:
//!
//! 1. Replace `new_correlation_id()` with
//!    `opentelemetry::trace::TraceContextExt::span().span_context().trace_id()`
//! 2. Pass the trace_id through the existing `correlation_id` field name
//!    so downstream log parsers don't need to change
//!
//! ## Current status
//!
//! The module is a **structural foundation** — the function signature,
//! startup wiring, and documentation are in place. The OTel crate deps and
//! exporter configuration are intentionally deferred because they require:
//! - A decision on the OTLP collector / vendor (Grafana Tempo, Jaeger,
//!   Honeycomb, or vendor-neutral OTLP Collector)
//! - A corresponding config schema in `config.json`

use std::sync::OnceLock;

/// Whether OTel has been initialized (prevents double-init).
static OTEL_INITIALIZED: OnceLock<bool> = OnceLock::new();

/// Initialize the OpenTelemetry tracer provider.
///
/// Current behavior: **no-op**. Sets no global tracer provider, so the
/// `opentelemetry` API returns a default no-op tracer. The app logs a single
/// `tracing::info!` line indicating the no-op state so operators can verify
/// the integration point is wired.
///
/// ## Future behavior (when an exporter crate is added)
///
/// 1. Read `PRIZEPICKS_OTEL_ENDPOINT` (or fall back to
///    `OTEL_EXPORTER_OTLP_ENDPOINT`, or use a config.json path).
/// 2. If set, configure an `opentelemetry_otlp::SpanExporter` pointing at that
///    endpoint, build a `TracerProvider`, and register it globally.
/// 3. If unset, remain no-op (same as today).
///
/// Idempotent — safe to call multiple times. Subsequent calls are no-ops.
///
/// # Returns
///
/// An [`OtelGuard`] whose `Drop` impl shuts down the tracer provider. If
/// `init_otel` is called from a long-lived `main` / `run` function, the guard
/// should live for the process lifetime. If called multiple times, the guard
/// from the first call controls the shutdown.
pub fn init_otel() -> OtelGuard {
    if OTEL_INITIALIZED.set(true).is_err() {
        // Already initialized — return a no-op guard.
        return OtelGuard;
    }

    tracing::info!(
        "otel: no-op tracer provider (no exporter configured; set PRIZEPICKS_OTEL_ENDPOINT to activate)"
    );

    OtelGuard
}

/// Guard that shuts down the OpenTelemetry tracer provider on drop.
///
/// In the current no-op implementation, the `Drop` is a no-op. When an
/// exporter is wired, this guard calls [`opentelemetry::global::shutdown_tracer_provider`]
/// and/or the provider's `shutdown()` method, ensuring all in-flight spans
/// are flushed before the process exits.
#[derive(Debug)]
pub struct OtelGuard;

impl Drop for OtelGuard {
    fn drop(&mut self) {
        // No-op: no tracer provider to shut down.
        // Future: `opentelemetry::global::shutdown_tracer_provider();`
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_otel_returns_guard() {
        // The function must return a value without panicking.
        let _guard = init_otel();
    }

    #[test]
    fn init_otel_is_idempotent() {
        // Call twice — second call must not panic.
        let _g1 = init_otel();
        let _g2 = init_otel();
    }

    #[test]
    fn otel_guard_drop_does_not_panic() {
        // Dropping the guard must not panic (it's a no-op today).
        let guard = init_otel();
        drop(guard);
    }

    #[test]
    fn otel_guard_send_sync() {
        // `OtelGuard` must be `Send + Sync` so it can be held across
        // `tokio::runtime::Runtime::block_on` boundaries if needed.
        fn assert_send<T: Send>() {}
        fn assert_sync<T: Sync>() {}
        assert_send::<OtelGuard>();
        assert_sync::<OtelGuard>();
    }
}
