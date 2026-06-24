//! Professional Decision Schema for PrizePicks Player Prop Analysis
//!
//! Every player-prop analysis output should support this structured format,
//! enabling the frontend to render paper-sim tickets, journal entries,
//! and risk alerts with full data fidelity.
//!
//! Legacy JSON field names such as `contract_side` and `market_price_pct` are kept
//! for tracker compatibility. User-facing prompt language should stay DFS-specific:
//! player props, Over/Under lines, projections, picks, and paper-sim sizing.

use serde::{Deserialize, Serialize};

/// Professional research decision for a PrizePicks player prop.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct PrizePicksTradeDecision {
    /// PrizePicks prop/ticker identifier for tracking.
    pub ticker: String,
    /// Human-readable prop title.
    pub market_title: String,
    /// League/category: NFL, NBA, MLB, NHL, or Other.
    pub category: String,
    /// Tracking side for the pick. For Over/Under props, map Over/Under into this field.
    pub contract_side: ContractSide,
    /// Implied probability for the selected side (0.0–1.0).
    pub market_price_pct: f64,
    /// Model's fair probability estimate (0.0–100.0)
    pub fair_probability_pct: f64,
    /// Edge in percentage points (fair_probability – market_price * 100)
    pub edge_points: f64,
    /// Line spread in cents.
    pub spread_cents: f64,
    /// Data/liquidity score: 0–100 (higher = deeper data).
    pub liquidity_score: f64,
    /// EV per pick/contract unit in cents.
    pub ev_per_contract_cents: f64,
    /// EV as a percentage ROI
    pub ev_roi_pct: f64,
    /// Raw Kelly percentage (unbounded, can be >100%)
    pub raw_kelly_pct: f64,
    /// Recommended fractional Kelly percentage (conservative)
    pub fractional_kelly_pct: f64,
    /// Recommended paper-sim stake in dollars.
    pub recommended_stake_dollars: f64,
    /// Maximum paper-sim position size in dollars.
    pub max_position_dollars: f64,
    /// Final research decision.
    pub decision: DecisionAction,
    /// Confidence tier
    pub confidence_tier: ConfidenceTier,
    /// Calibrated thesis (2–3 sentences)
    pub thesis: String,
    /// Supporting evidence bullets
    pub evidence: Vec<String>,
    /// Risk flags identified
    pub risk_flags: Vec<RiskFlag>,
    /// Quality rating of the data behind this decision
    pub data_quality: DataQuality,
    /// Price or line to enter the paper-sim position.
    pub price_to_enter: f64,
}

/// Tracking side for a binary-style PrizePicks decision.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub enum ContractSide {
    YES,
    NO,
    #[default]
    PASS,
}

/// Final recommended action
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub enum DecisionAction {
    /// Record paper-sim TAKE — the edge justifies research action.
    TAKE,
    /// Monitor — not enough edge or data to act
    WATCH,
    /// Skip — negative EV or excessive risk
    #[default]
    PASS,
}

/// Confidence tier based on model certainty and data quality
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub enum ConfidenceTier {
    /// Strong conviction + excellent data quality
    High,
    /// Moderate conviction + good data quality
    Medium,
    /// Weak conviction or incomplete data
    Low,
    /// No confidence — default for PASS
    #[default]
    None,
}

/// Risk flags that can downgrade or invalidate a paper-sim pick.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum RiskFlag {
    /// Line/spread is wider than the estimated edge.
    SpreadExceedsEdge,
    /// Insufficient data depth for the recommended stake.
    InsufficientLiquidity,
    /// High correlation with an existing paper-sim position.
    CorrelatedExposure,
    /// Prop uses provisional settlement rules.
    ProvisionalSettlement,
    /// Prop can close before expected.
    EarlyCloseRisk,
    /// Extreme probability (>90% or <10%)
    ExtremeProbability,
    /// Resolution criteria are ambiguous
    AmbiguousResolution,
    /// Data is stale or incomplete
    StaleData,
    /// Paper-sim exposure would exceed maximum portfolio allocation.
    ConcentrationRisk,
    /// Other unspecified risk
    Other(String),
}

/// Quality of the data used to make this decision
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub enum DataQuality {
    /// Real-time PrizePicks/player prop data with live line context.
    Live,
    /// Cached data < 60 seconds old
    Fresh,
    /// Cached data 1–5 minutes old
    Stale,
    /// No direct prop data — reasoning from base rates and news only.
    #[default]
    Inferential,
    /// Speculative — very limited data
    Speculative,
}

impl PrizePicksTradeDecision {
    /// Create a new decision with sensible defaults.
    pub fn new(ticker: &str, market_title: &str) -> Self {
        Self {
            ticker: ticker.to_string(),
            market_title: market_title.to_string(),
            category: "Other".to_string(),
            contract_side: ContractSide::PASS,
            market_price_pct: 0.0,
            fair_probability_pct: 50.0,
            edge_points: 0.0,
            spread_cents: 0.0,
            liquidity_score: 0.0,
            ev_per_contract_cents: 0.0,
            ev_roi_pct: 0.0,
            raw_kelly_pct: 0.0,
            fractional_kelly_pct: 0.0,
            recommended_stake_dollars: 0.0,
            max_position_dollars: 0.0,
            decision: DecisionAction::PASS,
            confidence_tier: ConfidenceTier::None,
            thesis: String::new(),
            evidence: Vec::new(),
            risk_flags: Vec::new(),
            data_quality: DataQuality::Inferential,
            price_to_enter: 0.0,
        }
    }

    /// Compute edge, EV, and paper-sim Kelly sizing from implied probability and fair probability.
    /// Call this after setting market_price_pct and fair_probability_pct.
    pub fn compute(&mut self, bankroll_dollars: f64, kelly_fraction: f64, max_bet_pct: f64) {
        let market_price = self.market_price_pct / 100.0;
        let fair_prob = self.fair_probability_pct / 100.0;

        if market_price <= 0.0 || market_price >= 1.0 || fair_prob <= 0.0 || fair_prob >= 1.0 {
            self.edge_points = 0.0;
            self.ev_roi_pct = 0.0;
            self.raw_kelly_pct = 0.0;
            self.fractional_kelly_pct = 0.0;
            self.recommended_stake_dollars = 0.0;
            return;
        }

        // Edge in percentage points for the selected tracking side.
        self.edge_points = if self.contract_side == ContractSide::YES {
            (fair_prob - market_price) * 100.0
        } else if self.contract_side == ContractSide::NO {
            (market_price - fair_prob) * 100.0
        } else {
            0.0
        };

        // EV per pick/contract unit.
        if self.contract_side == ContractSide::YES {
            self.ev_per_contract_cents = (fair_prob - market_price) * 100.0;
            self.ev_roi_pct = ((fair_prob / market_price) - 1.0) * 100.0;
        } else if self.contract_side == ContractSide::NO {
            let no_price = 1.0 - market_price;
            let no_fair = 1.0 - fair_prob;
            self.ev_per_contract_cents = (no_fair - no_price) * 100.0;
            self.ev_roi_pct = ((no_fair / no_price) - 1.0) * 100.0;
        } else {
            self.ev_per_contract_cents = 0.0;
            self.ev_roi_pct = 0.0;
        }

        // Kelly Criterion: f* = (p * b - q) / b
        let raw_kelly = if self.contract_side == ContractSide::YES {
            let p = fair_prob;
            let q = 1.0 - p;
            let b = (1.0 - market_price) / market_price;
            if b > 0.0 {
                (p * b - q) / b
            } else {
                0.0
            }
        } else if self.contract_side == ContractSide::NO {
            let p = 1.0 - fair_prob;
            let q = 1.0 - p;
            let b = market_price / (1.0 - market_price);
            if b > 0.0 {
                (p * b - q) / b
            } else {
                0.0
            }
        } else {
            0.0
        };

        self.raw_kelly_pct = raw_kelly.max(0.0) * 100.0;
        self.fractional_kelly_pct = self.raw_kelly_pct * kelly_fraction;
        self.recommended_stake_dollars = bankroll_dollars * (self.fractional_kelly_pct / 100.0);

        // Liquidity score: simplistic scoring based on volume
        self.liquidity_score = ((self.liquidity_score / 50000.0) * 100.0).min(100.0);

        // Max paper-sim position: cap at config max_bet_pct of bankroll (persisted localMaxBetPct)
        let max_pct = if max_bet_pct > 0.0 { max_bet_pct } else { 0.05 };
        self.max_position_dollars = (bankroll_dollars * max_pct).min(self.recommended_stake_dollars);
    }

    /// Compute with isotonic calibration and portfolio correlation Kelly scaling.
    pub fn compute_risk_adjusted(
        &mut self,
        bankroll_dollars: f64,
        kelly_fraction: f64,
        kelly_scale: f64,
        max_bet_pct: f64,
        apply_calibrator: bool,
    ) {
        if apply_calibrator {
            let cal = crate::analysis::calibration::calibrate_yes_probability_pct(
                self.fair_probability_pct,
            );
            if cal.applied {
                self.fair_probability_pct = cal.calibrated_pct;
            }
        }
        self.compute(bankroll_dollars, kelly_fraction, max_bet_pct);
        let scale = kelly_scale.clamp(0.0, 1.0);
        if scale < 1.0 {
            self.fractional_kelly_pct *= scale;
            self.recommended_stake_dollars *= scale;
            self.max_position_dollars = self
                .max_position_dollars
                .min(self.recommended_stake_dollars);
            if !self.risk_flags.contains(&RiskFlag::CorrelatedExposure) {
                self.risk_flags.push(RiskFlag::CorrelatedExposure);
            }
        }
    }

    /// Return true if the decision passes all risk checks
    pub fn is_actionable(&self) -> bool {
        if self.decision != DecisionAction::TAKE {
            return false;
        }
        if !self.risk_flags.is_empty() {
            // Any risk flag except StaleData might be acceptable
            return self
                .risk_flags
                .iter()
                .all(|f| matches!(f, RiskFlag::StaleData));
        }
        true
    }

    /// Generate the AI prompt fragment describing this decision structure.
    pub fn prompt_schema() -> String {
        String::from(
            r#"## PRIZEPICKS PLAYER PROP DECISION SCHEMA

Every PrizePicks player prop analysis must output a JSON block with the following fields FIRST.
This is for DFS research and paper-sim tracking only. Do not instruct users to place real bets or submit orders.
Use player prop terms: prop, line, projection, Over, Under, pick, and paper-sim.
Legacy field names are kept for tracker compatibility.

{
  "ticker": "NFL-JoshAllen-O-275.5-2026-09-10",
  "market_title": "Josh Allen Over 275.5 passing yards",
  "category": "NFL",
  "contract_side": "YES",
  "market_price_pct": 55.0,
  "fair_probability_pct": 62.0,
  "edge_points": 7.0,
  "spread_cents": 3.0,
  "liquidity_score": 75.0,
  "ev_per_contract_cents": 7.0,
  "ev_roi_pct": 12.7,
  "raw_kelly_pct": 22.4,
  "fractional_kelly_pct": 5.6,
  "recommended_stake_dollars": 56.0,
  "max_position_dollars": 50.0,
  "decision": "TAKE",
  "confidence_tier": "High",
  "thesis": "Allen's last-three passing volume and matchup usage support the Over at this line.",
  "evidence": [
    "Allen averaged 301 passing yards over the last three games",
    "Opponent pass defense ranks bottom-third on early downs",
    "Projection: 289.0 yards vs line 275.5"
  ],
  "risk_flags": ["AmbiguousResolution"],
  "data_quality": "Live",
  "price_to_enter": 0.55
}

RULES:
- "decision" must be "TAKE", "WATCH", or "PASS".
- "contract_side" must be "YES", "NO", or "PASS"; for Over/Under prop user output, translate Over/Under into this tracking field.
- "confidence_tier" must be "High", "Medium", "Low", or "None".
- "data_quality" must be "Live", "Fresh", "Stale", "Inferential", or "Speculative".
- "risk_flags" can include: SpreadExceedsEdge, InsufficientLiquidity, CorrelatedExposure, ProvisionalSettlement, EarlyCloseRisk, ExtremeProbability, AmbiguousResolution, StaleData, ConcentrationRisk.
- JSON must be valid. No trailing commas. Place it FIRST in the response.
- This is research/paper-sim only. Never recommend placing a real bet or submitting an order.

After the JSON, provide a concise readable summary:
- DECISION: [TAKE/WATCH/PASS] [Over/Under or tracking side] at [line/probability]
- LINE VS FAIR: [line/probability] vs [fair]%
- EDGE: [edge points] pts, [EV ROI]% EV ROI
- SIZE: [raw Kelly]% raw Kelly, [fractional Kelly]% paper-sim sizing
- WHY: [thesis]
- RISK CONTROL: [key risk flags and invalidation conditions]
"#,
        )
    }
}

// ═══════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_kelly_calculation() {
        let mut decision = PrizePicksTradeDecision::new(
            "NFL-JoshAllen-O-275.5",
            "Josh Allen Over 275.5 passing yards",
        );
        decision.market_price_pct = 55.0;
        decision.fair_probability_pct = 62.0;
        decision.contract_side = ContractSide::YES;
        decision.liquidity_score = 50000.0; // Will be normalized to 100
        decision.compute(1000.0, 0.25);

        assert!((decision.edge_points - 7.0).abs() < 0.01);
        assert!(decision.ev_roi_pct > 0.0);
        assert!(decision.raw_kelly_pct > 0.0);
        assert!(decision.fractional_kelly_pct > 0.0);
        assert!(decision.recommended_stake_dollars > 0.0);
    }

    #[test]
    fn test_negative_ev_passes() {
        let mut decision = PrizePicksTradeDecision::new(
            "NFL-FakePlayer-U-25.5",
            "Fake Player Under 25.5 fantasy points",
        );
        decision.market_price_pct = 80.0;
        decision.fair_probability_pct = 70.0;
        decision.contract_side = ContractSide::YES;
        decision.compute(1000.0, 0.25);

        // Edge is negative — should not recommend a stake
        assert!(decision.edge_points < 0.0);
        assert!(decision.recommended_stake_dollars == 0.0);
    }

    #[test]
    fn test_spread_exceeds_edge_flag() {
        let risk = RiskFlag::SpreadExceedsEdge;
        match risk {
            RiskFlag::SpreadExceedsEdge => {}
            _ => panic!("Expected SpreadExceedsEdge"),
        }
    }

    #[test]
    fn test_decision_enum_serialization() {
        let take = DecisionAction::TAKE;
        let json = serde_json::to_string(&take).unwrap();
        assert_eq!(json, "\"TAKE\"");

        let parsed: DecisionAction = serde_json::from_str("\"PASS\"").unwrap();
        assert_eq!(parsed, DecisionAction::PASS);
    }

    #[test]
    fn test_is_actionable_with_risk_flags() {
        let mut decision = PrizePicksTradeDecision::new(
            "NFL-TestPlayer-O-75.5",
            "Test Player Over 75.5 receiving yards",
        );
        decision.decision = DecisionAction::TAKE;
        assert!(decision.is_actionable());

        decision.risk_flags.push(RiskFlag::SpreadExceedsEdge);
        assert!(!decision.is_actionable()); // Now has a blocking flag
    }

    #[test]
    fn test_contract_side_no_ev() {
        let mut decision =
            PrizePicksTradeDecision::new("NBA-TestPlayer-U-25.5", "Test Player Under 25.5 points");
        decision.market_price_pct = 60.0;
        decision.fair_probability_pct = 40.0;
        decision.contract_side = ContractSide::NO;
        decision.compute(1000.0, 0.25);

        assert!((decision.edge_points - 20.0).abs() < 0.01);
        assert!((decision.raw_kelly_pct - 33.33).abs() < 0.05);
        assert!((decision.fractional_kelly_pct - 8.33).abs() < 0.05);
        assert!((decision.recommended_stake_dollars - 83.33).abs() < 0.5);
        assert!(decision.ev_roi_pct > 0.0);
    }

    #[test]
    fn test_prompt_schema_is_prizepicks_prop_first() {
        let prompt = PrizePicksTradeDecision::prompt_schema();

        assert!(prompt.contains("PRIZEPICKS PLAYER PROP DECISION SCHEMA"));
        assert!(prompt.contains("Over/Under"));
        assert!(prompt.contains("paper-sim"));
        assert!(!prompt.contains("event contracts"));
        assert!(!prompt.contains("KXEVENT"));
        assert!(!prompt.contains("Fed"));
        assert!(!prompt.contains("orderbook"));
        assert!(!prompt.contains("Execute the trade"));
    }
}
