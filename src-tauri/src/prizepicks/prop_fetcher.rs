use serde::{Deserialize, Serialize};

// ═══════════════════════════════════════════════════════════════
// PrizePicks Player Prop Data Models
// Multi-source: OpticOdds → Direct Scrape → Mock
// ═══════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PrizePicksProp {
    pub external_id: String,
    pub player_name: String,
    pub team: String,
    pub opponent: String,
    pub stat_category: String,
    pub line: f64,
    pub league: String,
    pub projection: Option<f64>,
    pub source: String,
    pub game_time: Option<String>,
    pub over_odds: Option<i32>,
    pub under_odds: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PropsResponse {
    pub props: Vec<PrizePicksProp>,
    pub source: String,
}

// ═══════════════════════════════════════════════════════════════
// OpticOdds API Types
// ═══════════════════════════════════════════════════════════════

#[derive(Debug, Deserialize)]
struct OpticOddsFixturesResp {
    data: Vec<OpticOddsFixture>,
}

#[derive(Debug, Deserialize)]
struct OpticOddsFixture {
    id: String,
    home_team: Option<OpticOddsTeam>,
    away_team: Option<OpticOddsTeam>,
}

#[derive(Debug, Deserialize)]
struct OpticOddsTeam {
    name: String,
}

#[derive(Debug, Deserialize)]
struct OpticOddsOddsResp {
    data: Vec<OpticOddsOddsEntry>,
}

#[derive(Debug, Deserialize)]
struct OpticOddsOddsEntry {
    fixture_id: Option<String>,
    sportsbook: Option<String>,
    market: Option<String>,
    name: Option<String>,
    odds: Option<Vec<OpticOddsOddsSide>>,
    player: Option<OpticOddsPlayer>,
}

#[derive(Debug, Deserialize)]
struct OpticOddsOddsSide {
    name: Option<String>,
    price: Option<f64>,
}

#[derive(Debug, Deserialize)]
struct OpticOddsPlayer {
    name: String,
    team: Option<String>,
}

// ═══════════════════════════════════════════════════════════════
// League name → OpticOdds sport/leagues mapping
// ═══════════════════════════════════════════════════════════════

fn league_to_opticodds(league: &str) -> Option<(&'static str, &'static str)> {
    match league.to_uppercase().as_str() {
        "NFL" => Some(("football", "nfl")),
        "NBA" => Some(("basketball", "nba")),
        "MLB" => Some(("baseball", "mlb")),
        "NHL" => Some(("hockey", "nhl")),
        "WNBA" => Some(("basketball", "wnba")),
        "CBB" | "NCAAB" => Some(("basketball", "mens-college-basketball")),
        "SOCCER" | "EPL" | "MLS" => Some(("soccer", "epl")),
        "TENNIS" => Some(("tennis", "atp")),
        "MMA" | "UFC" => Some(("mma", "ufc")),
        "GOLF" | "PGA" => Some(("golf", "pga")),
        _ => None,
    }
}

// ═══════════════════════════════════════════════════════════════
// Multi-source player prop fetcher
// ═══════════════════════════════════════════════════════════════

pub struct PrizePicksFetcher {
    client: reqwest::Client,
    opticodds_key: String,
    /// League filter for fetches
    default_league: Option<String>,
}

impl PrizePicksFetcher {
    pub fn new(opticodds_key: String) -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(20))
            .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36")
            .build()
            .expect("Failed to build reqwest client");

        PrizePicksFetcher {
            client,
            opticodds_key,
            default_league: None,
        }
    }

    pub fn with_league(mut self, league: &str) -> Self {
        self.default_league = Some(league.to_string());
        self
    }

    // ── Main entry points ──

    pub async fn fetch_props(
        &mut self,
        league: Option<&str>,
        _cache_only: bool,
    ) -> Result<PropsResponse, String> {
        let league = league.or(self.default_league.as_deref());

        // Try OpticOdds first if we have a key
        if !self.opticodds_key.is_empty() {
            match self.fetch_from_opticodds(league).await {
                Ok(response) if !response.props.is_empty() => return Ok(response),
                Ok(_) => log::warn!("OpticOdds returned empty results, trying fallback"),
                Err(e) => log::warn!("OpticOdds fetch failed: {}, trying fallback", e),
            }
        }

        // Fallback: try direct PrizePicks web scrape
        match self.fetch_from_prizepicks_web(league).await {
            Ok(response) if !response.props.is_empty() => return Ok(response),
            Ok(_) => log::warn!("PrizePicks web scrape returned empty, using mock"),
            Err(e) => log::warn!("PrizePicks web scrape failed: {}, using mock", e),
        }

        // Last resort: mock data
        Ok(self.mock_props(league))
    }

    pub async fn search_props(&mut self, query: &str) -> Result<PropsResponse, String> {
        // Search across all sources
        let mut all_props = Vec::new();

        if !self.opticodds_key.is_empty() {
            if let Ok(response) = self.fetch_from_opticodds(None).await {
                for prop in response.props {
                    if prop
                        .player_name
                        .to_lowercase()
                        .contains(&query.to_lowercase())
                    {
                        all_props.push(prop);
                    }
                }
            }
        }

        if all_props.is_empty() {
            // Fallback: fetch all and filter
            let response = self.fetch_props(None, false).await?;
            for prop in response.props {
                if prop
                    .player_name
                    .to_lowercase()
                    .contains(&query.to_lowercase())
                {
                    all_props.push(prop);
                }
            }
        }

        Ok(PropsResponse {
            props: all_props,
            source: "Search".to_string(),
        })
    }

    pub async fn get_scored_props(&self) -> Result<Vec<serde_json::Value>, String> {
        // Return empty for now — scoring happens in the grading engine
        Ok(vec![])
    }

    // ── OpticOdds API ──

    async fn fetch_from_opticodds(&self, league: Option<&str>) -> Result<PropsResponse, String> {
        let base = "https://api.opticodds.com/api/v3";
        let key = &self.opticodds_key;

        // Step 1: Get active fixtures for the league
        let (sport, league_id) = match league {
            Some(l) => {
                league_to_opticodds(l).ok_or_else(|| format!("Unsupported league: {}", l))?
            }
            None => ("football", "nfl"), // default
        };

        let fixtures_url = format!(
            "{}/fixtures/active?sport={}&league={}",
            base, sport, league_id
        );

        let fixtures_resp: OpticOddsFixturesResp = self
            .client
            .get(&fixtures_url)
            .header("X-Api-Key", key)
            .send()
            .await
            .map_err(|e| format!("OpticOdds fixtures request failed: {}", e))?
            .json()
            .await
            .map_err(|e| format!("OpticOdds fixtures parse failed: {}", e))?;

        if fixtures_resp.data.is_empty() {
            return Ok(PropsResponse {
                props: vec![],
                source: "OpticOdds".to_string(),
            });
        }

        // Step 2: Get player prop odds for these fixtures from PrizePicks
        let fixture_ids: Vec<&str> = fixtures_resp
            .data
            .iter()
            .take(10) // Limit to avoid rate limits
            .map(|f| f.id.as_str())
            .collect();

        let mut all_props = Vec::new();

        for fixture_id in &fixture_ids {
            let odds_url = format!(
                "{}/fixtures/odds?fixture_id={}&sportsbook=PrizePicks",
                base, fixture_id
            );

            let odds_resp: OpticOddsOddsResp = match self
                .client
                .get(&odds_url)
                .header("X-Api-Key", key)
                .send()
                .await
            {
                Ok(resp) => match resp.json().await {
                    Ok(data) => data,
                    Err(_) => continue,
                },
                Err(_) => continue,
            };

            for entry in &odds_resp.data {
                if entry.sportsbook.as_deref() != Some("PrizePicks") {
                    continue;
                }

                let player_name = match &entry.player {
                    Some(p) => p.name.clone(),
                    None => continue,
                };

                let stat_category = entry
                    .market
                    .clone()
                    .or_else(|| entry.name.clone())
                    .unwrap_or_else(|| "Unknown".to_string());

                // Extract over/under from odds sides
                let mut over_odds_val: Option<i32> = None;
                let mut under_odds_val: Option<i32> = None;
                let line_val: f64 = 0.0;

                if let Some(sides) = &entry.odds {
                    for side in sides {
                        if let Some(name) = &side.name {
                            if let Some(price) = side.price {
                                match name.to_lowercase().as_str() {
                                    n if n.contains("over") => over_odds_val = Some(price as i32),
                                    n if n.contains("under") => under_odds_val = Some(price as i32),
                                    _ => {}
                                }
                            }
                        }
                    }
                }

                // Get team info from fixture
                let (team, opponent) = if let Some(fixture) =
                    fixtures_resp.data.iter().find(|f| f.id == *fixture_id)
                {
                    let home = fixture
                        .home_team
                        .as_ref()
                        .map(|t| t.name.clone())
                        .unwrap_or_default();
                    let away = fixture
                        .away_team
                        .as_ref()
                        .map(|t| t.name.clone())
                        .unwrap_or_default();
                    // Determine which team the player is on
                    if let Some(player) = &entry.player {
                        if let Some(player_team) = &player.team {
                            if player_team.contains(&home) || home.contains(player_team) {
                                (home, away)
                            } else {
                                (away, home)
                            }
                        } else {
                            (home, away)
                        }
                    } else {
                        (home, away)
                    }
                } else {
                    (String::new(), String::new())
                };

                all_props.push(PrizePicksProp {
                    external_id: format!("opticodds-{}", entry.fixture_id.as_deref().unwrap_or("")),
                    player_name,
                    team,
                    opponent,
                    stat_category,
                    line: line_val,
                    league: league.unwrap_or("Unknown").to_string(),
                    projection: None,
                    source: "OpticOdds".to_string(),
                    game_time: None,
                    over_odds: over_odds_val,
                    under_odds: under_odds_val,
                });
            }
        }

        Ok(PropsResponse {
            props: all_props,
            source: "OpticOdds".to_string(),
        })
    }

    // ── Direct PrizePicks web scrape ──

    async fn fetch_from_prizepicks_web(
        &self,
        league: Option<&str>,
    ) -> Result<PropsResponse, String> {
        // PrizePicks uses PerimeterX bot protection on api.prizepicks.com
        // But the web app at prizepicks.com loads data from a CDN
        // Try the public-facing web app data endpoint

        let league_path = match league {
            Some("NFL") => "nfl",
            Some("NBA") => "nba",
            Some("MLB") => "mlb",
            Some("NHL") => "nhl",
            Some("WNBA") => "wnba",
            Some(l) => {
                // Can't return a reference to a temporary, so use a static fallback
                let lower = l.to_lowercase();
                match lower.as_str() {
                    "nfl" => "nfl",
                    "nba" => "nba",
                    "mlb" => "mlb",
                    "nhl" => "nhl",
                    "wnba" => "wnba",
                    _ => "nfl",
                }
            }
            None => "nfl",
        };

        // Try the Next.js data endpoint that the web app uses
        let url = format!(
            "https://www.prizepicks.com/_next/data/projections/{}",
            league_path
        );

        let resp = self
            .client
            .get(&url)
            .header("Accept", "application/json")
            .header("Referer", "https://www.prizepicks.com/")
            .send()
            .await
            .map_err(|e| format!("PrizePicks web request failed: {}", e))?;

        if !resp.status().is_success() {
            return Err(format!("PrizePicks returned status {}", resp.status()));
        }

        let text = resp
            .text()
            .await
            .map_err(|e| format!("PrizePicks body read failed: {}", e))?;

        // Try to parse as JSON — the Next.js data endpoint returns page props
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) {
            return self.parse_prizepicks_nextjs(&json, league.unwrap_or("Unknown"));
        }

        Err("PrizePicks response is not valid JSON".to_string())
    }

    fn parse_prizepicks_nextjs(
        &self,
        json: &serde_json::Value,
        league: &str,
    ) -> Result<PropsResponse, String> {
        let mut props = Vec::new();

        // Navigate Next.js pageProps structure
        let page_props = json
            .get("pageProps")
            .or_else(|| json.get("props").and_then(|p| p.get("pageProps")))
            .ok_or("No pageProps in Next.js response")?;

        // Look for projections data in various possible locations
        let projections_data = page_props
            .get("projections")
            .or_else(|| page_props.get("data"))
            .or_else(|| page_props.get("initialState"))
            .or_else(|| page_props.get("dehydratedState"));

        if let Some(data) = projections_data {
            // Try to extract from a projections array
            if let Some(arr) = data.as_array() {
                for item in arr {
                    if let Some(prop) = self.parse_single_projection(item, league) {
                        props.push(prop);
                    }
                }
            } else if let Some(data_inner) = data.get("data").and_then(|d| d.as_array()) {
                for item in data_inner {
                    if let Some(prop) = self.parse_single_projection(item, league) {
                        props.push(prop);
                    }
                }
            }
        }

        Ok(PropsResponse {
            props,
            source: "PrizePicks-Web".to_string(),
        })
    }

    fn parse_single_projection(
        &self,
        item: &serde_json::Value,
        league: &str,
    ) -> Option<PrizePicksProp> {
        // PrizePicks projection JSON structure (from their web app):
        // { "id": "...", "type": "projection", "attributes": { "player_name": "...", ... } }
        // OR flat: { "player_name": "...", "line": ..., ... }

        let attrs = item
            .get("attributes")
            .or_else(|| item.get("data"))
            .unwrap_or(item);

        let player_name = attrs
            .get("player_name")
            .or_else(|| attrs.get("name"))
            .or_else(|| attrs.get("player"))
            .and_then(|v| v.as_str())?;

        let stat_category = attrs
            .get("stat_type")
            .or_else(|| attrs.get("stat"))
            .or_else(|| attrs.get("market"))
            .and_then(|v| v.as_str())
            .unwrap_or("Unknown");

        let line = attrs
            .get("line_score")
            .or_else(|| attrs.get("line"))
            .or_else(|| attrs.get("projection"))
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);

        let team = attrs
            .get("team_name")
            .or_else(|| attrs.get("team"))
            .and_then(|v| v.as_str())
            .unwrap_or("");

        let opponent = attrs
            .get("opponent_name")
            .or_else(|| attrs.get("opponent"))
            .and_then(|v| v.as_str())
            .unwrap_or("");

        let external_id = item
            .get("id")
            .or_else(|| attrs.get("id"))
            .and_then(|v| v.as_str())
            .unwrap_or("");

        Some(PrizePicksProp {
            external_id: format!("pp-{}", external_id),
            player_name: player_name.to_string(),
            team: team.to_string(),
            opponent: opponent.to_string(),
            stat_category: stat_category.to_string(),
            line,
            league: league.to_string(),
            projection: Some(line),
            source: "PrizePicks-Web".to_string(),
            game_time: None,
            over_odds: None,
            under_odds: None,
        })
    }

    // ── Mock data fallback ──

    fn mock_props(&self, league: Option<&str>) -> PropsResponse {
        let league = league.unwrap_or("NFL");
        let now = chrono::Utc::now();
        let game_time = (now + chrono::Duration::hours(3)).to_rfc3339();

        let mock_props = match league.to_uppercase().as_str() {
            "NFL" => vec![
                PrizePicksProp {
                    external_id: "mock-nfl-1".into(),
                    player_name: "Patrick Mahomes".into(),
                    team: "Kansas City Chiefs".into(),
                    opponent: "Buffalo Bills".into(),
                    stat_category: "Passing Yards".into(),
                    line: 275.5,
                    league: "NFL".into(),
                    projection: Some(282.0),
                    source: "Mock".into(),
                    game_time: Some(game_time.clone()),
                    over_odds: Some(-110),
                    under_odds: Some(-110),
                },
                PrizePicksProp {
                    external_id: "mock-nfl-2".into(),
                    player_name: "Josh Allen".into(),
                    team: "Buffalo Bills".into(),
                    opponent: "Kansas City Chiefs".into(),
                    stat_category: "Passing Yards".into(),
                    line: 265.5,
                    league: "NFL".into(),
                    projection: Some(258.0),
                    source: "Mock".into(),
                    game_time: Some(game_time.clone()),
                    over_odds: Some(-110),
                    under_odds: Some(-110),
                },
                PrizePicksProp {
                    external_id: "mock-nfl-3".into(),
                    player_name: "Travis Kelce".into(),
                    team: "Kansas City Chiefs".into(),
                    opponent: "Buffalo Bills".into(),
                    stat_category: "Receiving Yards".into(),
                    line: 72.5,
                    league: "NFL".into(),
                    projection: Some(68.0),
                    source: "Mock".into(),
                    game_time: Some(game_time.clone()),
                    over_odds: Some(-115),
                    under_odds: Some(-105),
                },
                PrizePicksProp {
                    external_id: "mock-nfl-4".into(),
                    player_name: "Stefon Diggs".into(),
                    team: "Buffalo Bills".into(),
                    opponent: "Kansas City Chiefs".into(),
                    stat_category: "Receptions".into(),
                    line: 6.5,
                    league: "NFL".into(),
                    projection: Some(7.2),
                    source: "Mock".into(),
                    game_time: Some(game_time.clone()),
                    over_odds: Some(-120),
                    under_odds: Some(even_money()),
                },
                PrizePicksProp {
                    external_id: "mock-nfl-5".into(),
                    player_name: "Isiah Pacheco".into(),
                    team: "Kansas City Chiefs".into(),
                    opponent: "Buffalo Bills".into(),
                    stat_category: "Rushing Yards".into(),
                    line: 58.5,
                    league: "NFL".into(),
                    projection: Some(62.0),
                    source: "Mock".into(),
                    game_time: Some(game_time.clone()),
                    over_odds: Some(-110),
                    under_odds: Some(-110),
                },
            ],
            "NBA" => vec![
                PrizePicksProp {
                    external_id: "mock-nba-1".into(),
                    player_name: "LeBron James".into(),
                    team: "Los Angeles Lakers".into(),
                    opponent: "Boston Celtics".into(),
                    stat_category: "Points".into(),
                    line: 24.5,
                    league: "NBA".into(),
                    projection: Some(26.0),
                    source: "Mock".into(),
                    game_time: Some(game_time.clone()),
                    over_odds: Some(-110),
                    under_odds: Some(-110),
                },
                PrizePicksProp {
                    external_id: "mock-nba-2".into(),
                    player_name: "Jayson Tatum".into(),
                    team: "Boston Celtics".into(),
                    opponent: "Los Angeles Lakers".into(),
                    stat_category: "PRA".into(),
                    line: 38.5,
                    league: "NBA".into(),
                    projection: Some(36.0),
                    source: "Mock".into(),
                    game_time: Some(game_time.clone()),
                    over_odds: Some(-105),
                    under_odds: Some(-115),
                },
                PrizePicksProp {
                    external_id: "mock-nba-3".into(),
                    player_name: "Anthony Davis".into(),
                    team: "Los Angeles Lakers".into(),
                    opponent: "Boston Celtics".into(),
                    stat_category: "Rebounds".into(),
                    line: 11.5,
                    league: "NBA".into(),
                    projection: Some(12.0),
                    source: "Mock".into(),
                    game_time: Some(game_time.clone()),
                    over_odds: Some(-110),
                    under_odds: Some(-110),
                },
            ],
            _ => vec![PrizePicksProp {
                external_id: "mock-generic-1".into(),
                player_name: "Sample Player".into(),
                team: "Team A".into(),
                opponent: "Team B".into(),
                stat_category: "Points".into(),
                line: 20.5,
                league: league.into(),
                projection: Some(21.0),
                source: "Mock".into(),
                game_time: Some(game_time),
                over_odds: Some(-110),
                under_odds: Some(-110),
            }],
        };

        PropsResponse {
            props: mock_props,
            source: "Mock".into(),
        }
    }
}

fn even_money() -> i32 {
    100
}
