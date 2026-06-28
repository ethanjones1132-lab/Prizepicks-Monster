import type { PropPick, ScoredProp } from './index';

export interface PrizePicksMarketSummary {
  ticker: string;
  event_ticker: string;
  title: string;
  category: string;
  status: string;
  yes_prob_pct: number;
  yes_ask: number;
  yes_bid: number;
  no_ask: number;
  no_bid: number;
  last_price: number;
  volume_24h: number;
  total_volume: number;
  liquidity: number;
  spread: number;
  close_time?: string | null;
  expiration_time?: string | null;
  result: string;
  can_close_early: boolean;
  is_provisional: boolean;
}

export interface PrizePicksCategoryStat {
  category: string;
  count: number;
  volume_24h: number;
}

export interface PrizePicksPrediction {
  id: string;
  ticker: string;
  title: string;
  category: string;
  predicted_probability: number;
  actual_outcome?: string | null;
  confidence_score?: number | null;
  reasoning?: string | null;
  created_at: string;
  resolved_at?: string | null;
  stake_amount: number;
  pnl?: number | null;
  pick_type?: string | null;
  price_to_enter?: number | null;
  market_price_at_entry?: number | null;
  contract_side?: string | null;
  edge_points?: number | null;
  fractional_kelly_pct?: number | null;
  recommended_stake_dollars?: number | null;
  risk_flags?: string[] | null;
  thesis?: string | null;
  data_quality?: string | null;
  decision?: string | null;
}

export interface CorrelationConflict {
  exposure_ticker: string;
  exposure_title: string;
  strength: string;
  kelly_multiplier: number;
  explanation: string;
}

export interface KellyShrinkageReport {
  multiplier: number;
  n: number;
  brier: number | null;
  base_rate: number | null;
  climatology_brier: number | null;
  brier_skill_score: number | null;
  sample_factor: number;
  calibration_factor: number;
  reason: string;
}

export interface StakeAdjustment {
  kelly_scale: number;
  raw_recommended_stake: number;
  adjusted_recommended_stake: number;
  conflicts: CorrelationConflict[];
  warnings: string[];
  kelly_shrinkage?: KellyShrinkageReport | null;
}

export interface PrizePicksPriceSnapshot {
  id: string;
  ticker: string;
  title: string;
  category: string;
  yes_prob_pct: number;
  yes_bid: number;
  yes_ask: number;
  spread: number;
  volume_24h: number;
  liquidity: number;
  snapshot_at: string;
}

export interface PrizePicksPriceHistory {
  ticker: string;
  snapshots: PrizePicksPriceSnapshot[];
  opening_yes_prob?: number | null;
  current_yes_prob?: number | null;
  prob_change?: number | null;
  spread_change?: number | null;
}

export interface PrizePicksCacheStatus {
  has_cache: boolean;
  full_catalog: boolean;
  markets_count: number;
  fetched_at: number;
  is_stale: boolean;
}

/**
 * Combined payload returned by `prizepicksApi.getDashboardBootstrap()`.
 * Replaces the previous fan-out of `getTopProps` + `getScoredProps` +
 * `getCacheStatus` on dashboard mount with a single IPC round-trip.
 * Field names match the Rust struct (`PrizePicksDashboardBootstrap`)
 * — the keys come through as snake_case via serde defaults.
 */
export interface PrizePicksDashboardBootstrap {
  props: PropPick[];
  scored_props: ScoredProp[];
  cache_status: PrizePicksCacheStatus;
}

export interface PrizePicksTradeDecision {
  ticker: string;
  market_title: string;
  category: string;
  /** Mirrors backend ContractSide enum (YES/NO/PASS) for IPC compatibility */
  contract_side: 'YES' | 'NO' | 'PASS';
  market_price_pct: number;
  fair_probability_pct: number;
  edge_points: number;
  spread_cents: number;
  liquidity_score: number;
  ev_per_contract_cents: number;
  ev_roi_pct: number;
  raw_kelly_pct: number;
  fractional_kelly_pct: number;
  recommended_stake_dollars: number;
  max_position_dollars: number;
  decision: 'TAKE' | 'WATCH' | 'PASS';
  confidence_tier: 'High' | 'Medium' | 'Low' | 'None';
  thesis: string;
  evidence: string[];
  risk_flags: string[];
  data_quality: string;
  price_to_enter: number;
  model_disagreement: boolean;
  disagreement_points: number;
}

export interface PaperStreak {
  /** "W" for a win streak, "L" for a loss streak, "None" when no closed lots yet. */
  kind: 'W' | 'L' | 'None' | string;
  length: number;
}

/**
 * Per-category performance breakdown for a single PrizePicks stat category
 * (e.g. Points, Rebounds, Goals). Returned as part of `PaperAnalytics`.
 * Sorted by `realized_pnl` DESC so the strongest categories surface first.
 */
export interface PaperCategoryStats {
  category: string;
  total_trades: number;
  open_trades: number;
  wins: number;
  losses: number;
  win_rate: number;
  realized_pnl: number;
  total_staked: number;
  roi_pct: number;
}

export interface PaperAnalytics {
  starting_balance: number;
  cash_balance: number;
  open_market_value: number;
  equity: number;
  realized_pnl: number;
  unrealized_pnl: number;
  total_return_pct: number;
  total_trades: number;
  open_positions: number;
  win_rate: number;
  wins: number;
  losses: number;
  profit_factor: number;
  avg_winner: number;
  avg_loser: number;
  largest_winner: number;
  largest_loser: number;
  max_drawdown_pct: number;
  current_streak: PaperStreak;
  category_stats: PaperCategoryStats[];
  fetched_at: string;
}

/** Historical equity snapshot for the paper-trading account. */
export interface PaperEquitySnapshot {
  id: number;
  ts: string;
  balance_dollars: number;
  open_market_value: number;
  equity_dollars: number;
  unrealized_pnl: number;
}

// ── ML Predictor types (mirrors src-tauri/src/ml_predictor.rs) ──

export interface MLFeatureImportance {
  feature: string;
  importance: number;
}

export interface MLTrainingResult {
  status: string;
  samples: number | null;
  cv_accuracy_mean: number | null;
  cv_accuracy_std: number | null;
  win_rate: number | null;
  model_path: string | null;
  feature_importance: MLFeatureImportance[] | null;
  message: string;
}

export interface MLPrediction {
  prediction_id: string;
  player_name: string;
  stat_category: string;
  line: number;
  ml_win_probability: number;
  ml_prediction: 'Win' | 'Loss' | string;
  original_confidence: number;
  original_probability: number | null;
  line_change: number;
}

export interface MLPredictionBatch {
  status: string;
  model_path: string | null;
  predictions_count: number;
  predictions: MLPrediction[];
  message: string;
}

export interface MLModelStatus {
  model_exists: boolean;
  model_path: string;
  trained_at: string | null;
  samples: number | null;
  cv_accuracy_mean: number | null;
  cv_accuracy_std: number | null;
  win_rate: number | null;
  feature_importance: MLFeatureImportance[] | null;
  pending_predictions: number;
  resolved_predictions: number;
  message: string;
}

// ── Per-category ML models (mirrors src-tauri/src/ml_predictor.rs) ──

export interface MLCategoryModelResult {
  category: string;
  token: string;
  status: string;
  samples: number;
  win_rate: number;
  model_path: string | null;
  cv_accuracy_mean: number | null;
  cv_accuracy_std: number | null;
  feature_importance: MLFeatureImportance[];
  message: string;
}

export interface MLCategoryTrainResult {
  status: string;
  message: string;
  output_dir: string;
  trained_count: number;
  skipped_count: number;
  min_samples: number;
  categories: MLCategoryModelResult[];
}

export interface MLCategoryModelInfo {
  category: string;
  token: string;
  model_path: string;
  meta_path: string;
  trained_at: string | null;
  samples: number | null;
  cv_accuracy_mean: number | null;
  cv_accuracy_std: number | null;
  win_rate: number | null;
  feature_importance: MLFeatureImportance[];
}

export interface MLCategoryModelList {
  status: string;
  model_dir: string;
  message: string;
  models: MLCategoryModelInfo[];
}

