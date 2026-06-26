import { invoke } from '@tauri-apps/api/core';
import type {
  KellyShrinkageReport,
  PrizePicksCategoryStat,
  PrizePicksMarketSummary,
  PrizePicksPrediction,
  PrizePicksPriceHistory,
  PrizePicksTradeDecision,
  PaperAnalytics,
  StakeAdjustment,
  MLCategoryModelList,
  MLCategoryTrainResult,
  MLModelStatus,
  MLPrediction,
  MLPredictionBatch,
  MLTrainingResult,
} from '../types/prizepicks';
import type { PropPick, ScoredProp } from '../types';

export interface PrizePicksGradingSummary {
  total_predictions: number;
  pending_gradable: number;
  graded: number;
  wins: number;
  losses: number;
  total_pnl: number;
  fetched_at: string;
}

export type PrizePicksBetSide = 'OVER' | 'UNDER' | 'PASS' | 'UNKNOWN';

/** Parse PrizePicks API contract_side/pick_type into user-facing Over/Under terminology */
export function parsePrizePicksBetSide(
  contractSide?: string | null,
  pickType?: string | null,
): PrizePicksBetSide {
  const side = (contractSide ?? '').trim().toUpperCase();
  if (side === 'YES') return 'OVER';
  if (side === 'NO') return 'UNDER';
  if (side === 'PASS') return 'PASS';
  const pick = (pickType ?? '').trim().toLowerCase();
  if (pick === 'over') return 'OVER';
  if (pick === 'under') return 'UNDER';
  return 'UNKNOWN';
}

export function prizepicksBetWon(pred: PrizePicksPrediction): boolean | null {
  const actual = pred.actual_outcome;
  if (!actual) return null;
  const side = parsePrizePicksBetSide(pred.contract_side, pred.pick_type);
  if (side === 'OVER') return actual === 'Yes';
  if (side === 'UNDER') return actual === 'No';
  return null;
}

export const prizepicksApi = {
  // ── Prop data ──

  getProps: (league: string) =>
    invoke<PropPick[]>('prizepicks_get_props', { league }),

  getTopProps: (limit?: number) =>
    invoke<PropPick[]>('prizepicks_get_top_props', { limit: limit ?? 50 }),

  searchProps: (query: string) =>
    invoke<PropPick[]>('prizepicks_search_props', { query }),

  getScoredProps: () =>
    invoke<ScoredProp[]>('prizepicks_get_scored_props'),

  // ── PrizePicks feed / odds comparison ──

  getMarkets: (category: string) =>
    invoke<PrizePicksMarketSummary[]>('prizepicks_get_markets', { category }),

  getTopMarkets: (limit?: number) =>
    invoke<PrizePicksMarketSummary[]>('prizepicks_get_top_markets', { limit: limit ?? 50 }),

  searchMarkets: (query: string) =>
    invoke<PrizePicksMarketSummary[]>('prizepicks_search_markets', { query }),

  getMarket: (ticker: string) =>
    invoke<PrizePicksMarketSummary>('prizepicks_get_market', { ticker }),

  getCategoryStats: () =>
    invoke<PrizePicksCategoryStat[]>('prizepicks_get_category_stats'),

  refresh: () => invoke<number>('prizepicks_refresh'),

  // ── Predictions ──

  getPredictions: () => invoke<PrizePicksPrediction[]>('prizepicks_get_predictions'),

  gradePending: () => invoke<PrizePicksGradingSummary>('prizepicks_grade_pending_predictions'),

  computeStakeAdjustment: (args: {
    ticker: string;
    category: string;
    contractSide: string;
    recommendedStake: number;
  }) =>
    invoke<StakeAdjustment>('prizepicks_compute_stake_adjustment', {
      ticker: args.ticker,
      category: args.category,
      contractSide: args.contractSide,
      recommendedStake: args.recommendedStake,
    }),

  getPriceHistory: (ticker: string, limit?: number) =>
    invoke<PrizePicksPriceHistory>('prizepicks_get_price_history', { ticker, limit: limit ?? 200 }),

  // Walk resolved predictions and capture closing-line value (CLV) for any
  // that don't yet have one. Idempotent; safe to call from a tab-focus handler.
  captureClv: () => invoke<number>('prizepicks_capture_clv'),

  getKellyShrinkageReport: () =>
    invoke<KellyShrinkageReport>('prizepicks_kelly_shrinkage_report'),

  recordPaperDecision: (sessionId: string, decision: PrizePicksTradeDecision) =>
    invoke<string>('prizepicks_record_paper_decision', { sessionId, decision }),

  getPaperAnalytics: () => invoke<PaperAnalytics>('paper_get_analytics'),

  settlePaperPositions: () =>
    invoke<{ settled: number; wins: number; losses: number; total_pnl: number }>(
      'paper_settle_pending',
    ),

  // ── ML Predictor ──

  mlTrainModel: (outputPath?: string) =>
    invoke<MLTrainingResult>('ml_train_model', { outputPath }),

  mlPredictBatch: () => invoke<MLPredictionBatch>('ml_predict_batch'),

  mlGetModelStatus: () => invoke<MLModelStatus>('ml_get_model_status'),

  mlGetPredictions: (limit?: number) =>
    invoke<MLPrediction[]>('ml_get_predictions', { limit: limit ?? 50 }),

  mlExportFeatures: (outputPath?: string) =>
    invoke<string>('ml_export_features', { outputPath }),

  // ── Per-category ML models ──

  mlTrainPerCategory: (outputDir?: string, minSamples?: number) =>
    invoke<MLCategoryTrainResult>('ml_train_per_category', {
      outputDir,
      minSamples: minSamples ?? 10,
    }),

  mlPredictBatchPerCategory: () =>
    invoke<MLPredictionBatch>('ml_predict_batch_per_category'),

  mlGetCategoryModels: () =>
    invoke<MLCategoryModelList>('ml_get_category_models'),
};
