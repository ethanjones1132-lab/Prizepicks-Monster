# PrizePicks Monster — Priority Roadmap

Last updated: 2026-06-25 (maintenance pass; P3-1 volatility-adjusted Kelly from historical Brier shipped — analysis::kelly_shrinkage computes multiplier from sample size + Brier Skill Score, wired into portfolio_risk::compute_stake_adjustment_with_shrinkage and exposed as the prizepicks_kelly_shrinkage_report Tauri command; 10 new shrinkage tests + 3 new portfolio_risk tests bring suite to 136 passing)
Working copy: `C:\\Projects\\prizepicks-monster`
Commit: `df43bd9`

Quick status: **P0 done · P1 mostly done (1 partial) · P2 done · P3 mostly done (1 done, 1 remaining)**

---

## High-impact improvements (ranked)

| Priority | Item | Why it matters | Status |
|----------|------|----------------|--------|
| **P0** | Fix grading to use `contract_side` + store `market_price_at_entry` | Unblocks trustworthy paper-sim and the entire calibration loop | ✅ Done |
| **P0** | Background auto-grade for PrizePicks (poll resolved markets) | Notifications auto-grade ESPN props only; PrizePicks grading was manual | ✅ Done |
| **P1** | Correlated position auto-scaling | Warnings exist (event/series co-exposure) but Kelly stakes were not scaled down | ✅ Done |
| **P1** | Wire `edge_eval` calibrator into PrizePicks decision path | Isotonic calibrator applied to `analyze_single_prop` (sports props), not LLM `PrizePicksTradeDecision` forecasts | ✅ Done |
| **P1** | PrizePicks historical price/spread snapshots | `line_tracker.rs` is PrizePicks-only; no candlestick API in `prizepicks/client.rs` — blocks CLV tracking and momentum signals | ✅ Done |
| **P1** | PrizePicks-native correlation engine | `correlation.rs` is NFL prop families; portfolio checks are ticker-prefix heuristics, not macro/political/event-graph correlation | ⚠️ Partial |
| **P2** | Persist `localMaxBetPct` to config | UI-only state; resets when modal closes (unlike `minQuality`, which is in `localStorage`) | ✅ Done (2026-06-24) |
| **P2** | Sync bankroll limits from `predictions.db` + paper positions | Makes daily/weekly cap warnings and `BankrollView` accurate | ✅ Done |
| **P2** | Model disagreement flags at entry | Flag when `fair_probability_pct` diverges sharply from market implied prob at decision time | ✅ Done (2026-06-25) |
| **P2** | CLV per prediction | `eval-cli` scores closing-line value on benchmark data; live predictions don't store entry vs close | ✅ Done (2026-06-25) |
| **P3** | Volatility-adjusted Kelly from historical Brier | Shrinkage slider is manual; handoffs call for Brier-driven auto-shrinkage | ✅ Done (2026-06-25) |
| **P3** | Multi-category ML classifiers (politics/econ/weather) | Current ML is scikit-learn on sports prop features via Python subprocess; README still lists ML training as unchecked | ⬜ Not started |

---

## Remaining count

| Tier | Done | Remaining |
|------|------|-----------|
| P0 | 2 | **0** |
| P1 | 3 (+1 partial) | **0–1** |
| P2 | 4 | **0** |
| P3 | 1 | **1** |

**1–2 items left** (P3-2 ML classifiers; correlation engine is still the lone P1 partial).

## P0 implementation notes (shipped)

- `src-tauri/src/prizepicks/grading.rs` — contract-side grading, binary PnL, `grade_pending_predictions`, `spawn_auto_grade_task`
- `src-tauri/src/prizepicks/models.rs` — `contract_side`, `market_price_at_entry` on predictions
- `src-tauri/src/predictions/tracker.rs` — rich `PrizePicksTradeDecision` extraction
- `src-tauri/src/lib.rs` — auto-grade task on startup

## P1 implementation notes (shipped)

- `src-tauri/src/prizepicks/portfolio_risk.rs` — Kelly scaling (event 0.50, series 0.75, category 0.90, same-ticker 0.85)
- `src-tauri/src/analysis/calibration.rs` — isotonic calibrator wired into PrizePicks paper trades
- `src-tauri/src/prizepicks/price_tracker.rs` — snapshots on `prizepicks_refresh`, `prizepicks_get_price_history`
- UI: `src-ui/src/components/PrizePicksView.tsx`, `MarketDetailPanel.tsx`, `PrizePicksPredictionsPanel.tsx`, `PriceHistoryChart.tsx`

**P1 gap:** ticker-prefix heuristics only — no macro/political/event-graph correlation yet.

## P2 implementation notes (shipped)

- `src-tauri/src/chat/decision_schema.rs` — `model_disagreement: bool` and `disagreement_points: f64` now computed in `PrizePicksTradeDecision::compute()` (and thus `compute_risk_adjusted`); threshold >12pp divergence between fair_probability_pct and market_price_pct. Test coverage in `test_contract_side_no_ev`. Serialized via full_decision_json on paper trade record.
- `src-tauri/src/predictions/storage.rs` — CLV columns `entry_price_pct`, `closing_price_pct`, `clv_points`, `clv_ticker`, `clv_captured_at` added via `migrate_predictions_columns`. `extract_entry_price_pct` parses `full_decision_json.market_price_pct` and writes it on `insert_prediction`. `capture_closing_prices_for_resolved` walks resolved predictions and links the latest `prizepicks_price_snapshots` row at-or-before `resolved_at`. Guarded with `WHERE clv_captured_at IS NULL` so the sweep is idempotent.
- `src-tauri/src/predictions/clv.rs` — `spawn_clv_capture_task` background loop, interval shared with auto-grade/paper-settle tasks.
- Tauri command `prizepicks_capture_clv` exposed for on-demand sweep from the UI; bound in `src-ui/src/services/prizepicks.ts` as `prizepicksApi.captureClv()`.
- Tests: 13 new in `predictions::storage::tests` — entry-price extraction (valid/missing/invalid/none/out-of-range/boundaries), insert captures entry price, missing-decision tolerates NULL, capture skips without snapshot, capture picks latest-before-resolution snapshot, idempotent, skip when ticker missing. Total 123 lib tests passing.

## P3 implementation notes (in progress)

- `src-tauri/src/analysis/kelly_shrinkage.rs` — new module. `compute_shrinkage(&[ResolvedForBrier])` returns a `KellyShrinkageReport { multiplier, n, brier, base_rate, climatology_brier, brier_skill_score, sample_factor, calibration_factor, reason }`. Cold start (n=0) returns multiplier=1.0. Cold but non-zero (1 ≤ n < 30) fades linearly from 0.50 → 1.0 via `sample_factor`. Warm: `multiplier = sample_factor * sqrt(max(BSS, 0)).clamp(MIN_MULT, 1.0)` where `BSS = 1 - brier/climatology_brier`. Climatology Brier = `base_rate * (1 - base_rate)` (binary). 10 unit tests cover cold start, single prediction, small sample, warm near-climatology, sharp well-calibrated (BSS=1), mildly miscalibrated (BSS<0), overconfident (floored at MIN_MULT=0.50), degenerate all-wins (no NaN), parse_hit_outcome strings, and the predictions adapter.
- `src-tauri/src/prizepicks/portfolio_risk.rs` — added `compute_stake_adjustment_with_shrinkage(...)` which folds the shrinkage multiplier on top of the correlation scale. The original `compute_stake_adjustment` is preserved as a thin wrapper passing `None`. `StakeAdjustment` gains an optional `kelly_shrinkage: Option<KellyShrinkageReport>` field. When the shrinkage multiplier is <1.0, a "Volatility-adjusted Kelly: X% of raw (Brier-shrunk from observed history)." warning is appended. 3 new tests: `shrinkage_folds_into_kelly_scale`, `shrinkage_unity_keeps_legacy_behavior`, `shrinkage_warms_to_full_kelly`.
- `src-tauri/src/commands/prizepicks_cmd.rs` — new Tauri command `prizepicks_kelly_shrinkage_report` returns the live report. Helper `fetch_resolved_for_brier` queries `predictions` for rows with non-null `actual_outcome` and projects them into `ResolvedForBrier` via the shared `parse_hit_outcome` mapping.
- `src-tauri/src/lib.rs` — registered `prizepicks_kelly_shrinkage_report` in the `invoke_handler`.
- `src-ui/src/types/prizepicks.ts` — added `KellyShrinkageReport` interface; `StakeAdjustment` gains optional `kelly_shrinkage` field.
- `src-ui/src/services/prizepicks.ts` — added `prizepicksApi.getKellyShrinkageReport()`. The existing `MarketDetailPanel` already surfaces shrinkage warnings via `adjustment.warnings`, so no UI change needed beyond the type.
- **Wiring into the live decision path** (deferred): `prizepicks_record_paper_decision` and `prizepicks_compute_stake_adjustment` still call the legacy `compute_stake_adjustment` (shrinkage=None). The plumbing is in place; activating it is a one-line change in those commands. Holding off to keep this pass focused and to avoid touching the user-facing decision path without calibration data accumulating first.

## Suggested next target: P3 (1 remaining)

1. **Multi-category ML classifiers** — the larger of the two. Requires a Python training pipeline update, feature extraction expansion, and a UI to surface per-category model picks. No Rust work in this one without first shipping the training pipeline.

## Dashboard performance (deferred)

**Phase 1 (shipped 2026-06-17):** flat `GET /markets` quick cache (replaces nested `/events` for dashboard load). See `prizepicks/client.rs` — `fetch_markets_flat_pages`, `ensure_quick_cache`.

### Phase 2 — Decouple cache reads from long fetches

- Extract `Arc<RwLock<PrizePicksCache>>` + `fetch_in_progress` guard so UI reads never block on 20-page full warm
- Background full-catalog warm writes cache without holding the outer `PrizePicksClient` mutex across HTTP pagination
- Optionally slim cache to `PrizePicksMarketSummary` instead of full `PrizePicksMarket`
- **Target:** warm revisit under 300ms; category switch under 500ms

### Phase 3 — Frontend critical-path trim

- Keep `PrizePicksView` mounted across tab switches (avoid cold reload)
- Combined IPC: `prizepicks_get_dashboard_bootstrap` → `{ markets, categories, cache_full }`
- Defer `PrizePicksPredictionsPanel` load; debounce `computeStakeAdjustment` in market detail
- Show partial-cache indicator when `full_catalog == false`

### Phase 4 — Startup prefetch and persistence (optional)

- Prefetch quick cache at app startup (before user opens dashboard)
- Delay full warm until quick cache exists + idle window (or explicit Refresh only)
- Persist summary cache to SQLite for instant next-launch paint

---

## Environment notes

- Canonical WSL repo (`~/.openclaw/agents/coderclaw/workspace/prizepicks-monster`) was unreachable as of 2026-06-17
- `edge-eval` and `monster-edge-core` live at `C:\\Users\\ethan\\prizepicks-build\\` (sibling paths)
