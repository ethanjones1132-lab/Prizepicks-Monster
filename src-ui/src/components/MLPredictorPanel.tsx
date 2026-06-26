import { useCallback, useEffect, useState } from 'react';
import { prizepicksApi } from '../services/prizepicks';
import type {
  MLModelStatus,
  MLPrediction,
  MLPredictionBatch,
  MLTrainingResult,
} from '../types/prizepicks';

function formatPct(value: number | null | undefined, digits = 1): string {
  if (value === null || value === undefined || !Number.isFinite(value)) return '—';
  return `${(value * 100).toFixed(digits)}%`;
}

function formatNumber(value: number | null | undefined, digits = 2): string {
  if (value === null || value === undefined || !Number.isFinite(value)) return '—';
  return value.toFixed(digits);
}

function leanLabel(winProb: number): { label: string; cls: string } {
  if (winProb >= 0.6) return { label: 'Lean Over', cls: 'leanOver' };
  if (winProb >= 0.5) return { label: 'Toss-up Over', cls: 'leanToss' };
  if (winProb >= 0.4) return { label: 'Toss-up Under', cls: 'leanToss' };
  return { label: 'Lean Under', cls: 'leanUnder' };
}

export function MLPredictorPanel() {
  const [status, setStatus] = useState<MLModelStatus | null>(null);
  const [predictions, setPredictions] = useState<MLPrediction[]>([]);
  const [loading, setLoading] = useState(true);
  const [training, setTraining] = useState(false);
  const [scoring, setScoring] = useState(false);
  const [exporting, setExporting] = useState(false);
  const [message, setMessage] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const [s, preds] = await Promise.all([
        prizepicksApi.mlGetModelStatus(),
        prizepicksApi.mlGetPredictions(20).catch(() => [] as MLPrediction[]),
      ]);
      setStatus(s);
      setPredictions(preds);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  const onTrain = async () => {
    setTraining(true);
    setMessage(null);
    setError(null);
    try {
      const result: MLTrainingResult = await prizepicksApi.mlTrainModel();
      if (result.status === 'insufficient_data') {
        setMessage(
          `${result.message} Need at least 10 resolved predictions (Win/Loss/Push).`,
        );
      } else if (result.status === 'trained') {
        setMessage(
          `Trained on ${result.samples} samples. CV accuracy ${formatPct(
            result.cv_accuracy_mean,
          )} ± ${formatPct(result.cv_accuracy_std)}. ${result.message}`,
        );
      } else {
        setMessage(result.message || `Train status: ${result.status}`);
      }
      await load();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setTraining(false);
    }
  };

  const onScore = async () => {
    setScoring(true);
    setMessage(null);
    setError(null);
    try {
      const batch: MLPredictionBatch = await prizepicksApi.mlPredictBatch();
      if (batch.status === 'no_model') {
        setMessage('No model on disk yet. Train first.');
      } else if (batch.status === 'no_pending') {
        setMessage('No pending props to score.');
      } else {
        setMessage(
          `Scored ${batch.predictions_count} pending props. ${batch.message}`,
        );
      }
      await load();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setScoring(false);
    }
  };

  const onExport = async () => {
    setExporting(true);
    setMessage(null);
    setError(null);
    try {
      const path = await prizepicksApi.mlExportFeatures();
      setMessage(`Exported feature CSV → ${path}`);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setExporting(false);
    }
  };

  const needsTraining =
    !status?.model_exists || (status?.resolved_predictions ?? 0) < 10;

  return (
    <section className="predictionsPanel">
      <div className="panelToolbar">
        <h4>ML prop predictor</h4>
        <button
          type="button"
          className="ghostBtn"
          onClick={() => void load()}
          disabled={loading}
        >
          Refresh
        </button>
        <button
          type="button"
          className="primaryBtn"
          onClick={() => void onTrain()}
          disabled={training || (status?.resolved_predictions ?? 0) < 10}
          title={
            (status?.resolved_predictions ?? 0) < 10
              ? `Need ≥10 resolved predictions (have ${status?.resolved_predictions ?? 0})`
              : 'Train (or retrain) the GradientBoosting model'
          }
        >
          {training ? 'Training…' : 'Train model'}
        </button>
        <button
          type="button"
          className="ghostBtn"
          onClick={() => void onScore()}
          disabled={scoring || !status?.model_exists}
          title={
            status?.model_exists
              ? 'Score all pending props with the current model'
              : 'Train a model first'
          }
        >
          {scoring ? 'Scoring…' : 'Score pending'}
        </button>
        <button
          type="button"
          className="ghostBtn"
          onClick={() => void onExport()}
          disabled={exporting}
        >
          {exporting ? 'Exporting…' : 'Export features CSV'}
        </button>
      </div>

      {message && <p className="info pad">{message}</p>}
      {error && <p className="error pad">{error}</p>}
      {loading && <p className="muted pad">Loading ML status…</p>}

      {!loading && status && (
        <>
          <div className="paperSummary">
            <div>
              <span className="muted">Model</span>
              <strong>{status.model_exists ? 'Trained' : 'Not trained'}</strong>
            </div>
            <div>
              <span className="muted">Samples</span>
              <strong>{status.samples ?? '—'}</strong>
            </div>
            <div>
              <span className="muted">CV accuracy</span>
              <strong>
                {formatPct(status.cv_accuracy_mean)} ±{' '}
                {formatPct(status.cv_accuracy_std)}
              </strong>
            </div>
            <div>
              <span className="muted">Win rate (train)</span>
              <strong>{formatPct(status.win_rate)}</strong>
            </div>
            <div>
              <span className="muted">Pending / Resolved</span>
              <strong>
                {status.pending_predictions} / {status.resolved_predictions}
              </strong>
            </div>
            <div>
              <span className="muted">Trained at</span>
              <strong>
                {status.trained_at
                  ? new Date(status.trained_at).toLocaleString()
                  : '—'}
              </strong>
            </div>
          </div>

          {needsTraining && (
            <p className="muted pad">
              {status.resolved_predictions < 10
                ? `Need at least 10 resolved predictions to train (currently ${status.resolved_predictions}). Keep paper-trading and grading props.`
                : 'Model file is missing. Click "Train model" to create it.'}
            </p>
          )}

          {status.feature_importance && status.feature_importance.length > 0 && (
            <div className="featureImportanceBlock">
              <h5 className="muted small">Top feature importances</h5>
              <table className="featureTable">
                <thead>
                  <tr>
                    <th>Feature</th>
                    <th style={{ textAlign: 'right' }}>Importance</th>
                  </tr>
                </thead>
                <tbody>
                  {status.feature_importance.slice(0, 10).map((f) => (
                    <tr key={f.feature}>
                      <td>
                        <code>{f.feature}</code>
                      </td>
                      <td style={{ textAlign: 'right' }}>
                        {f.importance.toFixed(4)}
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}

          <h5 className="sectionHeader">Latest ML predictions</h5>
          {predictions.length === 0 ? (
            <p className="muted pad">
              No ML predictions stored yet.{' '}
              {status.model_exists
                ? 'Click "Score pending" to generate them.'
                : 'Train a model first.'}
            </p>
          ) : (
            <table className="predictionTable">
              <thead>
                <tr>
                  <th>Player</th>
                  <th>Stat</th>
                  <th style={{ textAlign: 'right' }}>Line</th>
                  <th style={{ textAlign: 'right' }}>ML win%</th>
                  <th>Lean</th>
                  <th style={{ textAlign: 'right' }}>Conf</th>
                  <th style={{ textAlign: 'right' }}>Δ line</th>
                </tr>
              </thead>
              <tbody>
                {predictions.map((p) => {
                  const lean = leanLabel(p.ml_win_probability);
                  return (
                    <tr key={p.prediction_id}>
                      <td>{p.player_name}</td>
                      <td>
                        <span className="chip small">{p.stat_category || '—'}</span>
                      </td>
                      <td style={{ textAlign: 'right' }}>
                        {formatNumber(p.line, 1)}
                      </td>
                      <td style={{ textAlign: 'right' }}>
                        {(p.ml_win_probability * 100).toFixed(1)}%
                      </td>
                      <td>
                        <span className={`chip small ${lean.cls}`}>
                          {lean.label}
                        </span>
                      </td>
                      <td style={{ textAlign: 'right' }}>{p.original_confidence}</td>
                      <td style={{ textAlign: 'right' }}>
                        {p.line_change >= 0 ? '+' : ''}
                        {formatNumber(p.line_change, 2)}
                      </td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          )}

          <p className="muted small pad">
            Model file: <code>{status.model_path}</code>
          </p>
        </>
      )}
    </section>
  );
}
