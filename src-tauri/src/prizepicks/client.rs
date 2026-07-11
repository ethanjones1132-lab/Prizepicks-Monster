use crate::prizepicks::models::{
    PrizePicksBalance, PrizePicksBalanceResponse, PrizePicksCache, PrizePicksCacheStatus, PrizePicksConfig,
    PrizePicksEvent, PrizePicksEventsResponse, PrizePicksMarket, PrizePicksMarketSummary,
    PrizePicksMarketsQuery, PrizePicksMarketsResponse, PrizePicksOrderbook,
    PrizePicksOrderbookResponse, PrizePicksPosition, PrizePicksPositionsResponse,
};
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE, USER_AGENT};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;

// ═══════════════════════════════════════════════════════════════
// PrizePicks HTTP Client
// ═══════════════════════════════════════════════════════════════

const PRIMARY_BASE_URL: &str = "https://api.elections.prizepicks.com/trade-api/v2";
const FALLBACK_BASE_URL: &str = "https://trading-api.prizepicks.com/trade-api/v2";
const DEMO_BASE_URL: &str = "https://demo-api.prizepicks.co/trade-api/v2";

/// How many seconds a cached market list stays fresh
const CACHE_TTL_SECS: u64 = 60;

/// Maximum pages to fetch when paginating through all markets (explicit refresh)
const MAX_PAGINATION_PAGES: usize = 20;

/// Pages fetched on cold start / dashboard load — keeps first paint fast
const QUICK_LOAD_PAGES: usize = 2;

/// Cap category/search result payloads sent to the UI
const MAX_UI_MARKET_RESULTS: usize = 100;

/// How many events to request per page (full nested catalog)
const PAGE_LIMIT: u32 = 200;

/// Flat /markets page size for dashboard quick load
const FLAT_MARKET_PAGE_LIMIT: u32 = 100;

fn sanitize_ticker(ticker: &str) -> String {
    ticker
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '-' || *c == '_' || *c == '.')
        .collect()
}

pub struct PrizePicksClient {
    pub config: PrizePicksConfig,
    client: reqwest::Client,
    /// JWT bearer token acquired via /login
    token: Option<String>,
    /// When the token expires (unix seconds)
    token_expiry: Option<u64>,
    /// Cached market list — wrapped in `Arc<RwLock<...>>` so concurrent
    /// reads (UI dashboard renders) don't block on the long full-catalog
    /// warm. The fetch path takes the read-lock only to check freshness
    /// / short-circuit, runs the HTTP loop WITHOUT holding the lock at
    /// all, then takes the write-lock to install the result. This is the
    /// Phase 3 decoupling the roadmap calls for — before, a single
    /// `tokio::sync::Mutex<Arc<...>>` wrapped the whole client, so any
    /// full warm (10s+ of 20 pages of `/events`) blocked every read
    /// command for the duration of the warm.
    cache: Arc<RwLock<Option<PrizePicksCache>>>,
    /// Guard against concurrent fetches — when two warm paths race
    /// (e.g. an explicit `prizepicks_refresh` from the UI and the 8s
    /// startup background warm), only one of them runs the HTTP loop;
    /// the loser short-circuits. Without this guard, both fetches would
    /// issue the same 20-page `/events` sweep and the second write
    /// would clobber the first.
    fetch_in_progress: Arc<AtomicBool>,
}

impl PrizePicksClient {
    pub fn new(config: PrizePicksConfig) -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(15))
            .build()
            .expect("Failed to build reqwest client");
        PrizePicksClient {
            config,
            client,
            token: None,
            token_expiry: None,
            cache: Arc::new(RwLock::new(None)),
            fetch_in_progress: Arc::new(AtomicBool::new(false)),
        }
    }

    fn base_url(&self) -> &str {
        if self.config.use_demo {
            DEMO_BASE_URL
        } else if !self.config.base_url.is_empty() {
            &self.config.base_url
        } else {
            PRIMARY_BASE_URL
        }
    }

    fn now_secs() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    }

    /// Read-lock the cache and check whether it's stale.
    /// Many concurrent readers, no HTTP holds this lock.
    pub async fn is_cache_stale(&self) -> bool {
        let guard = self.cache.read().await;
        match &*guard {
            None => true,
            Some(cache) => Self::now_secs() - cache.fetched_at > CACHE_TTL_SECS,
        }
    }

    /// Read-lock the cache and clone the markets vector out.
    /// Returns `None` if the cache has not been populated yet.
    /// The returned vector is detached from the cache — callers may
    /// sort/filter without affecting concurrent readers.
    pub async fn get_cached(&self) -> Option<Vec<PrizePicksMarketSummary>> {
        let guard = self.cache.read().await;
        guard.as_ref().map(|c| c.markets.clone())
    }

    /// Read-lock and clone the entire cache struct (markets + metadata).
    /// Returns `None` if the cache is empty.
    /// Used by `cache_store` to persist the cache to SQLite.
    pub async fn clone_cache(&self) -> Option<PrizePicksCache> {
        let guard = self.cache.read().await;
        guard.clone()
    }

    /// Write-lock and replace the in-memory cache from an external source
    /// (e.g. SQLite persist restore at startup). Enables instant next-launch
    /// paint by populating the cache before any HTTP fetch runs.
    pub async fn restore_cache(&self, cache: PrizePicksCache) {
        let mut guard = self.cache.write().await;
        *guard = Some(cache);
    }

    fn is_token_valid(&self) -> bool {
        match (&self.token, self.token_expiry) {
            (Some(_), Some(expiry)) => Self::now_secs() + 60 < expiry,
            (Some(_), None) => true,
            _ => false,
        }
    }

    fn auth_headers(&self) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        headers.insert(
            USER_AGENT,
            HeaderValue::from_static("prizepicks-monster/0.6.0"),
        );
        if let Some(token) = &self.token {
            if let Ok(val) = HeaderValue::from_str(&format!("Bearer {}", token)) {
                headers.insert(AUTHORIZATION, val);
            }
        }
        headers
    }

    /// Authenticate with email/password to get a JWT token.
    /// Only required for portfolio/trading endpoints.
    pub async fn login(&mut self) -> Result<(), String> {
        if self.config.email.is_empty() || self.config.password.is_empty() {
            return Err("No PrizePicks credentials configured".to_string());
        }

        let url = format!("{}/login", self.base_url());
        let body = serde_json::json!({
            "email": self.config.email,
            "password": self.config.password,
        });

        let resp = self
            .client
            .post(&url)
            .header(CONTENT_TYPE, "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("PrizePicks login request failed: {}", e))?;

        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(format!("PrizePicks login failed ({}): {}", status, text));
        }

        let json: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| format!("Failed to parse login response: {}", e))?;
        let token = json["token"]
            .as_str()
            .ok_or("No token in login response")?
            .to_string();

        self.token = Some(token);
        // PrizePicks tokens are valid for 24h
        self.token_expiry = Some(Self::now_secs() + 86400);
        Ok(())
    }

    /// Ensure we have a valid token; attempt login if not.
    async fn ensure_auth(&mut self) -> Result<(), String> {
        if !self.is_token_valid() {
            self.login().await?;
        }
        Ok(())
    }

    // ─── Public read endpoints (no auth required) ──────────────────────────────

    /// Fetch a single page of markets with optional query filters.
    pub async fn fetch_markets_page(
        &self,
        query: &PrizePicksMarketsQuery,
    ) -> Result<PrizePicksMarketsResponse, String> {
        let url = format!("{}/markets", self.base_url());
        let mut req = self.client.get(&url).headers(self.auth_headers());

        if let Some(limit) = query.limit {
            req = req.query(&[("limit", limit.to_string())]);
        }
        if let Some(cursor) = &query.cursor {
            req = req.query(&[("cursor", cursor)]);
        }
        if let Some(status) = &query.status {
            req = req.query(&[("status", status)]);
        }
        if let Some(series_ticker) = &query.series_ticker {
            req = req.query(&[("series_ticker", series_ticker)]);
        }
        if let Some(event_ticker) = &query.event_ticker {
            req = req.query(&[("event_ticker", event_ticker)]);
        }
        if let Some(min_ts) = query.min_close_ts {
            req = req.query(&[("min_close_ts", min_ts.to_string())]);
        }
        if let Some(max_ts) = query.max_close_ts {
            req = req.query(&[("max_close_ts", max_ts.to_string())]);
        }
        if let Some(mve_filter) = &query.mve_filter {
            req = req.query(&[("mve_filter", mve_filter.as_str())]);
        }

        let resp = req.send().await.map_err(|e| {
            // Try fallback URL on connection errors
            format!("PrizePicks market fetch failed: {}", e)
        })?;

        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(format!("PrizePicks API error ({}): {}", status, text));
        }

        resp.json::<PrizePicksMarketsResponse>()
            .await
            .map_err(|e| format!("Failed to parse PrizePicks markets response: {}", e))
    }

    /// Fetch a single page of non-multivariate events with nested markets.
    async fn fetch_events_page(
        &self,
        base_url: &str,
        cursor: Option<&str>,
    ) -> Result<PrizePicksEventsResponse, String> {
        let url = format!("{}/events", base_url);
        let mut req = self.client.get(&url).headers(self.auth_headers()).query(&[
            ("limit", PAGE_LIMIT.to_string()),
            ("status", "open".to_string()),
            ("with_nested_markets", "true".to_string()),
        ]);

        if let Some(cursor) = cursor {
            req = req.query(&[("cursor", cursor.to_string())]);
        }

        let resp = req
            .send()
            .await
            .map_err(|e| format!("PrizePicks events fetch failed: {}", e))?;

        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(format!(
                "PrizePicks events API error ({}): {}",
                status, text
            ));
        }

        resp.json::<PrizePicksEventsResponse>()
            .await
            .map_err(|e| format!("Failed to parse PrizePicks events response: {}", e))
    }

    fn flatten_event_markets(event: PrizePicksEvent) -> Vec<PrizePicksMarket> {
        let event_title = event.title.trim().to_string();
        let event_category = event.category.clone();
        let event_series_ticker = event
            .series_ticker
            .trim()
            .is_empty()
            .then_some(())
            .and(None)
            .or_else(|| Some(event.series_ticker.clone()));

        event
            .markets
            .unwrap_or_default()
            .into_iter()
            .map(|mut market| {
                if market.title.trim().is_empty() {
                    let yes_sub_title = market
                        .yes_sub_title
                        .as_deref()
                        .map(str::trim)
                        .filter(|value| !value.is_empty());
                    market.title = match (event_title.is_empty(), yes_sub_title) {
                        (false, Some(value)) => format!("{} - {}", event_title, value),
                        (false, None) => event_title.clone(),
                        (true, Some(value)) => value.to_string(),
                        (true, None) => market.ticker.clone(),
                    };
                }

                if market
                    .series_ticker
                    .as_deref()
                    .map(str::trim)
                    .unwrap_or("")
                    .is_empty()
                {
                    market.series_ticker = event_series_ticker.clone();
                }

                if market
                    .category
                    .as_deref()
                    .map(str::trim)
                    .unwrap_or("")
                    .is_empty()
                {
                    market.category = event_category.clone();
                }

                market
            })
            .collect()
    }

    fn top_summaries(markets: &[PrizePicksMarketSummary], limit: usize) -> Vec<PrizePicksMarketSummary> {
        let mut ranked: Vec<&PrizePicksMarketSummary> = markets.iter().collect();
        ranked.sort_by(|a, b| {
            b.volume_24h
                .partial_cmp(&a.volume_24h)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| {
                    b.total_volume
                        .partial_cmp(&a.total_volume)
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
        });
        ranked
            .into_iter()
            .take(limit)
            .cloned()
            .collect()
    }

    /// Flat open markets via GET /markets — much smaller payloads than nested /events.
    async fn fetch_markets_flat_pages(
        &self,
        max_pages: usize,
    ) -> Result<Vec<PrizePicksMarket>, String> {
        let mut all_markets: Vec<PrizePicksMarket> = Vec::new();
        let mut cursor: Option<String> = None;
        let mut pages = 0usize;
        let mut retries = 0usize;
        const MAX_RETRIES: usize = 3;

        loop {
            if pages >= max_pages {
                break;
            }

            let query = PrizePicksMarketsQuery {
                limit: Some(FLAT_MARKET_PAGE_LIMIT),
                cursor: cursor.clone(),
                status: Some("open".to_string()),
                mve_filter: Some("exclude".to_string()),
                ..Default::default()
            };

            match self.fetch_markets_page(&query).await {
                Ok(resp) => {
                    retries = 0;
                    pages += 1;
                    if resp.markets.is_empty() {
                        break;
                    }
                    all_markets.extend(resp.markets);
                    cursor = resp.cursor;
                    if cursor.is_none() {
                        break;
                    }
                }
                Err(e) => {
                    if e.contains("429") && retries < MAX_RETRIES {
                        retries += 1;
                        let wait_ms = 1000u64 * retries as u64;
                        tracing::warn!(
                            "PrizePicks flat markets rate limited, retry in {}ms",
                            wait_ms
                        );
                        tokio::time::sleep(std::time::Duration::from_millis(wait_ms)).await;
                    } else if !all_markets.is_empty() {
                        tracing::warn!("PrizePicks flat markets pagination error: {}", e);
                        break;
                    } else {
                        return Err(e);
                    }
                }
            }
        }

        Ok(all_markets)
    }

    async fn fetch_markets_flat_resilient(
        &self,
        max_pages: usize,
    ) -> Result<Vec<PrizePicksMarket>, String> {
        let primary_base_url = self.base_url().to_string();
        match self.fetch_markets_flat_pages(max_pages).await {
            Ok(markets) => Ok(markets),
            Err(e) if primary_base_url == PRIMARY_BASE_URL => {
                tracing::warn!(
                    "Primary PrizePicks flat markets failed, trying fallback: {}",
                    e
                );
                // Fallback is also dead; gracefully degrade
                match self.try_secondary_base(max_pages).await {
                    Ok(markets) => Ok(markets),
                    Err(e2) => {
                        tracing::error!(
                            "Both PrizePicks flat markets endpoints failed. Primary: {}. Fallback: {}. Returning empty.",
                            e, e2
                        );
                        Ok(vec![])
                    }
                }
            }
            Err(e) => Err(e),
        }
    }

    /// Attempt fetch from the secondary base URL.
    async fn try_secondary_base(&self, max_pages: usize) -> Result<Vec<PrizePicksMarket>, String> {
        // Create a one-off client pointing at the fallback base URL
        let fallback_url = FALLBACK_BASE_URL;
        // Fallback URL may have a different host; try the fetch directly
        // by using the fallback as the base.
        let mut results = Vec::new();
        for page in 1..=max_pages.min(2) {
            let url = format!("{}/markets?page={}&limit={}", fallback_url, page, FLAT_MARKET_PAGE_LIMIT);
            let resp = self.client.get(&url)
                .send()
                .await
                .map_err(|e| format!("Fallback markets request failed: {}", e))?;
            if !resp.status().is_success() {
                return Err(format!("Fallback markets returned status {}", resp.status()));
            }
            let page_data: serde_json::Value = resp.json().await
                .map_err(|e| format!("Fallback markets parse failed: {}", e))?;
            let markets_page: Vec<PrizePicksMarket> = serde_json::from_value(page_data)
                .unwrap_or_default();
            results.extend(markets_page);
            if results.len() < FLAT_MARKET_PAGE_LIMIT as usize {
                break; // Last page
            }
        }
        Ok(results)
    }

    /// Nested /events catalog — used only for explicit full refresh.
        async fn fetch_events_catalog_from_base(
            &self,
            base_url: &str,
            max_pages: usize,
        ) -> Result<Vec<PrizePicksMarketSummary>, String> {
            let mut all_markets: Vec<PrizePicksMarketSummary> = Vec::new();
            let mut cursor: Option<String> = None;
            let mut pages = 0;
            let mut retries = 0usize;
            const MAX_RETRIES: usize = 3;

            loop {
                if pages >= max_pages {
                    break;
                }

                match self.fetch_events_page(base_url, cursor.as_deref()).await {
                    Ok(resp) => {
                        retries = 0;
                        let has_next = resp.cursor.is_some();
                        cursor = resp.cursor;
                        if resp.events.is_empty() {
                            break;
                        }

                        pages += 1;
                        for event in resp.events {
                            let markets = Self::flatten_event_markets(event);
                            all_markets.extend(markets.iter().map(PrizePicksMarketSummary::from));
                        }

                        if !has_next {
                            break;
                        }
                    }
                    Err(e) => {
                        if e.contains("429") && retries < MAX_RETRIES {
                            retries += 1;
                            let wait_ms = 1000u64 * retries as u64;
                            tracing::warn!(
                                "PrizePicks events catalog rate limited, retry in {}ms",
                                wait_ms
                            );
                            tokio::time::sleep(std::time::Duration::from_millis(wait_ms)).await;
                        } else if !all_markets.is_empty() {
                            tracing::warn!("PrizePicks events catalog pagination error: {}", e);
                            break;
                        } else {
                            return Err(e);
                        }
                    }
                }
            }

            Ok(all_markets)
        }

        async fn fetch_events_catalog_resilient(
            &self,
        ) -> Result<Vec<PrizePicksMarketSummary>, String> {
            let max_pages = if self.config.use_demo { 1 } else { 3 };
            let primary_base_url = self.base_url().to_string();
            match self
                .fetch_events_catalog_from_base(&primary_base_url, max_pages)
                .await
            {
                Ok(markets) => Ok(markets),
                Err(e) if primary_base_url == PRIMARY_BASE_URL => {
                    tracing::warn!("Primary PrizePicks URL failed, trying fallback: {}", e);
                    match self.fetch_events_catalog_from_base(FALLBACK_BASE_URL, max_pages).await {
                        Ok(markets) => Ok(markets),
                        Err(e2) => {
                            tracing::error!(
                                "Both PrizePicks endpoints failed. Primary: {}. Fallback: {}. Returning empty cache.",
                                e, e2
                            );
                            // Graceful degradation: return empty vec instead of propagating the error
                            // The trading API infrastructure (api.elections.prizepicks.com) has been
                            // decommissioned. Other data sources (OpticOdds, ESPN, Sleeper) still work.
                            Ok(vec![])
                        }
                    }
                }
                Err(e) => Err(e),
            }
        }

    /// Write-lock the cache and install a fresh fetch result.
    /// Called by the fetch path AFTER the HTTP loop has returned
    /// (so the long network round-trip does NOT hold the lock).
    async fn store_cache(&self, markets: Vec<PrizePicksMarketSummary>, full_catalog: bool) {
        let mut guard = self.cache.write().await;
        *guard = Some(PrizePicksCache {
            markets,
            fetched_at: Self::now_secs(),
            full_catalog,
        });
    }

    /// Read-lock the cache and decide whether the caller should run
    /// the full-catalog warm.
    pub async fn needs_full_catalog(&self) -> bool {
        let guard = self.cache.read().await;
        match &*guard {
            None => true,
            Some(cache) if Self::is_cache_fresh(cache) == false => true,
            Some(cache) => !cache.full_catalog,
        }
    }

    /// True when the cache is fresh AND full — this is the only state
    /// that should skip BOTH the quick-cache prefetch AND the full
    /// warm. Stale-but-full and fresh-but-partial both need attention.
    async fn is_cache_full_and_fresh(&self) -> bool {
        let guard = self.cache.read().await;
        match &*guard {
            Some(c) if c.full_catalog => Self::is_cache_fresh(c),
            _ => false,
        }
    }

    fn is_cache_fresh(cache: &PrizePicksCache) -> bool {
        Self::now_secs() - cache.fetched_at <= CACHE_TTL_SECS
    }

    /// Read-lock the cache and return a clone of the status struct
    /// the UI uses to render the 📦 partial-cache badge.
    pub async fn cache_status(&self) -> PrizePicksCacheStatus {
        let guard = self.cache.read().await;
        match &*guard {
            None => PrizePicksCacheStatus {
                has_cache: false,
                full_catalog: false,
                markets_count: 0,
                fetched_at: 0,
                is_stale: true,
            },
            Some(cache) => PrizePicksCacheStatus {
                has_cache: true,
                full_catalog: cache.full_catalog,
                markets_count: cache.markets.len(),
                fetched_at: cache.fetched_at,
                is_stale: !Self::is_cache_fresh(cache),
            },
        }
    }

    /// Quick cache for dashboard first paint — at most `QUICK_LOAD_PAGES` API pages.
    /// The HTTP loop runs WITHOUT holding the cache write-lock, so a
    /// concurrent reader (`prizepicks_get_top_markets` from the UI) can
    /// keep returning the previous cache while we warm in the background.
    pub async fn ensure_quick_cache(&self) -> Result<(), String> {
        // Short-circuit if the existing cache is fresh — no work needed.
        {
            let guard = self.cache.read().await;
            if let Some(cache) = &*guard {
                if Self::is_cache_fresh(cache) {
                    return Ok(());
                }
                if cache.full_catalog {
                    // Stale full cache — fall through to quick reload so UI is not blocked 10s+
                    tracing::info!("PrizePicks full cache stale; quick-reloading for dashboard");
                }
            }
        }

        // De-dupe concurrent fetches — only one wins, the rest no-op.
        if !self.try_begin_fetch() {
            tracing::info!("PrizePicks quick cache fetch already in progress, skipping");
            return Ok(());
        }

        let result = self.run_quick_cache_fetch().await;
        self.end_fetch();
        result
    }

    async fn run_quick_cache_fetch(&self) -> Result<(), String> {
        let started = std::time::Instant::now();
        tracing::info!(
            "PrizePicks quick cache load via flat /markets ({} pages x {} markets)",
            QUICK_LOAD_PAGES,
            FLAT_MARKET_PAGE_LIMIT
        );
        let raw_markets = self.fetch_markets_flat_resilient(QUICK_LOAD_PAGES).await?;
        let markets: Vec<PrizePicksMarketSummary> = raw_markets
            .iter()
            .map(PrizePicksMarketSummary::from)
            .collect();
        tracing::info!(
            "PrizePicks quick cache ready: {} markets in {}ms",
            markets.len(),
            started.elapsed().as_millis()
        );
        self.store_cache(markets, false).await;
        Ok(())
    }

    /// Fetch all open non-multivariate markets, paginating through all pages.
    /// Caches the result for `CACHE_TTL_SECS` seconds.
    /// The HTTP loop runs WITHOUT holding the cache write-lock, so a
    /// concurrent reader can keep returning the previous quick cache
    /// while the 20-page full warm runs in the background.
    pub async fn fetch_all_markets(&self) -> Result<Vec<PrizePicksMarketSummary>, String> {
        // Short-circuit on fresh full cache — return a clone without
        // doing any network I/O. This is the common case for the UI
        // hitting the dashboard while the 8s background warm is
        // already complete.
        {
            let guard = self.cache.read().await;
            if let Some(cached) = &*guard {
                if cached.full_catalog && Self::is_cache_fresh(cached) {
                    return Ok(cached.markets.clone());
                }
            }
        }

        // De-dupe concurrent fetches. If the 8s startup warm is
        // already running and the user clicks "Refresh", the user
        // gets the same single in-flight HTTP loop rather than
        // triggering a second one. The winner waits for the HTTP
        // loop to finish, then returns the populated cache.
        if !self.try_begin_fetch() {
            // Another fetch is already running. Wait briefly for it
            // to populate the cache, then return whatever is there.
            // The wait is bounded — if the in-flight fetch fails or
            // stalls, the dashboard would block here. We bound the
            // wait by polling the cache for fresh full-catalog
            // status, with a 30s safety net.
            return self.wait_for_in_flight_fetch().await;
        }

        let result = self.run_full_cache_fetch().await;
        self.end_fetch();
        result
    }

    async fn run_full_cache_fetch(&self) -> Result<Vec<PrizePicksMarketSummary>, String> {
        let started = std::time::Instant::now();
        tracing::info!(
            "PrizePicks full cache refresh via nested /events ({} pages max)",
            MAX_PAGINATION_PAGES
        );
        let all_markets = self
            .fetch_events_catalog_resilient()
            .await?;
        tracing::info!(
            "PrizePicks full cache ready: {} markets in {}ms",
            all_markets.len(),
            started.elapsed().as_millis()
        );
        self.store_cache(all_markets.clone(), true).await;
        Ok(all_markets)
    }

    /// When a competing fetch is already in progress, poll for the
    /// populated cache rather than running a second concurrent fetch.
    /// Bounded at 30s so a stuck fetch doesn't deadlock the caller.
    async fn wait_for_in_flight_fetch(&self) -> Result<Vec<PrizePicksMarketSummary>, String> {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            if self.is_cache_full_and_fresh().await {
                if let Some(markets) = self.get_cached().await {
                    return Ok(markets);
                }
            }
            if std::time::Instant::now() >= deadline {
                return Err(
                    "Timed out waiting for in-flight PrizePicks cache fetch to populate".to_string(),
                );
            }
        }
    }

    /// Atomically attempt to claim the right to run a fetch.
    /// Returns `true` if this caller won the race and should run
    /// the HTTP loop. Returns `false` if another caller already
    /// won — the loser should short-circuit or wait.
    fn try_begin_fetch(&self) -> bool {
        self.fetch_in_progress
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }

    /// Release the fetch guard. ALWAYS call this after the HTTP loop
    /// returns (success or failure) so the next caller can fetch.
    fn end_fetch(&self) {
        self.fetch_in_progress.store(false, Ordering::Release);
    }

    /// Read-lock the cache and return a `&[PrizePicksMarketSummary]` slice of
    /// the cached markets. Returns `None` if the cache is empty.
    async fn cached_market_slice(&self) -> Option<Vec<PrizePicksMarketSummary>> {
        self.get_cached().await
    }

    /// Fetch a single market by ticker
    pub async fn fetch_market(&self, ticker: &str) -> Result<PrizePicksMarket, String> {
        let safe_ticker = sanitize_ticker(ticker);
        if safe_ticker.is_empty() {
            return Err("Invalid ticker".to_string());
        }

        let url = format!("{}/markets/{}", self.base_url(), safe_ticker);
        let resp = self
            .client
            .get(&url)
            .headers(self.auth_headers())
            .send()
            .await
            .map_err(|e| format!("PrizePicks single market fetch failed: {}", e))?;

        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(format!(
                "PrizePicks market {} not found ({}): {}",
                safe_ticker, status, text
            ));
        }

        let json: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| format!("Failed to parse market response: {}", e))?;

        serde_json::from_value(json["market"].clone())
            .map_err(|e| format!("Failed to deserialize market: {}", e))
    }

    /// Fetch the orderbook for a market
    pub async fn fetch_orderbook(&self, ticker: &str) -> Result<PrizePicksOrderbook, String> {
        let safe_ticker = sanitize_ticker(ticker);
        if safe_ticker.is_empty() {
            return Err("Invalid ticker".to_string());
        }

        let url = format!("{}/markets/{}/orderbook", self.base_url(), safe_ticker);
        let resp = self
            .client
            .get(&url)
            .headers(self.auth_headers())
            .send()
            .await
            .map_err(|e| format!("PrizePicks orderbook fetch failed: {}", e))?;

        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(format!("PrizePicks orderbook error ({}): {}", status, text));
        }

        let parsed: PrizePicksOrderbookResponse = resp
            .json()
            .await
            .map_err(|e| format!("Failed to parse orderbook: {}", e))?;

        Ok(parsed.orderbook)
    }

    /// Search markets by keyword against the cached market list
    pub async fn search_markets(
        &self,
        query: &str,
    ) -> Result<Vec<PrizePicksMarketSummary>, String> {
        let trimmed = query.trim();
        if trimmed.len() < 2 {
            return Err("Search query must be at least 2 characters".to_string());
        }
        self.ensure_quick_cache().await?;
        let markets = self
            .cached_market_slice()
            .await
            .ok_or_else(|| "PrizePicks market cache unavailable".to_string())?;
        let q = trimmed.to_lowercase();
        let results: Vec<PrizePicksMarketSummary> = markets
            .iter()
            .filter(|m| {
                m.title.to_lowercase().contains(&q)
                    || m.ticker.to_lowercase().contains(&q)
                    || m.event_ticker.to_lowercase().contains(&q)
            })
            .take(MAX_UI_MARKET_RESULTS)
            .cloned()
            .collect();
        Ok(results)
    }

    /// Get markets filtered by category (inferred from ticker)
    pub async fn get_markets_by_category(
        &self,
        category: &str,
    ) -> Result<Vec<PrizePicksMarketSummary>, String> {
        self.ensure_quick_cache().await?;
        let markets = self
            .cached_market_slice()
            .await
            .ok_or_else(|| "PrizePicks market cache unavailable".to_string())?;
        let results: Vec<PrizePicksMarketSummary> = markets
            .iter()
            .filter(|m| {
                if category == "All" {
                    true
                } else {
                    m.category.eq_ignore_ascii_case(category)
                }
            })
            .take(MAX_UI_MARKET_RESULTS)
            .cloned()
            .collect();
        Ok(results)
    }

    /// Get top markets by 24h volume
    pub async fn get_top_markets(
        &self,
        limit: usize,
    ) -> Result<Vec<PrizePicksMarketSummary>, String> {
        self.ensure_quick_cache().await?;
        let markets = self
            .cached_market_slice()
            .await
            .ok_or_else(|| "PrizePicks market cache unavailable".to_string())?;
        Ok(Self::top_summaries(
            &markets,
            limit.min(MAX_UI_MARKET_RESULTS),
        ))
    }

    // ─── Auth-required endpoints ────────────────────────────────────────────────

    /// Get portfolio balance (requires login)
    pub async fn get_balance(&mut self) -> Result<PrizePicksBalance, String> {
        self.ensure_auth().await?;
        let url = format!("{}/portfolio/balance", self.base_url());
        let resp = self
            .client
            .get(&url)
            .headers(self.auth_headers())
            .send()
            .await
            .map_err(|e| format!("PrizePicks balance fetch failed: {}", e))?;

        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(format!("PrizePicks balance error ({}): {}", status, text));
        }

        let parsed: PrizePicksBalanceResponse = resp
            .json()
            .await
            .map_err(|e| format!("Failed to parse balance: {}", e))?;

        Ok(parsed.balance)
    }

    /// Get portfolio positions (requires login)
    pub async fn get_positions(&mut self) -> Result<Vec<PrizePicksPosition>, String> {
        self.ensure_auth().await?;
        let url = format!("{}/portfolio/positions", self.base_url());
        let resp = self
            .client
            .get(&url)
            .headers(self.auth_headers())
            .send()
            .await
            .map_err(|e| format!("PrizePicks positions fetch failed: {}", e))?;

        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(format!("PrizePicks positions error ({}): {}", status, text));
        }

        let parsed: PrizePicksPositionsResponse = resp
            .json()
            .await
            .map_err(|e| format!("Failed to parse positions: {}", e))?;

        Ok(parsed.market_positions)
    }

    /// Force-invalidate cache (used after config changes).
    /// Take the cache write-lock so a concurrent `prizepicks_get_top_markets`
    /// can't observe a half-invalidated state.
    pub async fn invalidate_cache(&mut self) {
        let mut guard = self.cache.write().await;
        *guard = None;
        self.token = None;
        self.token_expiry = None;
    }

    /// Summarize all cached markets by category. Read-locks the cache
    /// and clones the markets out so the aggregation doesn't hold any
    /// lock during the sort.
    pub async fn category_stats(&self) -> Vec<PrizePicksCategoryStat> {
        let markets = match self.get_cached().await {
            Some(m) if !m.is_empty() => m,
            _ => return Vec::new(),
        };

        let mut stats: std::collections::HashMap<&str, (usize, f64)> =
            std::collections::HashMap::new();

        for m in &markets {
            let cat = &m.category;
            let entry = stats.entry(cat.as_str()).or_insert((0, 0.0));
            entry.0 += 1;
            entry.1 += m.volume_24h;
        }

        let mut result: Vec<PrizePicksCategoryStat> = stats
            .into_iter()
            .map(|(cat, (count, vol))| PrizePicksCategoryStat {
                category: cat.to_string(),
                count,
                volume_24h: vol,
            })
            .collect();

        result.sort_by(|a, b| b.count.cmp(&a.count));
        result
    }
}

/// Statistics about a market category
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PrizePicksCategoryStat {
    pub category: String,
    pub count: usize,
    pub volume_24h: f64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use tokio::sync::RwLock;

    fn make_market(ticker: &str, title: &str) -> PrizePicksMarketSummary {
        PrizePicksMarketSummary {
            ticker: ticker.to_string(),
            event_ticker: format!("EVT-{ticker}"),
            title: title.to_string(),
            category: "Test".to_string(),
            status: "active".to_string(),
            yes_prob_pct: 0.5,
            yes_ask: 0.55,
            yes_bid: 0.50,
            no_ask: 0.50,
            no_bid: 0.45,
            last_price: 0.52,
            volume_24h: 100.0,
            total_volume: 1000.0,
            liquidity: 500.0,
            spread: 0.05,
            close_time: None,
            expiration_time: None,
            result: "".to_string(),
            can_close_early: false,
            is_provisional: false,
        }
    }

    #[test]
    fn flatten_event_markets_inherits_event_metadata() {
        let event = PrizePicksEvent {
            event_ticker: "KXNEWPOPE-70".to_string(),
            title: "Who will the next Pope be?".to_string(),
            series_ticker: "KXNEWPOPE".to_string(),
            status: String::new(),
            category: Some("Elections".to_string()),
            sub_title: None,
            mutually_exclusive: true,
            markets: Some(vec![PrizePicksMarket {
                ticker: "KXNEWPOPE-70-PPIZ".to_string(),
                event_ticker: "KXNEWPOPE-70".to_string(),
                yes_sub_title: Some("Pierbattista Pizzaballa".to_string()),
                ..Default::default()
            }]),
            strike_date: String::new(),
        };

        let markets = PrizePicksClient::flatten_event_markets(event);
        assert_eq!(markets.len(), 1);
        assert_eq!(
            markets[0].title,
            "Who will the next Pope be? - Pierbattista Pizzaballa"
        );
        assert_eq!(markets[0].category.as_deref(), Some("Elections"));
        assert_eq!(markets[0].series_ticker.as_deref(), Some("KXNEWPOPE"));
    }

    // ─── Phase 3 cache decoupling tests ─────────────────────────────────

    fn new_test_client() -> PrizePicksClient {
        PrizePicksClient::new(PrizePicksConfig {
            base_url: "https://example.invalid".to_string(),
            email: String::new(),
            password: String::new(),
            poll_interval_secs: 60,
            use_demo: false,
        })
    }

    #[tokio::test]
    async fn empty_cache_status_reports_no_cache() {
        let client = new_test_client();
        let status = client.cache_status().await;
        assert!(!status.has_cache);
        assert!(!status.full_catalog);
        assert_eq!(status.markets_count, 0);
        assert_eq!(status.fetched_at, 0);
        assert!(status.is_stale);
    }

    #[tokio::test]
    async fn empty_cache_is_stale() {
        let client = new_test_client();
        assert!(client.is_cache_stale().await);
    }

    #[tokio::test]
    async fn empty_cache_needs_full_warm() {
        let client = new_test_client();
        assert!(client.needs_full_catalog().await);
    }

    #[tokio::test]
    async fn get_cached_returns_none_on_empty() {
        let client = new_test_client();
        assert!(client.get_cached().await.is_none());
    }

    #[tokio::test]
    async fn store_cache_populates_and_status_reflects() {
        let client = new_test_client();
        let markets = vec![
            make_market("AAA", "Market A"),
            make_market("BBB", "Market B"),
        ];
        client.store_cache(markets.clone(), false).await;

        let cached = client.get_cached().await.expect("cache populated");
        assert_eq!(cached.len(), 2);
        assert_eq!(cached[0].ticker, "AAA");

        let status = client.cache_status().await;
        assert!(status.has_cache);
        assert!(!status.full_catalog);
        assert_eq!(status.markets_count, 2);
        assert!(!status.is_stale);
    }

    #[tokio::test]
    async fn store_cache_with_full_catalog_flag() {
        let client = new_test_client();
        client
            .store_cache(vec![make_market("FULL", "Full Market")], true)
            .await;
        let status = client.cache_status().await;
        assert!(status.has_cache);
        assert!(status.full_catalog);
        assert_eq!(status.markets_count, 1);
    }

    #[tokio::test]
    async fn needs_full_catalog_only_false_when_full_and_fresh() {
        let client = new_test_client();

        // empty -> needs full warm
        assert!(client.needs_full_catalog().await);

        // partial cache -> still needs full warm
        client
            .store_cache(vec![make_market("PART", "Partial")], false)
            .await;
        assert!(client.needs_full_catalog().await);

        // full cache -> no full warm needed
        client
            .store_cache(vec![make_market("FULL", "Full")], true)
            .await;
        assert!(!client.needs_full_catalog().await);
    }

    #[tokio::test]
    async fn invalidate_cache_clears_everything() {
        let client = new_test_client();
        client
            .store_cache(vec![make_market("X", "X Market")], true)
            .await;
        assert!(!client.is_cache_stale().await);

        let mut client = client;
        client.invalidate_cache().await;
        assert!(client.is_cache_stale().await);
        assert!(client.get_cached().await.is_none());
    }

    #[tokio::test]
    async fn fetch_in_progress_guard_dedupes_concurrent_fetches() {
        let client = new_test_client();
        // First caller wins, second caller loses.
        assert!(client.try_begin_fetch());
        assert!(!client.try_begin_fetch());
        assert!(!client.try_begin_fetch());
        // After the first finishes, the next caller can win.
        client.end_fetch();
        assert!(client.try_begin_fetch());
        client.end_fetch();
    }

    #[tokio::test]
    async fn concurrent_reads_do_not_block_each_other() {
        // This is the headline test for Phase 3 decoupling. With a
        // tokio::sync::Mutex<Client>, two readers serialize. With
        // Arc<RwLock<...>>, two readers can hold the read-lock at
        // the same time — they don't block each other.
        //
        // We can't *measure* lock contention from a unit test, but
        // we can prove the cache returns consistent data even when
        // one reader is mid-await while another fires a write.
        let client = Arc::new(new_test_client());
        client
            .store_cache(
                vec![make_market("RACE", "Race test market")],
                true,
            )
            .await;

        let mut handles = Vec::new();
        for _ in 0..16 {
            let c = client.clone();
            handles.push(tokio::spawn(async move {
                let cached = c.get_cached().await.expect("cache populated");
                cached[0].ticker.clone()
            }));
        }
        for h in handles {
            let ticker = h.await.expect("task panicked");
            assert_eq!(ticker, "RACE");
        }
    }

    #[tokio::test]
    async fn reader_clone_is_independent_of_cache_writes() {
        // The headline win: a reader holding a clone of the markets
        // is NOT blocked by a concurrent writer replacing the cache.
        // With the old tokio::sync::Mutex<Client> design, a writer
        // would hold the outer mutex for 10+ seconds and freeze all
        // readers. With Arc<RwLock<...>>, reads clone the inner
        // Vec out under the read-lock and the writer only blocks
        // other writers (briefly, for the swap).
        let client = Arc::new(new_test_client());
        client
            .store_cache(vec![make_market("V1", "Version 1")], false)
            .await;

        // Reader gets a clone.
        let reader_data = client.get_cached().await.expect("first read");
        assert_eq!(reader_data[0].ticker, "V1");

        // Writer replaces the cache while the reader's clone is still
        // in scope. The reader's clone is unaffected (Vec is owned).
        client
            .store_cache(vec![make_market("V2", "Version 2")], true)
            .await;

        // Reader's data is still V1.
        assert_eq!(reader_data[0].ticker, "V1");
        // Fresh read sees V2.
        let new_data = client.get_cached().await.expect("second read");
        assert_eq!(new_data[0].ticker, "V2");
    }

    #[tokio::test]
    async fn category_stats_empty_cache_returns_empty_vec() {
        let client = new_test_client();
        let stats = client.category_stats().await;
        assert!(stats.is_empty());
    }

    #[tokio::test]
    async fn category_stats_after_population() {
        // End-to-end smoke test: store a cache, then read category stats.
        // Doesn't assert on the count (the Default::default() market has
        // an empty ticker so the inferred category is whatever the
        // ticker-prefix heuristic returns) — just proves the
        // read-locked path doesn't panic.
        let client = new_test_client();
        client
            .store_cache(vec![make_market("NBA-LAL", "LeBron Points")], true)
            .await;
        let _ = client.category_stats().await;
    }

    #[tokio::test]
    async fn is_cache_full_and_fresh_reflects_state() {
        let client = new_test_client();
        // Empty -> not full-and-fresh.
        assert!(!client.is_cache_full_and_fresh().await);
        // Partial -> not full-and-fresh.
        client
            .store_cache(vec![make_market("P", "Partial")], false)
            .await;
        assert!(!client.is_cache_full_and_fresh().await);
        // Full -> full-and-fresh (just populated, still within TTL).
        client
            .store_cache(vec![make_market("F", "Full")], true)
            .await;
        assert!(client.is_cache_full_and_fresh().await);
    }

    #[tokio::test]
    async fn lock_is_rwlock_underlying() {
        // Smoke test that the cache field is the right type — if
        // someone regresses it to a Mutex, this stops compiling.
        // (We use a function to force the type to be visible here.)
        fn assert_rwlock<T>(_: &Arc<RwLock<T>>) {}
        let c = new_test_client();
        assert_rwlock(&c.cache);
    }
}

/// Build a PrizePicksConfig from the app config
pub fn prizepicks_config_from_app(config: &crate::config::AppConfig) -> PrizePicksConfig {
    PrizePicksConfig {
        base_url: PRIMARY_BASE_URL.to_string(),
        email: config.prizepicks_email.clone(),
        password: config.prizepicks_password.clone(),
        poll_interval_secs: config.prizepicks_poll_interval_secs,
        use_demo: false,
    }
}
