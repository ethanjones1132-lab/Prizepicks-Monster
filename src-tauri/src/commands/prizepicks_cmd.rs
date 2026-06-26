use super::PrizePicksState;
use sqlx::{Pool, Sqlite};
use std::sync::Arc;
use tauri::State;
use tokio::sync::Mutex;

#[tauri::command]
pub async fn prizepicks_get_markets(
    category: String,
    prizepicks: State<'_, PrizePicksState>,
) -> Result<Vec<crate::prizepicks::PrizePicksMarketSummary>, String> {
    let mut client = prizepicks.lock().await;
    client.get_markets_by_category(&category).await
}

#[tauri::command]
pub async fn prizepicks_get_market(
    ticker: String,
    prizepicks: State<'_, PrizePicksState>,
) -> Result<crate::prizepicks::PrizePicksMarketSummary, String> {
    let client = prizepicks.lock().await;
    let market = client.fetch_market(&ticker).await?;
    Ok(crate::prizepicks::PrizePicksMarketSummary::from(&market))
}

#[tauri::command]
pub async fn prizepicks_get_orderbook(
    ticker: String,
    prizepicks: State<'_, PrizePicksState>,
) -> Result<crate::prizepicks::PrizePicksOrderbook, String> {
    let client = prizepicks.lock().await;
    client.fetch_orderbook(&ticker).await
}

#[tauri::command]
pub async fn prizepicks_search_markets(
    query: String,
    prizepicks: State<'_, PrizePicksState>,
) -> Result<Vec<crate::prizepicks::PrizePicksMarketSummary>, String> {
    if query.len() > 200 {
        return Err("Search query too long (max 200 characters)".to_string());
    }
    let mut client = prizepicks.lock().await;
    client.search_markets(&query).await
}

#[tauri::command]
pub async fn prizepicks_get_top_markets(
    limit: Option<usize>,
    prizepicks: State<'_, PrizePicksState>,
) -> Result<Vec<crate::prizepicks::PrizePicksMarketSummary>, String> {
    let n = limit.unwrap_or(30).min(100);
    let mut client = prizepicks.lock().await;
    client.get_top_markets(n).await
}

#[tauri::command]
pub async fn prizepicks_get_category_stats(
    prizepicks: State<'_, PrizePicksState>,
) -> Result<Vec<crate::prizepicks::PrizePicksCategoryStat>, String> {
    let client = prizepicks.lock().await;
    Ok(client.category_stats())
}

#[tauri::command]
pub async fn prizepicks_get_portfolio(
    config: State<'_, Arc<Mutex<crate::config::AppConfig>>>,
    prizepicks: State<'_, PrizePicksState>,
) -> Result<serde_json::Value, String> {
    {
        let app_cfg = config.lock().await;
        let mut client = prizepicks.lock().await;
        if client.config.email != app_cfg.prizepicks_email
            || client.config.password != app_cfg.prizepicks_password
        {
            let new_cfg = crate::prizepicks::prizepicks_config_from_app(&app_cfg);
            client.config = new_cfg;
            client.invalidate_cache();
        }
    }

    let mut client = prizepicks.lock().await;
    let balance = client.get_balance().await?;
    let positions = client.get_positions().await?;

    Ok(serde_json::json!({
        "balance_cents": balance.balance,
        "balance_dollars": balance.balance as f64 / 100.0,
        "reserved_fees_cents": balance.reserved_fees,
        "positions": positions,
    }))
}

#[tauri::command]
pub async fn prizepicks_refresh(
    config: State<'_, Arc<Mutex<crate::config::AppConfig>>>,
    prizepicks: State<'_, PrizePicksState>,
    db_pool: State<'_, Pool<Sqlite>>,
) -> Result<usize, String> {
    {
        let app_cfg = config.lock().await;
        let mut client = prizepicks.lock().await;
        let new_cfg = crate::prizepicks::prizepicks_config_from_app(&app_cfg);
        client.config = new_cfg;
        client.invalidate_cache();
    }
    let mut client = prizepicks.lock().await;
    let markets = client.fetch_all_markets().await?;
    let summaries: Vec<crate::prizepicks::PrizePicksMarketSummary> = markets
        .iter()
        .map(crate::prizepicks::PrizePicksMarketSummary::from)
        .collect();
    if let Err(e) = crate::prizepicks::price_tracker::snapshot_markets(&db_pool, &summaries).await {
        tracing::warn!("prizepicks price snapshot on refresh: {}", e);
    }
    Ok(markets.len())
}

#[tauri::command]
pub async fn prizepicks_get_predictions(
    tracker: State<'_, Arc<Mutex<crate::predictions::tracker::PredictionTracker>>>,
) -> Result<Vec<crate::prizepicks::models::PrizePicksPrediction>, String> {
    let t = tracker.lock().await;
    Ok(t.get_prizepicks_predictions().await)
}

#[tauri::command]
pub async fn prizepicks_get_prediction_stats(
    tracker: State<'_, Arc<Mutex<crate::predictions::tracker::PredictionTracker>>>,
) -> Result<crate::prizepicks::models::PrizePicksPredictionStats, String> {
    let t = tracker.lock().await;
    let all = t.get_prizepicks_predictions().await;
    Ok(t.get_prizepicks_stats(&all).await)
}

#[tauri::command]
pub async fn prizepicks_grade_pending_predictions(
    tracker: State<'_, Arc<Mutex<crate::predictions::tracker::PredictionTracker>>>,
    prizepicks: State<'_, PrizePicksState>,
) -> Result<crate::prizepicks::models::PrizePicksGradingSummary, String> {
    let t = tracker.lock().await;
    let client = prizepicks.lock().await;
    crate::prizepicks::grading::grade_pending_predictions(&t, &*client).await
}

#[tauri::command]
pub async fn prizepicks_get_grading_summary(
    tracker: State<'_, Arc<Mutex<crate::predictions::tracker::PredictionTracker>>>,
) -> Result<crate::prizepicks::models::PrizePicksGradingSummary, String> {
    let t = tracker.lock().await;
    Ok(t.get_prizepicks_grading_summary().await)
}

#[tauri::command]
pub async fn export_prizepicks_predictions_csv(
    tracker: State<'_, Arc<Mutex<crate::predictions::tracker::PredictionTracker>>>,
) -> Result<String, String> {
    use super::csv_export;

    let t = tracker.lock().await;
    let mut all = t.get_prizepicks_predictions().await;
    all.sort_by(|a, b| b.created_at.cmp(&a.created_at));

    csv_export(
        &["date", "ticker", "title", "category", "predicted_probability", "actual_outcome", "confidence_score", "stake_amount", "pnl"],
        |wtr| {
            for pred in &all {
                wtr.write_record(&[
                    pred.created_at.clone(),
                    pred.ticker.clone(),
                    pred.title.clone(),
                    pred.category.clone(),
                    pred.predicted_probability.to_string(),
                    pred.actual_outcome.clone().unwrap_or_default(),
                    pred.confidence_score.map(|s| s.to_string()).unwrap_or_default(),
                    pred.stake_amount.to_string(),
                    pred.pnl.map(|p| p.to_string()).unwrap_or_default(),
                ])
                .map_err(|e| crate::error::AppError::Io(format!("CSV row error: {e}")))?;
            }
            Ok(())
        },
    ).map_err(Into::into)
}

#[tauri::command]
pub async fn prizepicks_compute_stake_adjustment(
    ticker: String,
    category: String,
    contract_side: String,
    recommended_stake: f64,
    tracker: State<'_, Arc<Mutex<crate::predictions::tracker::PredictionTracker>>>,
    prizepicks: State<'_, PrizePicksState>,
    db_pool: State<'_, Pool<Sqlite>>,
) -> Result<crate::prizepicks::StakeAdjustment, String> {
    let pending = {
        let t = tracker.lock().await;
        t.get_prizepicks_predictions().await
    };
    let mut exposures = crate::prizepicks::portfolio_risk::exposures_from_predictions(
        &pending
            .iter()
            .filter(|p| p.actual_outcome.is_none())
            .cloned()
            .collect::<Vec<_>>(),
    );

    if let Ok(positions) = prizepicks.lock().await.get_positions().await {
        exposures.extend(crate::prizepicks::portfolio_risk::exposures_from_positions(&positions));
    }

    // Fold the live Brier-driven Kelly shrinkage into the adjustment so the
    // returned `kelly_scale` already reflects historical calibration.
    // `fetch_resolved_for_brier` is shared with the paper-decision path.
    let shrinkage = match fetch_resolved_for_brier(&db_pool).await {
        Ok(resolved) => Some(crate::analysis::kelly_shrinkage::compute_shrinkage(&resolved)),
        Err(_) => None,
    };

    Ok(crate::prizepicks::portfolio_risk::compute_stake_adjustment_with_shrinkage(
        &ticker,
        &category,
        Some(&contract_side),
        recommended_stake,
        &exposures,
        shrinkage,
    ))
}

#[tauri::command]
pub async fn prizepicks_snapshot_prices(
    prizepicks: State<'_, PrizePicksState>,
    db_pool: State<'_, Pool<Sqlite>>,
) -> Result<crate::prizepicks::PrizePicksSnapshotBatch, String> {
    let mut client = prizepicks.lock().await;
    let markets = client.fetch_all_markets().await?;
    let summaries: Vec<crate::prizepicks::PrizePicksMarketSummary> = markets
        .iter()
        .map(crate::prizepicks::PrizePicksMarketSummary::from)
        .collect();
    crate::prizepicks::price_tracker::snapshot_markets(&db_pool, &summaries).await
}

#[tauri::command]
pub async fn prizepicks_get_price_history(
    ticker: String,
    limit: Option<i64>,
    db_pool: State<'_, Pool<Sqlite>>,
) -> Result<crate::prizepicks::PrizePicksPriceHistory, String> {
    crate::prizepicks::price_tracker::get_price_history(&db_pool, &ticker, limit.unwrap_or(200))
        .await
}

/// Sweep resolved predictions and capture closing-line value (CLV) for any that
/// don't yet have a closing price tagged. Returns the number of predictions
/// updated. Safe to invoke from the UI (e.g. on tab focus) — idempotent.
#[tauri::command]
pub async fn prizepicks_capture_clv(
    db_pool: State<'_, Pool<Sqlite>>,
) -> Result<usize, String> {
    crate::predictions::storage::capture_closing_prices_for_resolved(&db_pool).await
}

/// Compute the live volatility-adjusted Kelly shrinkage report from resolved
/// predictions. Returns a `KellyShrinkageReport` with `multiplier`, Brier
/// score, Brier Skill Score, sample factor, calibration factor, and a short
/// human-readable reason. Cold-start (no resolved predictions) returns
/// multiplier = 1.0 and `brier = None`.
#[tauri::command]
pub async fn prizepicks_kelly_shrinkage_report(
    db_pool: State<'_, Pool<Sqlite>>,
) -> Result<crate::analysis::kelly_shrinkage::KellyShrinkageReport, String> {
    let resolved = fetch_resolved_for_brier(&db_pool).await?;
    Ok(crate::analysis::kelly_shrinkage::compute_shrinkage(&resolved))
}

/// Shared helper: pull resolved predictions and project them into the
/// minimal shape that `kelly_shrinkage` needs.
async fn fetch_resolved_for_brier(
    db_pool: &Pool<Sqlite>,
) -> Result<Vec<crate::analysis::kelly_shrinkage::ResolvedForBrier>, String> {
    use sqlx::Row;
    // The production schema stores the predicted probability in the
    // `probability` column and the realized outcome ("Win" / "Loss" /
    // "Push") in the `outcome` column (see `predictions::storage` schema).
    // Previously this helper queried the struct field names
    // (`predicted_probability` / `actual_outcome`), which don't exist in
    // the DB, so the helper always returned an empty Vec and the live
    // shrinkage report silently fell back to the cold-start multiplier.
    let rows = sqlx::query(
        "SELECT probability, outcome \
         FROM predictions \
         WHERE outcome IN ('Win', 'Loss', 'Push')",
    )
    .fetch_all(db_pool)
    .await
    .map_err(|e| format!("Failed to fetch resolved predictions: {}", e))?;
    let mut out = Vec::with_capacity(rows.len());
    for r in rows {
        let prob: Option<f64> = r.try_get("probability").ok();
        let outcome: Option<String> = r.try_get("outcome").ok();
        let (prob, outcome) = match (prob, outcome) {
            (Some(p), Some(o)) => (p, o),
            _ => continue,
        };
        if let Some(hit) = crate::analysis::kelly_shrinkage::parse_hit_outcome(&outcome) {
            out.push(crate::analysis::kelly_shrinkage::ResolvedForBrier {
                predicted_probability_pct: prob,
                hit,
            });
        }
    }
    Ok(out)
}

#[tauri::command]
pub async fn prizepicks_record_paper_decision(
    session_id: String,
    mut decision: crate::chat::decision_schema::PrizePicksTradeDecision,
    tracker: State<'_, Arc<Mutex<crate::predictions::tracker::PredictionTracker>>>,
    prizepicks: State<'_, PrizePicksState>,
    db_pool: State<'_, Pool<Sqlite>>,
) -> Result<String, String> {
    let bankroll = crate::bankroll::load_bankroll_config();
    let pending = {
        let t = tracker.lock().await;
        t.get_prizepicks_predictions().await
    };
    let mut exposures = crate::prizepicks::portfolio_risk::exposures_from_predictions(
        &pending
            .iter()
            .filter(|p| p.actual_outcome.is_none())
            .cloned()
            .collect::<Vec<_>>(),
    );
    if let Ok(positions) = prizepicks.lock().await.get_positions().await {
        exposures.extend(crate::prizepicks::portfolio_risk::exposures_from_positions(&positions));
    }

    // Fold the live Brier-driven Kelly shrinkage into the adjustment so the
    // recorded paper decision and any UI display reflect historical
    // calibration. `fetch_resolved_for_brier` is shared with the
    // `prizepicks_kelly_shrinkage_report` command and the
    // `prizepicks_compute_stake_adjustment` path.
    let shrinkage = match fetch_resolved_for_brier(&db_pool).await {
        Ok(resolved) => Some(crate::analysis::kelly_shrinkage::compute_shrinkage(&resolved)),
        Err(_) => None,
    };

    let side = format!("{:?}", decision.contract_side);
    let raw_stake = if decision.recommended_stake_dollars > 0.0 {
        decision.recommended_stake_dollars
    } else {
        bankroll.total_bankroll * (decision.fractional_kelly_pct / 100.0)
    };
    let adj = crate::prizepicks::portfolio_risk::compute_stake_adjustment_with_shrinkage(
        &decision.ticker,
        &decision.category,
        Some(&side),
        raw_stake,
        &exposures,
        shrinkage,
    );
    decision.compute_risk_adjusted(
        bankroll.total_bankroll,
        bankroll.kelly_fraction,
        adj.kelly_scale,
        bankroll.max_bet_pct,
        true,
    );

    let prediction_id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();
    let decision_json =
        serde_json::to_string(&decision).map_err(|e| format!("serialize decision: {}", e))?;
    let pick_type = match decision.contract_side {
        crate::chat::decision_schema::ContractSide::YES => Some("Over".to_string()),
        crate::chat::decision_schema::ContractSide::NO => Some("Under".to_string()),
        crate::chat::decision_schema::ContractSide::PASS => None,
    };

    let prediction = crate::predictions::tracker::Prediction {
        id: prediction_id.clone(),
        session_id,
        raw_text: decision_json.clone(),
        player_name: Some(decision.ticker.clone()),
        pick_type,
        line: Some(decision.recommended_stake_dollars),
        stat_category: Some(decision.category.clone()),
        confidence: Some(format!("{:?}", decision.confidence_tier)),
        confidence_score: None,
        probability: Some(decision.fair_probability_pct),
        reasoning: if decision.thesis.is_empty() {
            None
        } else {
            Some(decision.thesis.clone())
        },
        risk: if adj.warnings.is_empty() {
            None
        } else {
            Some(adj.warnings.join("; "))
        },
        created_at: now,
        full_decision_json: Some(decision_json.clone()),
    };

    let record = crate::predictions::tracker::PredictionRecord {
        prediction,
        outcome: crate::predictions::tracker::PredictionOutcome::Pending,
        actual_result: None,
        notes: Some(format!(
            "Paper trade: {:?} {} @ {:.2} (kelly_scale {:.0}%)",
            decision.contract_side,
            decision.ticker,
            decision.price_to_enter,
            adj.kelly_scale * 100.0
        )),
        resolved_at: None,
    };

    let t = tracker.lock().await;
    t.save_prediction(record).await?;

    if decision.contract_side != crate::chat::decision_schema::ContractSide::PASS {
        let entry_cents = crate::paper::normalize_entry_cents(decision.price_to_enter);
        let stake = decision.recommended_stake_dollars.max(0.0);
        if stake > 0.0 && entry_cents > 0.0 && entry_cents < 100.0 {
            let qty = stake / (entry_cents / 100.0);
            let side = format!("{:?}", decision.contract_side);
            let trade_input = crate::paper::PaperTradeInput {
                ticker: decision.ticker.clone(),
                title: decision.market_title.clone(),
                category: decision.category.clone(),
                side,
                qty,
                entry_price_cents: entry_cents,
                source: crate::paper::PaperTradeSource::Manual,
                decision_json: Some(decision_json),
            };
            match crate::paper::place_trade(&db_pool, trade_input).await {
                Ok(lot) => {
                    tracing::info!(
                        "paper lot opened: {} {:?} qty {:.2} @ {:.1}c",
                        lot.ticker,
                        decision.contract_side,
                        lot.qty,
                        lot.entry_price_cents
                    );
                }
                Err(e) => {
                    tracing::warn!(
                        "paper lot not opened for {} (prediction {} saved): {}",
                        decision.ticker,
                        prediction_id,
                        e
                    );
                }
            }
        }
    }

    Ok(prediction_id)
}

#[tauri::command]
pub async fn prizepicks_get_props(
    league: String,
    prizepicks_fetcher: State<'_, Arc<Mutex<crate::prizepicks::PrizePicksFetcher>>>,
) -> Result<Vec<crate::prizepicks::PrizePicksProp>, String> {
    let mut fetcher = prizepicks_fetcher.lock().await;
    let league_opt = if league == "All" { None } else { Some(league.as_str()) };
    let response = fetcher.fetch_props(league_opt, false).await?;
    Ok(response.props)
}

#[tauri::command]
pub async fn prizepicks_get_top_props(
    limit: Option<usize>,
    prizepicks_fetcher: State<'_, Arc<Mutex<crate::prizepicks::PrizePicksFetcher>>>,
) -> Result<Vec<crate::prizepicks::PrizePicksProp>, String> {
    let mut fetcher = prizepicks_fetcher.lock().await;
    let response = fetcher.fetch_props(None, false).await?;
    let limit = limit.unwrap_or(50);
    Ok(response.props.into_iter().take(limit).collect())
}

#[tauri::command]
pub async fn prizepicks_search_props(
    query: String,
    prizepicks_fetcher: State<'_, Arc<Mutex<crate::prizepicks::PrizePicksFetcher>>>,
) -> Result<Vec<crate::prizepicks::PrizePicksProp>, String> {
    let mut fetcher = prizepicks_fetcher.lock().await;
    let response = fetcher.search_props(&query).await?;
    Ok(response.props)
}

#[tauri::command]
pub async fn prizepicks_get_scored_props(
    prizepicks_fetcher: State<'_, Arc<Mutex<crate::prizepicks::PrizePicksFetcher>>>,
) -> Result<Vec<serde_json::Value>, String> {
    let fetcher = prizepicks_fetcher.lock().await;
    fetcher.get_scored_props().await
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::SqlitePoolOptions;

    /// Build a minimal in-memory pool with just the schema columns
    /// `fetch_resolved_for_brier` reads. Mirrors the production schema in
    /// `predictions::storage`.
    async fn fresh_pool() -> Pool<Sqlite> {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::query(
            r#"
            CREATE TABLE predictions (
                id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL DEFAULT '',
                raw_text TEXT NOT NULL DEFAULT '',
                player_name TEXT,
                pick_type TEXT,
                line REAL,
                stat_category TEXT,
                confidence TEXT,
                confidence_score INTEGER,
                probability REAL,
                reasoning TEXT,
                risk TEXT,
                created_at TEXT NOT NULL DEFAULT '',
                outcome TEXT NOT NULL DEFAULT 'Pending',
                actual_result REAL,
                notes TEXT,
                resolved_at TEXT
            )
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();
        pool
    }

    /// Regression test: fetch_resolved_for_brier must read the production
    /// schema columns (`probability` and `outcome`), not the in-memory struct
    /// field names. The previous version queried `predicted_probability` /
    /// `actual_outcome`, which don't exist in the DB, so the helper always
    /// returned an empty Vec and the live shrinkage report silently fell
    /// back to the cold-start multiplier.
    #[tokio::test]
    async fn fetch_resolved_reads_production_schema_columns() {
        let pool = fresh_pool().await;

        // Insert a mix of resolved (Win / Loss / Push) and Pending rows.
        // The probability is the model's predicted win probability in
        // percent — the same shape the production code writes to the
        // `probability` column at insert time.
        let rows = [
            ("win-1", "Win", 60.0_f64),
            ("win-2", "Win", 70.0),
            ("loss-1", "Loss", 55.0),
            ("push-1", "Push", 50.0),
            ("pending-1", "Pending", 80.0),
        ];
        for (id, outcome, prob) in rows {
            sqlx::query(
                "INSERT INTO predictions (id, outcome, probability) VALUES (?1, ?2, ?3)",
            )
            .bind(id)
            .bind(outcome)
            .bind(prob)
            .execute(&pool)
            .await
            .unwrap();
        }

        let resolved = fetch_resolved_for_brier(&pool).await.unwrap();
        // 4 resolved rows (Win / Win / Loss / Push); the Pending row is
        // filtered out by the WHERE clause.
        assert_eq!(resolved.len(), 4);

        // All hit flags should be parsed: Win → true, Loss → false,
        // Push → false (see parse_hit_outcome test cases).
        let hits: Vec<bool> = resolved.iter().map(|r| r.hit).collect();
        assert_eq!(hits.iter().filter(|h| **h).count(), 2);
        assert_eq!(hits.iter().filter(|h| !**h).count(), 2);

        // Probability is read as-is from the `probability` column.
        let probs: Vec<f64> = resolved
            .iter()
            .map(|r| r.predicted_probability_pct)
            .collect();
        for p in &probs {
            assert!((50.0..=70.0).contains(p), "unexpected prob: {p}");
        }
    }

    /// An empty pool (no resolved rows) should return an empty Vec — the
    /// downstream `compute_shrinkage` handles that as a cold start.
    #[tokio::test]
    async fn fetch_resolved_empty_pool_returns_empty() {
        let pool = fresh_pool().await;
        let resolved = fetch_resolved_for_brier(&pool).await.unwrap();
        assert!(resolved.is_empty());
    }

    /// All-Pending pool (no resolved rows) should also return empty, since
    /// the WHERE clause filters on `outcome IN ('Win','Loss','Push')`.
    #[tokio::test]
    async fn fetch_resolved_filters_pending_rows() {
        let pool = fresh_pool().await;
        sqlx::query("INSERT INTO predictions (id, outcome, probability) VALUES (?1, ?2, ?3)")
            .bind("p1")
            .bind("Pending")
            .bind(60.0_f64)
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO predictions (id, outcome, probability) VALUES (?1, ?2, ?3)")
            .bind("p2")
            .bind("Pending")
            .bind(70.0_f64)
            .execute(&pool)
            .await
            .unwrap();
        let resolved = fetch_resolved_for_brier(&pool).await.unwrap();
        assert!(resolved.is_empty());
    }
}
