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

    Ok(crate::prizepicks::portfolio_risk::compute_stake_adjustment(
        &ticker,
        &category,
        Some(&contract_side),
        recommended_stake,
        &exposures,
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

    let side = format!("{:?}", decision.contract_side);
    let raw_stake = if decision.recommended_stake_dollars > 0.0 {
        decision.recommended_stake_dollars
    } else {
        bankroll.total_bankroll * (decision.fractional_kelly_pct / 100.0)
    };
    let adj = crate::prizepicks::portfolio_risk::compute_stake_adjustment(
        &decision.ticker,
        &decision.category,
        Some(&side),
        raw_stake,
        &exposures,
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
