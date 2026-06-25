# PrizePicks Monster — Priority Roadmap

Last updated: 2026-06-25 (maintenance pass; model disagreement flags implemented in PrizePicksTradeDecision::compute + test coverage; marked P2 item done)
Working copy: `C:\\Projects\\prizepicks-monster`
Commit: `e941fb7`

Quick status: **P0 done · P1 mostly done (1 partial) · P2 3/4 done · P3 not started**

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
| **P2** | CLV per prediction | `eval-cli` scores closing-line value on benchmark data; live predictions don't store entry vs close | ⬜ Not started |
| **P3** | Volatility-adjusted Kelly from historical Brier | Shrinkage slider is manual; handoffs call for Brier-driven auto-shrinkage | ⬜ Not started |
| **P3** | Multi-category ML classifiers (politics/econ/weather) | Current ML is scikit-learn on sports prop features via Python subprocess; README still lists ML training as unchecked | ⬜ Not started |

---

## Remaining count

| Tier | Done | Remaining |
|------|------|-----------|
| P0 | 2 | **0** |
| P1 | 3 (+1 partial) | **0–1** |
| P2 | 3 | **1** |
| P3 | 0 | **2** |

**4–5 items left** (4 if heuristic correlation counts as P1-complete).

---

## P0 implementation notes (shipped)

- `src-tauri/src/prizepicks/grading.rs` — contract-side grading, binary PnL, `grade_pending_predictions`, `spawn_auto_grade_task`
- `src-tauri/src/prizepicks/models.rs` — `contract_side`, `market_price_at_entry` on predictions
- `src-tauri/src/predictions/tracker.rs` — rich `PrizePicksTradeDecision` extraction
- `src-tauri/src/lib.rs` — auto-grade task on startup

---

## P1 implementation notes (shipped)

- `src-tauri/src/prizepicks/portfolio_risk.rs` — Kelly scaling (event 0.50, series 0.75, category 0.90, same-ticker 0.85)
- `src-tauri/src/analysis/calibration.rs` — isotonic calibrator wired into PrizePicks paper trades
- `src-tauri/src/prizepicks/price_tracker.rs` — snapshots on `prizepicks_refresh`, `prizepicks_get_price_history`
- UI: `src-ui/src/components/PrizePicksView.tsx`, `MarketDetailPanel.tsx`, `PrizePicksPredictionsPanel.tsx`, `PriceHistoryChart.tsx`

**P1 gap:** ticker-prefix heuristics only — no macro/political/event-graph correlation yet.

---

## P2 implementation notes (shipped)

- `src-tauri/src/chat/decision_schema.rs` — `model_disagreement: bool` and `disagreement_points: f64` now computed in `PrizePicksTradeDecision::compute()` (and thus `compute_risk_adjusted`); threshold >12pp divergence between fair_probability_pct and market_price_pct. Test coverage in `test_contract_side_no_ev`. Serialized via full_decision_json on paper trade record.

---

## Suggested next target: P2

Highest leverage for paper-sim trustworthiness:

1. CLV per prediction (entry vs close) — build on existing price snapshots and market_price_at_entry
2. (done) Model disagreement flags at entry

---

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
