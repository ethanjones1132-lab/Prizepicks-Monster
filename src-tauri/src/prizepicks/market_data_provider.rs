//! Trait for fetching PrizePicks market data.
//!
//! Creating a seam between market-data consumers (paper trading, grading)
//! and the concrete HTTP client makes tests possible without a live API
//! and keeps the domain logic decoupled from transport.

use async_trait::async_trait;
use crate::prizepicks::client::PrizePicksClient;
use crate::prizepicks::models::{PrizePicksMarket, PrizePicksOrderbook};

/// Anything that can answer market queries — the PrizePicks API, a
/// cached snapshot, or a test double.
#[async_trait]
pub trait MarketDataProvider: Send + Sync {
    async fn get_market(&self, ticker: &str) -> Result<PrizePicksMarket, String>;
    async fn get_orderbook(&self, ticker: &str) -> Result<PrizePicksOrderbook, String>;
}

#[async_trait]
impl MarketDataProvider for PrizePicksClient {
    async fn get_market(&self, ticker: &str) -> Result<PrizePicksMarket, String> {
        self.fetch_market(ticker).await
    }

    async fn get_orderbook(&self, ticker: &str) -> Result<PrizePicksOrderbook, String> {
        self.fetch_orderbook(ticker).await
    }
}
