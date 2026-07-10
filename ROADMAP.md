# PrizePicks Monster — Phased Roadmap

Last updated: 2026-07-09 (overnight maintenance pass — Profit Factor in paper summary card) — OTel structural foundation)

This roadmap derives from `PRIORITIES.md` (ranked backlog), `AGENTS.md` (working rules), and commit history. Milestones are checkable items with explicit status.

---

## Phase 0 — Foundation (COMPLETE ✅)

- [x] PrizePicks grading (contract-side, Over/Under) — `prizepicks/grading.rs`
- [x] Background auto-grade (poll resolved markets) — `spawn_auto_grade_task`
- [x] Paper trading equity curve + snapshots — `paper/mod.rs`, `EquityCurve` UI
- [x] Kelly stake engine with correlation scaling — `portfolio_risk.rs`
- [x] Isotonic calibration wired into decision path — `analysis/calibration.rs`

---

## Phase 1 — Analytics Deepening (COMPLETE ✅)

All per-axis paper performance breakdowns shipped:

- [x] Per-category (Points, Rebounds, Assists, etc.)
- [x] Per-side (Over / Under)
- [x] Per-hold-time (Intraday → Long)
- [x] Per-player (extracted from lot title)
- [x] Per-entry-price (5× 20¢ buckets)
- [x] Per-disagreement-bucket (Disagreement >12pp / Consensus / Unknown)
- [x] Per-confidence-tier (High / Medium / Low / None)
- [x] Per-tag (user-journal tags, multi-tag lots count toward each)

Visualization shipped:

- [x] Equity curve chart (7d/30d/90d/All range toggle)
- [x] Session PnL chips (Today / 7d) with baseline tooltips
- [x] Streak chip (W3 / L2 / —)
- [x] Calibration scatter (fair % vs realized PnL, stake-proportional bubbles)
- [x] Paper journal UI (inline notes + tags editor with save)

---

## Phase 2 — ML / Prediction Layer (COMPLETE ✅)

- [x] ML predictor Python script (GradientBoostingClassifier, one-hot stat_category features)
- [x] Per-category training pipeline (separate model per stat category)
- [x] ML predictor UI tab (train, score pending, export features, per-category table)
- [x] Volatility-adjusted Kelly (Brier-driven shrinkage multiplier, cold/warm logic)
- [x] Shrinkage wired into live decision path (MarketDetailPanel surfaces warnings)
- [x] CLV capture (entry vs closing price, idempotent background task)

---

## Phase 3 — Dashboard Performance (MOSTLY COMPLETE)

| Item | Status | Notes |
|------|--------|-------|
| Quick cache (flat `/markets`) | ✅ Done 2026-06-17 | `fetch_markets_flat_pages`, `ensure_quick_cache` |
| Partial-cache indicator badge | ✅ Done 2026-06-27 | 📦 badge in header |
| Combined IPC bootstrap | ✅ Done 2026-06-27 | `prizepicks_get_dashboard_bootstrap` single round-trip |
| Startup prefetch (instant quick cache) | ✅ Done 2026-06-28 | Spawned at app startup, no 8s delay |
| **Decouple cache reads from long fetches** | ✅ Done 2026-07-03 | `Arc<RwLock<Option<PrizePicksCache>>>` + `AtomicBool` fetch guard — UI reads clone the cache under a read-lock, full warm (10s+ of 20 pages) runs without holding the write-lock, fetch dedup prevents concurrent 20-page sweeps. New `try_begin_fetch` / `end_fetch` / `wait_for_in_flight_fetch` helpers. 15 new unit tests. |
| Slim cache to `PrizePicksMarketSummary` | ⬜ Deferred | Optional optimization |
| Persist summary cache to SQLite | ⬜ Deferred | Instant next-launch paint |

---

## Phase 4 — Correlation Engine (P1 PARTIAL — NO CONCRETE PLAN)

- [x] Ticker-prefix heuristics (event/series/category/same-ticker scaling factors)
- [ ] **Event/series/macro graph** — player-level, team-level, same-game parlay correlations
- [ ] Data source for correlation graph (no API identified yet)
- [ ] Integration into `portfolio_risk.rs` scaling logic

**Status:** Accepted limitation. Heuristic sufficient for ≤3-leg paper positions. No active implementation planned.

---

## Phase 5 — Polish & Hardening (ONGOING)

- [x] README comprehensive rewrite (2026-07-02)
- [x] LICENSE file added (MIT, 2026-07-02)
- [x] TypeScript strict mode full coverage (src-ui tsconfig.json)
- [x] E2E tests for critical user flows (Playwright)
- [x] Benchmarks for hot paths (grading, portfolio_risk, calibration) — 2026-07-05
- [x] Structured logging foundation (tracing-subscriber human + JSON modes, frontend logger.ts) — 2026-07-05
|- [x] Per-command `correlation_id` for trace grouping (pre-OTel stepping stone) — 2026-07-05
|- [x] OpenTelemetry structural foundation (`telemetry.rs` module, no-op guard, onboarding docs) — 2026-07-08
|- [x] **OTel SDK adoption** (`opentelemetry` + `opentelemetry_sdk` + `opentelemetry-stdout` crates; `SdkTracerProvider` with `SimpleSpanProcessor` + stdout exporter; global tracer provider set; 6 unit tests) — **2026-07-09** (evening)
|- [x] **`tracing-opentelemetry` bridge** — wired into both JSON and Human subscriber configurations in `logging.rs`. Every `tracing::info_span!(...)` and `tracing::info!(...)` event flows through the OTel span pipeline automatically. No trait-bound workaround needed with `tracing-opentelemetry 0.33` + `opentelemetry 0.32`. `tracing-opentelemetry = "0.33"` added, `OtelLayer` plumbed into both subscriber arms. — **2026-07-09** (evening)

---

## Success Metrics (per phase)

| Phase | Metric | Target |
|-------|--------|--------|
| 0 | All lib tests pass | 275+ ✅ |
| 1 | 10 performance views render | 10/10 ✅ |
| 2 | ML predictor produces usable predictions | CV accuracy > 55% on holdout |
| 3 | Dashboard warm revisit < 300ms | Partial cache indicator shipped |
| 4 | Correlation reduces Kelly on truly correlated legs | Not yet measurable |
| 5 | Zero regressions on release | CI gate |

---

## Next Actionable Items (Priority Order)

|Last updated: 2026-07-09 (evening maintenance pass — `tracing-opentelemetry` bridge landed + OTel Layer plumbed into both subscriber arms)

| # | Item | Status | Notes |
|---|------|--------|-------|
| 1 | ~~Complete Phase 3 decoupling~~ | ✅ Done 2026-07-03 | `Arc<RwLock<Option<PrizePicksCache>>>` + `AtomicBool` fetch guard. 15 new unit tests. |
| 2 | ~~Add E2E test scaffolding — Playwright config + 2-3 critical flows~~ | ✅ Done 2026-07-04 | Playwright config + tests covering app load, paper trading, ML predictor, settings, analytics breakdowns. |
| 3 | ~~TypeScript strict mode~~ | ✅ Already enabled | `strict: true` is in `src-ui/tsconfig.json`. |
| 4 | ~~Benchmark harness~~ | ✅ Done 2026-07-05 | Criterion benches for `grading.rs`, `portfolio_risk.rs`, `calibration.rs`. 14 bench functions, all compile + run. |
| 5 | ~~OpenTelemetry SDK adoption~~ | ✅ Done 2026-07-09 | `opentelemetry` + `opentelemetry_sdk` + `opentelemetry-stdout` crates (0.32). `SdkTracerProvider` with `SimpleSpanProcessor` + stdout exporter. Global tracer provider set. 6 unit tests. No OTLP/gRPC deps. |
| 6 | ~~`tracing-opentelemetry` bridge~~ | ✅ Done 2026-07-09 | `tracing-opentelemetry = "0.33"` added. `otel_layer` wired into both JSON and Human subscriber arms in `logging.rs`. No trait-bound workaround needed. `cargo check` clean. |
| 7 | Correlation engine — event/series/macro graph | ⬜ Deferred | The P1 partial is the ticker-prefix heuristic. No data source identified for the full graph. Accepted limitation. |
| 8 | Slim cache to `PrizePicksMarketSummary` | ⬜ Deferred | Optional Phase 3 optimization. |
| 9 | Persist summary cache to SQLite | ⬜ Deferred | Instant next-launch paint. Depends on the slim-cache item above. |
## Milestone Tracking Format

- `[ ]` Not started
- `[/]` In progress
- `[x]` Done
- `[!]` Blocked / needs decision