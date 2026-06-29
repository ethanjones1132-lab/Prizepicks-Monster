# PrizePicks Monster — Priority Roadmap

Last updated: 2026-06-29 (afternoon maintenance pass #2; **Per-player paper performance breakdown shipped** — `PaperAnalytics` gained a new `player_stats: Vec<PaperPlayerStats>` field that buckets closed + open lots by player name extracted from the lot's `title` (`"<name> Over|Under <line> <stat>"` pattern) with an `"Unknown"` fallback for unparseable titles; new `extract_player_name(&PaperLot) -> String` parser (case-insensitive, defensive: empty/whitespace/no-separator → "Unknown"); new `PaperPlayerStats { player, total_trades, open_trades, wins, losses, win_rate, realized_pnl, total_staked, roi_pct }` struct serialized as snake_case; new `compute_player_stats(&[PaperLot])` helper that emits players sorted by `realized_pnl` DESC (alpha tiebreak); 14 new unit tests (7 parser, 7 stats); **216 lib tests pass** (was 202), tsc clean, ad-hoc verification 35/35)
Working copy: `C:\\Projects\\prizepicks-monster`
Commit: `761784e`
Quick status: **P0 done · P1 mostly done (1 partial) · P2 done · P3 done · Phase 3 partial-cache indicator done · Phase 3 combined IPC done · Phase 4 startup prefetch done · Per-category paper breakdown done · Per-side paper breakdown done · Per-window session PnL chips done · Per-hold-time paper breakdown done · Per-player paper breakdown done**

## 2026-06-29 afternoon pass #2 — Per-player paper performance breakdown

- `src-tauri/src/paper/mod.rs` — new `extract_player_name(&PaperLot) -> String` parser (case-insensitive, finds earliest " over " or " under " in the lot's `title` and returns the trimmed prefix; routes empty/whitespace/no-separator/empty-prefix titles to `"Unknown"`). New `PaperPlayerStats { player, total_trades, open_trades, wins, losses, win_rate, realized_pnl, total_staked, roi_pct }` struct (Serialize/Deserialize/PartialEq). New `compute_player_stats(&[PaperLot])` helper that buckets by extracted player name, applies the same win/loss/PnL/staked aggregation as the other breakdown helpers (pushes contribute stake but not wins/losses, open lots counted but excluded from PnL), and sorts the result by `realized_pnl` DESC with alphabetical tiebreak. Wired into `get_analytics` and exposed as `PaperAnalytics.player_stats: Vec<PaperPlayerStats>`. 14 new unit tests (7 parser + 7 stats): parser covers " Over " separator, " Under " separator, case-insensitive, empty title, whitespace-only title, no-separator → "Unknown", and prefix whitespace trim; stats cover empty input, multi-player bucketing, PnL-DESC sort with alphabetical tiebreak, unparseable-title "Unknown" bucket, open-lot handling, win_rate/ROI computation (2 wins + 1 loss with stake math), and push-stake handling. **216 lib tests pass** (was 202).
- `src-ui/src/types/prizepicks.ts` — added `PaperPlayerStats` interface; `PaperAnalytics` gains the `player_stats: PaperPlayerStats[]` field to match the Rust struct.
- `src-ui/src/components/PrizePicksPredictionsPanel.tsx` — new `PlayerBreakdown` inner component renders a five-column table (Player / Trades / Win % / PnL / ROI) with green/red tints matching the equity curve's color scheme. Open lot count is shown as a small `+N open` tag next to the player name. Empty-state copy guides the user to place paper trades. Mounted directly under the existing `HoldTimeBreakdown`, gated on `analytics` being loaded. The four performance tables (Category, Side, HoldTime, Player) now complement each other — per-player answers "which players am I actually making money on?" and is the most common follow-up question a sharp prop user has after seeing per-category or per-hold-time performance.
- `src-ui/src/index.css` — added `.playerBreakdown`, `.playerBreakdownHeader`, `.playerTable` + th/td styles, `.playerTable td.pos/.neg` color variants, `.playerOpenTag`, and tabular-nums + 600 font-weight on PnL/ROI cells using the existing `--pos` / `--neg` CSS variables. Same visual weight as `.categoryBreakdown` and `.sideBreakdown` (12px padding, 14px border-radius) so all four performance tables read as siblings.
- Ad-hoc verification (focused on the new behavior, NOT canonical suite green): `hermes-verify-2026-06-29-player.py` — 35/35 checks pass covering Rust struct/function/wiring presence, all 14 new unit tests present, TypeScript types + UI wiring, CSS classes, and a re-run of `cargo check`.

## 2026-06-29 afternoon pass — Per-hold-time paper performance breakdown

- `src-tauri/src/paper/mod.rs` — new `PaperHoldTimeBucket` enum (`Intraday` / `SameDay` / `MultiDay` / `Long` / `Unknown`, serialized as snake_case) with `from_seconds(secs)` classifier (boundaries 1h/24h/7d exclusive at the upper end so a 1h-exact lot is `SameDay`) and `as_label()` human-readable labels. New `PaperHoldTimeStats { bucket, bucket_label, total_trades, open_trades, wins, losses, win_rate, realized_pnl, total_staked, roi_pct, avg_hold_seconds, median_hold_seconds }` struct (Serialize/Deserialize/PartialEq). New `lot_hold_seconds(&PaperLot) -> Option<f64>` parses RFC 3339 open/close timestamps and returns `None` for missing/unparseable/negative-delta lots (defensively routed to `Unknown`). New `compute_hold_time_stats(&[PaperLot]) -> Vec<PaperHoldTimeStats>` buckets lots, computes win/loss/PnL/staked aggregates identically to `compute_category_stats`, and tracks per-bucket hold-duration samples for avg + median (median handles odd/even sample counts). Output is emitted in chronological bucket order (Intraday → SameDay → MultiDay → Long → Unknown) so the UI can render a stable "fastest to slowest" ladder without resorting. `PaperAnalytics` gains `hold_time_stats: Vec<PaperHoldTimeStats>`. 12 new unit tests cover: empty input, 4-bucket classification, bucket-boundary exclusive-at-upper-end (1h→SameDay, 24h→MultiDay, 7d→Long), zero-hold (0s → Intraday), negative-hold → Unknown with realized_pnl still recorded, unparseable timestamps → Unknown with wins/PnL/staked still counted, open-lot routed to Unknown (open-lot hold is still ticking), push (pnl=0) contributing stake + hold sample but not win/loss, avg/median with odd sample count, median with even sample count, per-bucket win-rate/ROI, and bucket omission (only populated buckets appear in the result). **202 lib tests pass** (was 190).
- `src-ui/src/types/prizepicks.ts` — new `PaperHoldTimeStats` interface; `PaperAnalytics` gains the `hold_time_stats: PaperHoldTimeStats[]` field. The interface mirrors the Rust struct 1:1 (snake_case fields), with `bucket_label` alongside the raw `bucket` identifier so the UI doesn't have to hard-code the human-readable mapping.
- `src-ui/src/components/PrizePicksPredictionsPanel.tsx` — new `formatHoldSeconds(secs)` helper renders hold duration as `Xs`/`Xm`/`X.Yh`/`X.Yd` (sub-minute stays in seconds, hours roll into days for legibility). New `HoldTimeBreakdown` inner component renders a 6-column table (Hold time / Trades / Win % / PnL / ROI / Avg / Median hold) mirroring the `CategoryBreakdown` and `SideBreakdown` table style so all three read as siblings. Renders `bucket_label` (e.g. `"Intraday (≤1h)"`) so the time windows are visible in the table without a legend. Open-lot count surfaces as a `+N open` muted tag next to the bucket label. Empty-state copy guides the user to place or settle paper trades. Mounted directly under the existing `SideBreakdown` table, gated on `analytics` being loaded.
- `src-ui/src/index.css` — added `.holdTimeBreakdown`, `.holdTimeBreakdownHeader`, `.holdTimeTable` + th/td styles, `.holdTimeOpenTag`, and pos/neg color variants using the existing `--pos` / `--neg` CSS variables. Same visual weight as `.sideBreakdown` and `.categoryBreakdown` (12px padding, 14px border-radius, 1px subtle border, rgba(255,255,255,0.04) row separators) so all three performance tables read as siblings.
- Ad-hoc verification (focused on the new behavior, NOT canonical suite green): 24/24 grep checks covering TS interface fields, React import + component + summary cell, mount point, bucket_label rendering (not raw bucket), CSS class presence, Rust enum/struct/helper/field/4-canonical-buckets, ≥10 hold-time tests (12 found), and a re-run of `cargo check`.

## 2026-06-27 evening pass — Streak indicator

- `src-tauri/src/paper/mod.rs` — new `PaperStreak { kind, length }` struct (Serialize/Deserialize). New `compute_current_streak(&[PaperLot])` walks DESC closed lots: seeds the streak on the first closed lot, increments while the sign matches, and returns the prior run as soon as the sign disagrees. Open lots are skipped. Pushes (realized_pnl == 0) are walked past so a single push doesn't erase a meaningful streak. Wired into `get_analytics` so the field is part of every `PaperAnalytics` payload. 8 new unit tests cover empty input, only-open-lots, all-wins, stop-at-first-loss, full loss streak, push-at-front with/without prior wins, push-after-wins preserves streak, and skipping open lots. **167 lib tests pass** (was 158).
- `src-ui/src/types/prizepicks.ts` — added `PaperStreak` interface; brought `PaperAnalytics` into sync with the Rust struct (added the previously-missing `avg_winner`, `avg_loser`, `largest_winner`, `largest_loser` fields, plus the new `current_streak`).
- `src-ui/src/components/PrizePicksPredictionsPanel.tsx` — new `StreakChip` inner component renders `W3` (green pos tint), `L2` (red neg tint), or `—` (muted) for an empty streak. Mounted as a new `Streak` cell in the `paperSummary` row alongside Paper equity / Cash / Open / Return / Win rate / Max DD.
- `src-ui/src/index.css` — `.streakChip` + `.pos/.neg/.muted` variants (pill, tinted border, themed background).

---

## 2026-06-28 afternoon pass #2 — Per-side paper performance breakdown

- `src-tauri/src/paper/mod.rs` — new `PaperSideStats { side, total_trades, open_trades, wins, losses, win_rate, realized_pnl, total_staked, roi_pct }` struct (Serialize/Deserialize/PartialEq) and `compute_side_stats(&[PaperLot])` helper. Buckets all lots by raw `side` (empty/whitespace → "Unknown"), aggregates wins/losses/realized PnL/total staked for closed lots, counts open lots separately, computes win_rate and roi_pct identically to `compute_category_stats`. Result sorted by `realized_pnl` DESC, ties broken alphabetically for deterministic output. Wired into `get_analytics` and exposed as `PaperAnalytics.side_stats: Vec<PaperSideStats>`. 8 new unit tests cover empty input, sort by PnL DESC, alphabetical tie-break, win rate / ROI computation (including push handling), open-lot exclusion, empty / whitespace side → "Unknown" bucket, only-pushes / zero-PnL edge case, and YES/NO split with mixed wins and losses. **182 lib tests pass** (was 174).
- `src-ui/src/types/prizepicks.ts` — added `PaperSideStats` interface; `PaperAnalytics` gains the `side_stats: PaperSideStats[]` field. New helper `paperSideLabel(side)` maps the raw backend "YES" → "Over" and "NO" → "Under" so the data layer stays platform-agnostic.
- `src-ui/src/components/PrizePicksPredictionsPanel.tsx` — new `SideBreakdown` inner component renders a five-column table (Side / Trades / Win % / PnL / ROI) with green/red tints matching the equity curve's color scheme. Open lot count is shown as a small `+N open` tag next to the side label. Empty-state copy guides the user to place paper trades. Mounted directly under the existing `CategoryBreakdown` table, gated on `analytics` being loaded. The two tables now complement each other: per-category answers "where is the edge?" (Points, Rebounds, etc.) and per-side answers "am I better at picking Overs or Unders?".
- `src-ui/src/index.css` — added `.sideBreakdown`, `.sideBreakdownHeader`, `.sideTable` + th/td styles, `.sideOpenTag`, and pos/neg color variants using the existing `--pos` / `--neg` CSS variables. Same visual weight as `.categoryBreakdown` (12px padding, 14px border-radius) so the two read as siblings.
- Ad-hoc verification (focused on the new behavior, NOT canonical suite green): `bash hermes-verify-2026-06-28-side-breakdown.sh` — 23/23 checks pass covering Rust struct/function presence, all 8 unit tests present, TypeScript types + UI wiring, CSS classes, no raw side leak in the table row (UI must use `paperSideLabel()`), and a re-run of `cargo check`.

## 2026-06-28 afternoon pass — Per-category paper performance breakdown

- `src-tauri/src/paper/mod.rs` — new `PaperCategoryStats { category, total_trades, open_trades, wins, losses, win_rate, realized_pnl, total_staked, roi_pct }` struct (Serialize/Deserialize/PartialEq). New `compute_category_stats(&[PaperLot])` buckets all lots by category (`""` / whitespace → `"Other"`), aggregates wins/losses/realized PnL/total staked for closed lots, counts open lots separately, computes win_rate = wins / (wins + losses) * 100, and roi_pct = realized_pnl / total_staked * 100. Pushes (realized_pnl == 0) don't count as wins or losses but their stake is included in `total_staked`. Open lots are excluded from the ROI denominator — only closed stake counts. Result sorted by `realized_pnl` DESC, ties broken alphabetically for deterministic output. Wired into `get_analytics` and exposed as `PaperAnalytics.category_stats: Vec<PaperCategoryStats>`. 7 new unit tests cover empty input, sort by PnL DESC, alphabetical tie-break, win rate / ROI computation (including push handling), open-lot exclusion, empty / whitespace category → "Other" bucket, and zero-decided / zero-PnL edge cases. **174 lib tests pass** (was 167).
- `src-ui/src/types/prizepicks.ts` — added `PaperCategoryStats` interface; `PaperAnalytics` gains the `category_stats: PaperCategoryStats[]` field to match the Rust struct.
- `src-ui/src/components/PrizePicksPredictionsPanel.tsx` — new `CategoryBreakdown` inner component renders a five-column table (Category / Trades / Win % / PnL / ROI) with green/red tints matching the equity curve's color scheme. Open lot count is shown as a small `+N open` tag next to the category name. Empty-state copy guides the user to place paper trades. Mounted between the equity curve and the prediction list, gated on `analytics` being loaded.
- `src-ui/src/index.css` — added `.categoryBreakdown`, `.categoryBreakdownHeader`, `.categoryTable` + th/td styles, `.categoryOpenTag`, and pos/neg color variants using the existing `--pos` / `--neg` CSS variables. Compact 12px padding / 14px border-radius to match the equity curve's visual weight.

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
|    | **P3** | Multi-category ML classifiers (politics/econ/weather) | Current ML is scikit-learn on sports prop features via Python subprocess; stat_category one-hot features added 2026-06-26; UI surface (ML predictor tab) added 2026-06-26; Kelly shrinkage wired into the live decision path on 2026-06-26 (was an unblocked deferred item from this P3 row) | ✅ Done (2026-06-26) |

---

## Remaining count

| Tier | Done | Remaining |
|------|------|-----------|
| P0 | 2 | **0** |
| P1 | 3 (+1 partial) | **0–1** |
| P2 | 4 | **0** |
| P3 | 2 | **0** |

**0–1 items left** (P3-2 shipped 2026-06-26; correlation engine is still the lone P1 partial — no proposed implementation yet, accepted limitation).

**Next brainstorm candidates** (in rough ROI order — pick the highest-impact one in a future clean-tree pass):
1. ~~**Hold-time / per-duration-bucket performance**~~ — ✅ shipped 2026-06-29 (see below).
2. ~~**Player-level PnL breakdown**~~ — ✅ shipped 2026-06-29 afternoon #2.
3. **Per-entry-price-bucket performance** — bucket by entry_price_cents ranges (e.g. 30-50¢, 50-70¢, 70-90¢) to show whether long-shots or favorites are more profitable. ~1h scope.
4. **Decision-quality vs. result scatter** — for each closed lot, plot `predicted_probability` (x) vs. `realized_pnl` (y) to visualize the calibration curve in the UI surface. The data already exists in `predictions` + `paper_lots`; this is a charting/visualization task. ~2h scope.

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

- `src-tauri/src/ml_predictor.py` — stat_category now one-hot encoded as categorical features (2026-06-26). Feature extraction dynamically detects unique stat categories from resolved predictions and adds binary columns. The category map is persisted to `_meta.json` alongside the trained model so `predict_batch` can construct identical feature vectors. Unknown categories during inference get all-zeros. `export-features` includes category columns in the CSV. `train_model` message now reports feature count and category count. The numeric features are unchanged (13 original + N category one-hots).
- `src-tauri/src/analysis/kelly_shrinkage.rs` — new module. `compute_shrinkage(&[ResolvedForBrier])` returns a `KellyShrinkageReport { multiplier, n, brier, base_rate, climatology_brier, brier_skill_score, sample_factor, calibration_factor, reason }`. Cold start (n=0) returns multiplier=1.0. Cold but non-zero (1 ≤ n < 30) fades linearly from 0.50 → 1.0 via `sample_factor`. Warm: `multiplier = sample_factor * sqrt(max(BSS, 0)).clamp(MIN_MULT, 1.0)` where `BSS = 1 - brier/climatology_brier`. Climatology Brier = `base_rate * (1 - base_rate)` (binary). 10 unit tests cover cold start, single prediction, small sample, warm near-climatology, sharp well-calibrated (BSS=1), mildly miscalibrated (BSS<0), overconfident (floored at MIN_MULT=0.50), degenerate all-wins (no NaN), parse_hit_outcome strings, and the predictions adapter.
- `src-tauri/src/prizepicks/portfolio_risk.rs` — added `compute_stake_adjustment_with_shrinkage(...)` which folds the shrinkage multiplier on top of the correlation scale. The original `compute_stake_adjustment` is preserved as a thin wrapper passing `None`. `StakeAdjustment` gains an optional `kelly_shrinkage: Option<KellyShrinkageReport>` field. When the shrinkage multiplier is <1.0, a "Volatility-adjusted Kelly: X% of raw (Brier-shrunk from observed history)." warning is appended. 3 new tests: `shrinkage_folds_into_kelly_scale`, `shrinkage_unity_keeps_legacy_behavior`, `shrinkage_warms_to_full_kelly`.
- `src-tauri/src/commands/prizepicks_cmd.rs` — new Tauri command `prizepicks_kelly_shrinkage_report` returns the live report. Helper `fetch_resolved_for_brier` queries `predictions` for rows with resolved `outcome` values (Win/Loss/Push) and projects them into `ResolvedForBrier` via the shared `parse_hit_outcome` mapping, reading the predicted probability from the `probability` column.
- `src-tauri/src/lib.rs` — registered `prizepicks_kelly_shrinkage_report` in the `invoke_handler`.
- `src-ui/src/types/prizepicks.ts` — added `KellyShrinkageReport` interface; `StakeAdjustment` gains optional `kelly_shrinkage` field.
- `src-ui/src/services/prizepicks.ts` — added `prizepicksApi.getKellyShrinkageReport()`. The existing `MarketDetailPanel` already surfaces shrinkage warnings via `adjustment.warnings`, so no UI change needed beyond the type.
- **Wiring into the live decision path** (✅ Done 2026-06-26): `prizepicks_record_paper_decision` and `prizepicks_compute_stake_adjustment` now fetch resolved predictions via the shared `fetch_resolved_for_brier` helper, build a `KellyShrinkageReport` with `kelly_shrinkage::compute_shrinkage`, and pass it to `compute_stake_adjustment_with_shrinkage`. `prizepicks_compute_stake_adjustment` gained a `db_pool: State<'_, Pool<Sqlite>>` parameter (Tauri injects it automatically; no UI change required). The returned `StakeAdjustment` now includes `kelly_shrinkage` on every adjustment, and the `MarketDetailPanel` was already wired to surface shrinkage warnings via `adjustment.warnings`.
- **Bug fix in `fetch_resolved_for_brier`** (✅ Done 2026-06-26): The helper previously queried the in-memory struct field names (`predicted_probability` / `actual_outcome`) instead of the actual production schema columns (`probability` / `outcome`). The columns don't exist in the DB, so the helper silently returned an empty Vec and `compute_shrinkage` always produced the cold-start multiplier (1.0). Fixed the SELECT, the WHERE clause, and both `try_get` column names. 3 new unit tests added under `commands::prizepicks_cmd::tests`: `fetch_resolved_reads_production_schema_columns` (mix of Win/Loss/Push/Pending rows; assert only the 4 resolved rows are returned and probability values are read from the `probability` column), `fetch_resolved_empty_pool_returns_empty`, `fetch_resolved_filters_pending_rows`. Total **146 lib tests passing** (was 143).
- **ML predictor UI surface (2026-06-26):**
  - `src-ui/src/types/prizepicks.ts` — added `MLFeatureImportance`, `MLTrainingResult`, `MLPrediction`, `MLPredictionBatch`, `MLModelStatus`.
  - `src-ui/src/services/prizepicks.ts` — `prizepicksApi.mlTrainModel(outputPath?)`, `mlPredictBatch()`, `mlGetModelStatus()`, `mlGetPredictions(limit?)`, `mlExportFeatures(outputPath?)` wrapping the existing Tauri commands.
  - `src-ui/src/components/MLPredictorPanel.tsx` — new component. Header summary card (model trained, sample count, CV accuracy ± std, training win rate, pending vs resolved, trained-at), top-10 feature importances table, latest 20 ML predictions table with Over/Under lean chip (green=Lean Over ≥0.6, gold=0.4–0.6 toss-up, red=Lean Under <0.4), three actions: Train model (disabled if <10 resolved), Score pending (disabled if no model), Export features CSV. Empty-state copy guides next steps when no model / no predictions.
  - `src-ui/src/App.tsx` — new `ml` tab `🤖 ML predictor` in the sidebar nav, mounted as a `prizepicksPage` section.
  - `src-ui/src/index.css` — added `.featureImportanceBlock`, `.featureTable`, `.predictionTable`, `.info` (success-tinted status banner), and `.chip.small.leanOver / .leanUnder / .leanToss` color variants.
  - `src-tauri/src/ml_predictor.rs` — added `pub fn model_meta_path_for(...)` (test-only wrapper around the existing private path-derivation helper) and a `#[cfg(test)] mod tests` block with 7 new unit tests: `model_meta_path_strips_joblib_and_appends_meta_json`, `model_meta_path_handles_alternate_filename`, `model_meta_path_preserves_directory` (with a `paths_eq` helper that ignores `/` vs `\` so the assertions are cross-platform), `ml_context_with_empty_predictions_returns_empty_string`, `ml_context_includes_accuracy_when_provided`, `ml_context_uses_na_when_accuracy_missing`, `ml_context_caps_at_ten_predictions`. Total **143 lib tests passing** (was 136).
  - **Multi-category training pipeline** (deferred): The Python script still trains a single `GradientBoostingClassifier` per stat_category via the one-hot expansion. True per-category classifiers (separate model files per `points` / `rebounds` / etc.) require routing changes in `ml_predictor.py` and per-category feature importances. Not on the maintenance critical path; deferred to a future pass.
  - **Train button gating:** the panel disables "Train model" until at least 10 resolved predictions exist in the DB (matches the Python script's `len(X) < 10` early return), and disables "Score pending" until a model file is on disk.
- **Per-category training pipeline (✅ Done 2026-06-26):**
  - `src-tauri/src/ml_predictor.py` — added `train_per_category_model(db_path, output_dir, min_samples=10)`, `predict_batch_per_category(db_path, model_dir)`, `list_category_models(model_dir)`, plus three matching CLI subcommands (`train-per-category`, `predict-per-category`, `list-category-models`). `extract_features_by_category` strips the one-hot `stat_category__<name>` columns added by `extract_features_from_db` so each per-category model only has to learn the 13 numeric features. Filenames are tokenized via `_safe_filename_token` (alphanumerics / `_` / `-` / `.` kept, other chars collapsed to `_`, edge-punctuation trimmed, empty → `uncategorized`).
  - `src-tauri/src/ml_predictor.rs` — added `MLCategoryModelResult`, `MLCategoryTrainResult`, `MLCategoryModelInfo`, `MLCategoryModelList`; new functions `train_per_category`, `list_category_models` (pure filesystem, globs `ml_model_*_meta.json` and skips the single-model `ml_model_meta.json` file), `predict_batch_per_category`, plus helpers `default_category_model_dir` (`~/.openclaw/prizepicks-monster/ml_models/`) and `safe_category_token` (mirrors the Python side).
  - `src-tauri/src/commands/ml_cmd.rs` — new Tauri commands `ml_train_per_category`, `ml_predict_batch_per_category` (also saves to `ml_predictions` table), `ml_get_category_models`.
  - `src-tauri/src/lib.rs` — registered the 3 new commands in `invoke_handler`.
  - `src-ui/src/types/prizepicks.ts` — added `MLCategoryTrainResult`, `MLCategoryModelList`, `MLCategoryModelInfo` types.
  - `src-ui/src/services/prizepicks.ts` — added `mlTrainPerCategory(outputDir?, minSamples?)`, `mlPredictBatchPerCategory()`, `mlGetCategoryModels()`.
  - `src-ui/src/components/MLPredictorPanel.tsx` — new "Per-category classifiers" section with two actions (`Train per-category`, `Score pending (per-category)`) and a table of per-category model metrics (stat category chip, sample count, CV accuracy ± std, win rate, trained-at). Train button is disabled when `resolved_predictions < 10`; score button is disabled until at least one per-category model is on disk. Load pulls `mlGetCategoryModels` in parallel with the existing status + predictions calls.

## Suggested next target: P1 (1 partial, no plan)

1. **PrizePicks-native correlation engine** — The existing `prizepicks/portfolio_risk.rs` correlation is ticker-prefix heuristics only. A proper implementation would need an event/series/macro graph (player-level correlations, team-level, same-game parlay structure) and a way to fetch it. No concrete plan in place. Most users of the current app have small (≤3 leg) paper positions where the heuristic is sufficient.

## Brainstormed & shipped (2026-06-29 afternoon #2)

- **Per-player paper performance breakdown** — `PaperAnalytics` already exposed `category_stats` (per stat category), `side_stats` (Over/Under), and `hold_time_stats` (Intraday→Long), but no view of "which players am I making money on?" A user with a sharp eye for a small set of players (their fantasy roster, their favorite team's stars, or a player they've been tracking for injury comebacks) might crush props on those names while bleeding on everyone else, and the existing summary couldn't show it. Shipped:
  - `src-tauri/src/paper/mod.rs` — new `extract_player_name(&PaperLot) -> String` parser. The existing lot `title` field follows the canonical PrizePicks pattern `"<Player Name> <Over|Under> <line> <stat>"` (e.g. `"Josh Allen Over 275.5 passing yards"`), and the parser finds the earliest " Over " or " Under " (case-insensitive, with surrounding whitespace) and returns the trimmed prefix. Defensive routing: empty/whitespace titles, titles with no separator, and titles where the prefix trims to empty all land under `"Unknown"`. New `PaperPlayerStats` struct mirroring `PaperCategoryStats` / `PaperSideStats` with the player name + the same 8 aggregates. New `compute_player_stats(&[PaperLot])` helper that emits players sorted by `realized_pnl` DESC with alphabetical tiebreak, using identical win/loss/PnL/staked aggregation semantics as the existing helpers. 14 new unit tests (7 parser + 7 stats).
  - `src-ui/src/types/prizepicks.ts` — `PaperPlayerStats` interface; `PaperAnalytics` gains the `player_stats` field.
  - `src-ui/src/components/PrizePicksPredictionsPanel.tsx` — new `PlayerBreakdown` inner component (five-column table: Player / Trades / Win % / PnL / ROI) with the same `+N open` tag and pos/neg tints as the existing breakdowns. Mounted under the existing `HoldTimeBreakdown` so the four performance tables read as siblings.
  - `src-ui/src/index.css` — `.playerBreakdown*` + `.playerTable*` + `.playerOpenTag` styles, same visual weight as the other breakdowns. Tabular nums + 600 weight on PnL/ROI cells so the dollar amounts align cleanly across rows.

## Brainstormed & shipped (2026-06-29 afternoon)

- **Per-hold-time paper performance breakdown** — `PaperAnalytics` already exposed `category_stats` and `side_stats`, but no view of "how long are my winning trades held?" A user with a sharp eye for in-game line movement might crush intraday lots while bleeding on multi-day futures (or vice-versa), and the existing summary couldn't show it. Shipped:
  - `src-tauri/src/paper/mod.rs` — new `PaperHoldTimeBucket` enum (`Intraday` ≤1h, `SameDay` 1-24h, `MultiDay` 1-7d, `Long` >7d, `Unknown` for unparseable) + `from_seconds()` classifier with boundaries exclusive at the upper end. New `PaperHoldTimeStats` struct mirroring `PaperCategoryStats` / `PaperSideStats` but adding `avg_hold_seconds` and `median_hold_seconds`. New `lot_hold_seconds()` parser (defensive: returns `None` for missing/unparseable/negative deltas → Unknown). New `compute_hold_time_stats(&[PaperLot])` helper that emits buckets in chronological order, with the same win/loss/PnL/staked aggregation semantics as the existing helpers (pushes contribute stake but not wins/losses, open lots counted but excluded from PnL). 12 new unit tests.
  - `src-ui/src/types/prizepicks.ts` — `PaperHoldTimeStats` interface; `PaperAnalytics` gains the `hold_time_stats` field.
  - `src-ui/src/components/PrizePicksPredictionsPanel.tsx` — new `HoldTimeBreakdown` inner component (6-column table: Hold time / Trades / Win % / PnL / ROI / Avg+Median hold) with `formatHoldSeconds()` helper. Mounted under the existing `SideBreakdown` so the three performance tables read as siblings.
  - `src-ui/src/index.css` — `.holdTimeBreakdown*` + `.holdTimeTable*` + `.holdTimeOpenTag` styles mirroring the side/category breakdowns.

## Brainstormed & shipped (2026-06-29 morning)

- **Per-window session PnL chips (today / 7d)** — The `PaperAnalytics` payload already exposed `session_pnl: SessionPnl` from the backend (`compute_session_pnl` walks `paper_equity_snapshots` DESC and finds the most-recent snapshot at-or-before the today-midnight and 7-days-ago cutoffs; 8 unit tests pass), but the TypeScript types were missing and the summary card didn't render the deltas — so a user looking at the panel had no way to see "how am I doing today/this week" without opening the equity-curve chart. Shipped:
  - `src-ui/src/types/prizepicks.ts` — new `SessionDelta { pnl_dollars, pnl_pct, baseline_equity, baseline_ts }` and `SessionPnl { today, this_week }` interfaces; `PaperAnalytics` gains `session_pnl: SessionPnl`. Both fields are nullable to match the backend (`null` when no qualifying baseline exists, e.g. brand-new account or first snapshot post-dates the cutoff).
  - `src-ui/src/components/PrizePicksPredictionsPanel.tsx` — new `SessionDeltaChip` inner component. Renders `+$X.XX (+Y.Y%)` with a pos/neg tint that mirrors `.streakChip`, and includes a baseline tooltip (`Baseline: $X.XX at <ts>\nΔ = $X.XX (Y.YY%)`) so the user can see exactly which snapshot the delta was computed from. `null` delta renders muted `—` so the summary layout doesn't shift for new accounts.
  - Two new cells added to the `paperSummary` row right after the existing "Streak" cell: **Today PnL** and **7d PnL**, each driven by `analytics.session_pnl.{today,this_week}`. Empty-state copy on the muted fallback guides the user to place a paper trade to seed the equity history.
  - `src-ui/src/index.css` — added `.sessionDeltaChip`, `.sessionDeltaChip.pos/.neg/.muted`, and `.sessionDeltaChip .sessionDeltaPct` (lighter, smaller percent text inside the chip). Same pill shape + 999px radius + pos/neg tints as `.streakChip` so the two read as siblings.
  - Ad-hoc verification (focused on the new behavior, NOT canonical suite green): 21/21 grep checks for TS interface fields, React import + component + summary cells, sign/muted branches, and all four CSS class variants.

## Brainstormed & shipped (2026-06-28)

- **Per-side paper performance breakdown** — The `PaperAnalytics` payload had `category_stats` (per stat category) but no equivalent for Over/Under performance. Most prop users have a strong opinion about whether they pick better Overs or Unders; without a per-side view they had to mentally aggregate from the prediction list. Shipped:
  - `src-tauri/src/paper/mod.rs` — new `PaperSideStats` struct + `compute_side_stats(&[PaperLot])` helper. Mirrors `compute_category_stats` but buckets by raw `side` (empty/whitespace → "Unknown" bucket). 8 new unit tests.
  - `src-ui/src/types/prizepicks.ts` — `PaperSideStats` interface + `paperSideLabel(side)` helper that maps raw "YES" → "Over" / "NO" → "Under".
  - `src-ui/src/components/PrizePicksPredictionsPanel.tsx` — new `SideBreakdown` inner component (five-column table: Side / Trades / Win % / PnL / ROI) mounted directly under the existing `CategoryBreakdown`. The two tables complement each other — per-category answers "where is the edge?" (Points, Rebounds, etc.) and per-side answers "am I better at picking Overs or Unders?".
  - `src-ui/src/index.css` — `.sideBreakdown*` + `.sideTable*` + `.sideOpenTag` styles, same visual weight as `.categoryBreakdown` so they read as siblings.

## Brainstormed & shipped (2026-06-27)

- **Paper equity curve chart** — The `paper_equity_snapshots` table and `get_equity_snapshots` query existed in the backend but were never wired to the UI; `PrizePicksPredictionsPanel` only showed a single equity number from `PaperAnalytics`. Shipped:
  - `src-tauri/src/commands/paper_cmd.rs` — new `paper_get_equity_history(limit?)` Tauri command (default 200).
  - `src-tauri/src/lib.rs` — registered in `invoke_handler`.
  - `src-ui/src/types/prizepicks.ts` — `PaperEquitySnapshot` interface.
  - `src-ui/src/services/prizepicks.ts` — `prizepicksApi.getPaperEquityHistory(limit?)`.
  - `src-ui/src/components/PrizePicksPredictionsPanel.tsx` — new `EquityCurve` inner component (pure SVG, no chart lib), 7d/30d/90d/All range toggle, delta/$ + delta% markers, min/max markers, max-drawdown cell added to the `paperSummary` card.
  - `src-ui/src/index.css` — `.equityChart*` + `.equityChartToolbar` styles (active range button state, header layout, SVG full-width).

## Dashboard performance (deferred)

**Phase 1 (shipped 2026-06-17):** flat `GET /markets` quick cache (replaces nested `/events` for dashboard load). See `prizepicks/client.rs` — `fetch_markets_flat_pages`, `ensure_quick_cache`.

### Phase 2 — Decouple cache reads from long fetches

- Extract `Arc<RwLock<PrizePicksCache>>` + `fetch_in_progress` guard so UI reads never block on 20-page full warm
- Background full-catalog warm writes cache without holding the outer `PrizePicksClient` mutex across HTTP pagination
- Optionally slim cache to `PrizePicksMarketSummary` instead of full `PrizePicksMarket`
- **Target:** warm revisit under 300ms; category switch under 500ms

### Phase 3 — Frontend critical-path trim

- ✅ **Show partial-cache indicator when `full_catalog == false`** (done 2026-06-27: `prizepicks_get_cache_status` command + 📦 badge in header)
- Keep `PrizePicksView` mounted across tab switches (avoid cold reload)
- ✅ **Combined IPC: `prizepicks_get_dashboard_bootstrap` → `{ markets, categories, cache_full }`** (done 2026-06-27: `prizepicks_get_dashboard_bootstrap` returns `{ props, scored_props, cache_status }`; `PrizePicksView` `useEffect` now fires a single IPC round-trip instead of three parallel invokes; granular commands remain for league/search/refresh)
- Defer `PrizePicksPredictionsPanel` load; debounce `computeStakeAdjustment` in market detail

### Phase 4 — Startup prefetch and persistence (optional)

- ✅ **Prefetch quick cache at app startup (before user opens dashboard)** (done 2026-06-28: `lib.rs` spawns `ensure_quick_cache` immediately on startup, no 8s delay)
- ✅ **Delay full warm until quick cache exists + idle window (or explicit Refresh only)** (done 2026-06-28: full warm still runs at 8s delay, but quick cache is ready from instant 0)
- Persist summary cache to SQLite for instant next-launch paint (deferred)

---

## Environment notes

- Canonical WSL repo (`~/.openclaw/agents/coderclaw/workspace/prizepicks-monster`) was unreachable as of 2026-06-17
- `edge-eval` and `monster-edge-core` live at `C:\\Users\\ethan\\prizepicks-build\\` (sibling paths)
