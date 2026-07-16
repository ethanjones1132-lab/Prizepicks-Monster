use serde::{Deserialize, Serialize};

// ═══════════════════════════════════════════════════════════════
// PrizePicks Player Prop Data Models
// Multi-source: OpticOdds → The Odds API → ESPN → Sleeper → Mock
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
// The Odds API Types
// ═══════════════════════════════════════════════════════════════

#[derive(Debug, Deserialize)]
struct OddsApiResponse {
    data: Vec<OddsApiSport>,
}

#[derive(Debug, Deserialize)]
struct OddsApiSport {
    key: String,
    group: String,
    title: String,
    description: String,
    active: bool,
    has_outrights: bool,
}

#[derive(Debug, Deserialize)]
struct OddsApiOddsResponse {
    // The Odds API returns an array at the top level for /v4/sports/{sport}/odds
    // Each element: { id, sport_key, sport_title, commence_time, home_team, away_team, bookmakers }
}

#[derive(Debug, Deserialize)]
struct OddsApiOutcome {
    name: Option<String>,
    price: Option<f64>,
    point: Option<f64>,
    description: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OddsApiMarket {
    key: Option<String>,
    last_update: Option<String>,
    outcomes: Option<Vec<OddsApiOutcome>>,
}

#[derive(Debug, Deserialize)]
struct OddsApiBookmaker {
    key: Option<String>,
    title: Option<String>,
    last_update: Option<String>,
    markets: Option<Vec<OddsApiMarket>>,
}

// ═══════════════════════════════════════════════════════════════
// ESPN API Types
// ═══════════════════════════════════════════════════════════════

#[derive(Debug, Deserialize)]
struct EspnScoreboard {
    events: Vec<EspnEvent>,
}

#[derive(Debug, Deserialize)]
struct EspnEvent {
    id: String,
    name: Option<String>,
    short_name: Option<String>,
    date: Option<String>,
    status: Option<EspnStatus>,
    competitions: Vec<EspnCompetition>,
}

#[derive(Debug, Deserialize)]
struct EspnStatus {
    #[serde(rename = "type")]
    type_: Option<EspnStatusType>,
}

#[derive(Debug, Deserialize)]
struct EspnStatusType {
    name: Option<String>,
    description: Option<String>,
}

#[derive(Debug, Deserialize)]
struct EspnCompetition {
    id: Option<String>,
    competitors: Vec<EspnCompetitor>,
    start_date: Option<String>,
}

#[derive(Debug, Deserialize)]
struct EspnCompetitor {
    id: Option<String>,
    home_away: Option<String>,
    team: Option<EspnTeam>,
    score: Option<String>,
}

#[derive(Debug, Deserialize)]
struct EspnTeam {
    id: Option<String>,
    abbreviation: Option<String>,
    display_name: Option<String>,
    name: Option<String>,
}

// ═══════════════════════════════════════════════════════════════
// Sleeper API Types
// ═══════════════════════════════════════════════════════════════

#[derive(Debug, Deserialize)]
struct SleeperNflState {
    season: Option<String>,
    week: Option<u32>,
    season_type: Option<String>,
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

// League name → The Odds API sport key
fn league_to_odds_api(league: &str) -> Option<&'static str> {
    match league.to_uppercase().as_str() {
        "NFL" => Some("americanfootball_nfl"),
        "NBA" => Some("basketball_nba"),
        "MLB" => Some("baseball_mlb"),
        "NHL" => Some("icehockey_nhl"),
        "WNBA" => Some("basketball_wnba"),
        "SOCCER" | "EPL" | "MLS" => Some("soccer_epl"),
        _ => None,
    }
}

// ═══════════════════════════════════════════════════════════════
// Multi-source player prop fetcher
// ═══════════════════════════════════════════════════════════════

pub struct PrizePicksFetcher {
    client: reqwest::Client,
    opticodds_key: String,
    odds_api_key: String,
    /// League filter for fetches
    default_league: Option<String>,
}

impl PrizePicksFetcher {
    pub fn new(opticodds_key: String, odds_api_key: String) -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(20))
            .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36")
            .build()
            .expect("Failed to build reqwest client");

        PrizePicksFetcher {
            client,
            opticodds_key,
            odds_api_key,
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

        // 1. Try OpticOdds first if we have a key
        if !self.opticodds_key.is_empty() {
            match self.fetch_from_opticodds(league).await {
                Ok(response) if !response.props.is_empty() => return Ok(response),
                Ok(_) => log::warn!("OpticOdds returned empty results, trying fallback"),
                Err(e) => log::warn!("OpticOdds fetch failed: {}, trying fallback", e),
            }
        }

        // 2. Try The Odds API if we have a key
        if !self.odds_api_key.is_empty() {
            match self.fetch_from_odds_api(league).await {
                Ok(response) if !response.props.is_empty() => return Ok(response),
                Ok(_) => log::warn!("The Odds API returned empty results, trying fallback"),
                Err(e) => log::warn!("The Odds API fetch failed: {}, trying fallback", e),
            }
        }

        // 3. Try ESPN for real game/player data
        match self.fetch_from_espn(league).await {
            Ok(response) if !response.props.is_empty() => return Ok(response),
            Ok(_) => log::warn!("ESPN returned empty results, trying Sleeper"),
            Err(e) => log::warn!("ESPN fetch failed: {}, trying Sleeper", e),
        }

        // 4. Try Sleeper for real player data
        match self.fetch_from_sleeper(league).await {
            Ok(response) if !response.props.is_empty() => return Ok(response),
            Ok(_) => log::warn!("Sleeper returned empty results, using mock"),
            Err(e) => log::warn!("Sleeper fetch failed: {}, using mock", e),
        }

        // Last resort: mock data
        Ok(self.mock_props(league))
    }

    pub async fn search_props(&mut self, query: &str) -> Result<PropsResponse, String> {
        let lower_query = query.to_lowercase();
        let mut all_props = Vec::new();

        // Search across all sources
        if !self.opticodds_key.is_empty() {
            if let Ok(response) = self.fetch_from_opticodds(None).await {
                for prop in response.props {
                    if prop.player_name.to_lowercase().contains(&lower_query) {
                        all_props.push(prop);
                    }
                }
            }
        }

        if all_props.is_empty() && !self.odds_api_key.is_empty() {
            if let Ok(response) = self.fetch_from_odds_api(None).await {
                for prop in response.props {
                    if prop.player_name.to_lowercase().contains(&lower_query) {
                        all_props.push(prop);
                    }
                }
            }
        }

        if all_props.is_empty() {
            // Fallback: fetch all and filter
            let response = self.fetch_props(None, false).await?;
            for prop in response.props {
                if prop.player_name.to_lowercase().contains(&lower_query) {
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

    // ── The Odds API ──

    async fn fetch_from_odds_api(&self, league: Option<&str>) -> Result<PropsResponse, String> {
        let key = &self.odds_api_key;
        let league = league.unwrap_or("NFL");

        let sport_key = league_to_odds_api(league)
            .ok_or_else(|| format!("Unsupported league for The Odds API: {}", league))?;

        // Fetch upcoming odds for the sport
        // The Odds API free tier: /v4/sports/{sport}/odds?regions=us&markets=h2h,spreads,totals
        let url = format!(
            "https://api.the-odds-api.com/v4/sports/{}/odds/?apiKey={}&regions=us&markets=player_points,player_rebounds,player_assists,player_threes,player_steals,player_blocks,player_turnovers&oddsFormat=american",
            sport_key, key
        );

        let resp = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| format!("The Odds API request failed: {}", e))?;

        if !resp.status().is_success() {
            return Err(format!("The Odds API returned status {}", resp.status()));
        }

        let text = resp
            .text()
            .await
            .map_err(|e| format!("The Odds API body read failed: {}", e))?;

        // The Odds API returns an array at the top level
        let games: Vec<serde_json::Value> = match serde_json::from_str(&text) {
            Ok(v) => v,
            Err(_) => return Ok(PropsResponse { props: vec![], source: "TheOddsAPI".to_string() }),
        };

        let mut all_props = Vec::new();

        for game in &games {
            let home_team = game.get("home_team").and_then(|v| v.as_str()).unwrap_or("");
            let away_team = game.get("away_team").and_then(|v| v.as_str()).unwrap_or("");
            let commence_time = game.get("commence_time").and_then(|v| v.as_str());

            let bookmakers = match game.get("bookmakers").and_then(|v| v.as_array()) {
                Some(b) => b,
                None => continue,
            };

            for bookmaker in bookmakers {
                let markets = match bookmaker.get("markets").and_then(|v| v.as_array()) {
                    Some(m) => m,
                    None => continue,
                };

                for market in markets {
                    let market_key = market
                        .get("key")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    let outcomes = match market.get("outcomes").and_then(|v| v.as_array()) {
                        Some(o) => o,
                        None => continue,
                    };

                    for outcome in outcomes {
                        let player_name = outcome
                            .get("description")
                            .or_else(|| outcome.get("name"))
                            .and_then(|v| v.as_str())
                            .unwrap_or("");

                        if player_name.is_empty() {
                            continue;
                        }

                        let stat_category = self.odds_api_market_to_stat(market_key);
                        let point = outcome.get("point").and_then(|v| v.as_f64()).unwrap_or(0.0);

                        if point <= 0.0 {
                            continue;
                        }

                        all_props.push(PrizePicksProp {
                            external_id: format!(
                                "oddsapi-{}-{}",
                                game.get("id").and_then(|v| v.as_str()).unwrap_or(""),
                                player_name
                            ),
                            player_name: player_name.to_string(),
                            team: home_team.to_string(),
                            opponent: away_team.to_string(),
                            stat_category,
                            line: point,
                            league: league.to_string(),
                            projection: None,
                            source: "TheOddsAPI".to_string(),
                            game_time: commence_time.map(|s| s.to_string()),
                            over_odds: None,
                            under_odds: None,
                        });
                    }
                }
            }
        }

        Ok(PropsResponse {
            props: all_props,
            source: "TheOddsAPI".to_string(),
        })
    }

    fn odds_api_market_to_stat(&self, market_key: &str) -> String {
        match market_key {
            "player_points" => "Points".to_string(),
            "player_rebounds" => "Rebounds".to_string(),
            "player_assists" => "Assists".to_string(),
            "player_threes" => "3-Pointers Made".to_string(),
            "player_steals" => "Steals".to_string(),
            "player_blocks" => "Blocks".to_string(),
            "player_turnovers" => "Turnovers".to_string(),
            "player_points_rebounds_assists" => "PRA".to_string(),
            "player_points_rebounds" => "Points + Rebounds".to_string(),
            "player_points_assists" => "Points + Assists".to_string(),
            "player_rebounds_assists" => "Rebounds + Assists".to_string(),
            "player_pitching_strikeouts" => "Strikeouts".to_string(),
            "player_pitching_outs" => "Outs Recorded".to_string(),
            "player_home_runs" => "Home Runs".to_string(),
            "player_hits" => "Hits".to_string(),
            "player_rbis" => "RBIs".to_string(),
            "player_runs" => "Runs".to_string(),
            "player_bases" => "Total Bases".to_string(),
            "player_pass_touchdowns" => "Passing Touchdowns".to_string(),
            "player_pass_yards" => "Passing Yards".to_string(),
            "player_pass_interceptions" => "Interceptions".to_string(),
            "player_pass_completions" => "Completions".to_string(),
            "player_rush_yards" => "Rushing Yards".to_string(),
            "player_rush_attempts" => "Rushing Attempts".to_string(),
            "player_reception_yards" => "Receiving Yards".to_string(),
            "player_receptions" => "Receptions".to_string(),
            "player_first_touchdown" => "First Touchdown Scored".to_string(),
            "player_field_goals" => "Field Goals Made".to_string(),
            "player_kicking_points" => "Kicking Points".to_string(),
            _ => market_key.replace("player_", "").replace("_", " "),
        }
    }

    // ── ESPN API ──

    async fn fetch_from_espn(&self, league: Option<&str>) -> Result<PropsResponse, String> {
        let league = league.unwrap_or("NFL");

        // ESPN has different API paths per sport
        let espn_sport = match league.to_uppercase().as_str() {
            "NFL" => "football/nfl",
            "NBA" => "basketball/nba",
            "MLB" => "baseball/mlb",
            "NHL" => "hockey/nhl",
            _ => return Err(format!("Unsupported league for ESPN: {}", league)),
        };

        let url = format!(
            "https://site.api.espn.com/apis/site/v2/sports/{}/scoreboard",
            espn_sport
        );

        let resp = self
            .client
            .get(&url)
            .header("Accept", "application/json")
            .send()
            .await
            .map_err(|e| format!("ESPN request failed: {}", e))?;

        if !resp.status().is_success() {
            return Err(format!("ESPN returned status {}", resp.status()));
        }

        let text = resp
            .text()
            .await
            .map_err(|e| format!("ESPN body read failed: {}", e))?;

        let scoreboard: EspnScoreboard = match serde_json::from_str(&text) {
            Ok(s) => s,
            Err(_) => return Ok(PropsResponse { props: vec![], source: "ESPN".to_string() }),
        };

        let mut all_props = Vec::new();

        for event in &scoreboard.events {
            let game_time = event.date.clone();

            for competition in &event.competitions {
                let home_team = competition
                    .competitors
                    .iter()
                    .find(|c| c.home_away.as_deref() == Some("home"))
                    .and_then(|c| c.team.as_ref())
                    .map(|t| t.display_name.as_deref().unwrap_or("").to_string())
                    .unwrap_or_default();

                let away_team = competition
                    .competitors
                    .iter()
                    .find(|c| c.home_away.as_deref() == Some("away"))
                    .and_then(|c| c.team.as_ref())
                    .map(|t| t.display_name.as_deref().unwrap_or("").to_string())
                    .unwrap_or_default();

                let home_abbr = competition
                    .competitors
                    .iter()
                    .find(|c| c.home_away.as_deref() == Some("home"))
                    .and_then(|c| c.team.as_ref())
                    .and_then(|t| t.abbreviation.clone());

                let away_abbr = competition
                    .competitors
                    .iter()
                    .find(|c| c.home_away.as_deref() == Some("away"))
                    .and_then(|c| c.team.as_ref())
                    .and_then(|t| t.abbreviation.clone());

                if home_team.is_empty() || away_team.is_empty() {
                    continue;
                }

                // Create props for notable stat categories
                // ESPN doesn't expose player prop lines, but we create "enriched" props
                // that show the game context with mock lines — better than fully fake data
                let stat_categories = match league.to_uppercase().as_str() {
                    "NFL" => vec!["Passing Yards", "Rushing Yards", "Receiving Yards", "Receptions", "Touchdowns"],
                    "NBA" => vec!["Points", "Rebounds", "Assists", "PRA", "Three Pointers"],
                    "MLB" => vec!["Hits", "RBIs", "Home Runs", "Strikeouts"],
                    "NHL" => vec!["Goals", "Assists", "Points", "Shots on Goal"],
                    _ => vec!["Points"],
                };

                let home_team_for_props = home_team.clone();
                let away_team_for_props = away_team.clone();
                let home_abbr_clone = home_abbr.clone();
                let away_abbr_clone = away_abbr.clone();

                // Create props for some generic key players based on the matchup
                // Since ESPN scoreboard doesn't give us player-specific data per game,
                // we need to generate contextualized props
                let generic_props = vec![
                    PrizePicksProp {
                        external_id: format!("espn-{}-home-passer", event.id),
                        player_name: format!("{} QB", if home_abbr_clone.is_some() { home_abbr_clone.unwrap_or_default() } else { home_team_for_props.clone() }),
                        team: home_team_for_props.clone(),
                        opponent: away_team_for_props.clone(),
                        stat_category: stat_categories[0].to_string(),
                        line: 0.0,
                        league: league.to_string(),
                        projection: None,
                        source: "ESPN".to_string(),
                        game_time: game_time.clone(),
                        over_odds: None,
                        under_odds: None,
                    },
                    PrizePicksProp {
                        external_id: format!("espn-{}-away-passer", event.id),
                        player_name: format!("{} QB", if away_abbr_clone.is_some() { away_abbr_clone.unwrap_or_default() } else { away_team_for_props.clone() }),
                        team: away_team_for_props.clone(),
                        opponent: home_team_for_props.clone(),
                        stat_category: stat_categories[0].to_string(),
                        line: 0.0,
                        league: league.to_string(),
                        projection: None,
                        source: "ESPN".to_string(),
                        game_time: game_time.clone(),
                        over_odds: None,
                        under_odds: None,
                    },
                ];

                all_props.extend(generic_props);
            }
        }

        Ok(PropsResponse {
            props: all_props,
            source: "ESPN".to_string(),
        })
    }

    // ── Sleeper API ──

    async fn fetch_from_sleeper(&self, league: Option<&str>) -> Result<PropsResponse, String> {
        let league = league.unwrap_or("NFL");

        // Sleeper only has comprehensive NFL player data
        if league.to_uppercase().as_str() != "NFL" {
            // For non-NFL leagues, return empty and fall through to mock
            return Ok(PropsResponse { props: vec![], source: "Sleeper".to_string() });
        }

        // Get NFL state for season context
        let state_url = "https://api.sleeper.app/v1/state/nfl";
        let state_resp = self
            .client
            .get(state_url)
            .send()
            .await
            .map_err(|e| format!("Sleeper state request failed: {}", e))?;

        let _season: String = if state_resp.status().is_success() {
            if let Ok(text) = state_resp.text().await {
                if let Ok(state) = serde_json::from_str::<SleeperNflState>(&text) {
                    state.season.unwrap_or_else(|| "2026".to_string())
                } else {
                    "2026".to_string()
                }
            } else {
                "2026".to_string()
            }
        } else {
            "2026".to_string()
        };

        // Get players (this is a large endpoint, use a timeout)
        let players_url = "https://api.sleeper.app/v1/players/nfl";

        let resp = self
            .client
            .get(players_url)
            .send()
            .await
            .map_err(|e| format!("Sleeper players request failed: {}", e))?;

        if !resp.status().is_success() {
            return Err(format!("Sleeper returned status {}", resp.status()));
        }

        let text = resp
            .text()
            .await
            .map_err(|e| format!("Sleeper body read failed: {}", e))?;

        // Sleeper returns a map of player_id -> player object
        // We'll extract some notable players to create enriched props
        let players_map: serde_json::Value = match serde_json::from_str(&text) {
            Ok(v) => v,
            Err(_) => return Ok(PropsResponse { props: vec![], source: "Sleeper".to_string() }),
        };

        let all_players = match players_map.as_object() {
            Some(obj) => obj,
            None => return Ok(PropsResponse { props: vec![], source: "Sleeper".to_string() }),
        };

        // Pick top players by position (QB, RB, WR, TE) grouped by team
        let mut top_players: Vec<(String, String, String, String)> = Vec::new(); // (name, team, position, status)

        for (_player_id, player_info) in all_players.iter().take(500) {
            let name = player_info
                .get("full_name")
                .or_else(|| player_info.get("name"))
                .and_then(|v| v.as_str())
                .unwrap_or("");

            if name.is_empty() {
                continue;
            }

            let team = player_info
                .get("team")
                .and_then(|v| v.as_str())
                .unwrap_or("FA")
                .to_string();

            let position = player_info
                .get("position")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            let status = player_info
                .get("status")
                .and_then(|v| v.as_str())
                .unwrap_or("Active")
                .to_string();

            // Only include active NFL players with a team and major position
            if status == "Active" && !team.is_empty() && team != "FA" {
                match position.as_str() {
                    "QB" | "RB" | "WR" | "TE" => {
                        top_players.push((name.to_string(), team, position, status));
                    }
                    _ => {}
                }
            }

            // Limit to keep things manageable
            if top_players.len() >= 40 {
                break;
            }
        }

        let mut all_props = Vec::new();
        let now = chrono::Utc::now();
        let default_game_time = (now + chrono::Duration::hours(72)).to_rfc3339();

        for (player_name, team, position, _status) in &top_players {
            // Create meaningful props based on position
            let (stat_category, line) = match position.as_str() {
                "QB" => ("Passing Yards", 225.0),
                "RB" => ("Rushing Yards", 55.0),
                "WR" => ("Receiving Yards", 45.0),
                "TE" => ("Receiving Yards", 30.0),
                _ => continue,
            };

            all_props.push(PrizePicksProp {
                external_id: format!("sleeper-{}-{}", team, player_name.replace(' ', "-")),
                player_name: player_name.to_string(),
                team: team.to_string(),
                opponent: "TBD".to_string(), // Sleeper doesn't give opponent in player endpoint
                stat_category: stat_category.to_string(),
                line,
                league: league.to_string(),
                projection: None,
                source: "Sleeper".to_string(),
                game_time: Some(default_game_time.clone()),
                over_odds: None,
                under_odds: None,
            });
        }

        Ok(PropsResponse {
            props: all_props,
            source: "Sleeper".to_string(),
        })
    }

    // ── Direct PrizePicks web scrape (legacy, kept for reference) ──

    async fn fetch_from_prizepicks_web(
        &self,
        league: Option<&str>,
    ) -> Result<PropsResponse, String> {
        let league_path = match league {
            Some("NFL") => "nfl",
            Some("NBA") => "nba",
            Some("MLB") => "mlb",
            Some("NHL") => "nhl",
            Some("WNBA") => "wnba",
            Some(l) => {
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
        let gt = |h| (now + chrono::Duration::hours(h)).to_rfc3339();

        let mock_props = match league.to_uppercase().as_str() {
            "NFL" => vec![
                // KC @ BUF
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
                    game_time: Some(gt(3)),
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
                    game_time: Some(gt(3)),
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
                    game_time: Some(gt(3)),
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
                    game_time: Some(gt(3)),
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
                    game_time: Some(gt(3)),
                    over_odds: Some(-110),
                    under_odds: Some(-110),
                },
                // SF @ DAL
                PrizePicksProp {
                    external_id: "mock-nfl-6".into(),
                    player_name: "Brock Purdy".into(),
                    team: "San Francisco 49ers".into(),
                    opponent: "Dallas Cowboys".into(),
                    stat_category: "Passing Yards".into(),
                    line: 245.5,
                    league: "NFL".into(),
                    projection: Some(232.0),
                    source: "Mock".into(),
                    game_time: Some(gt(3)),
                    over_odds: Some(-110),
                    under_odds: Some(-110),
                },
                PrizePicksProp {
                    external_id: "mock-nfl-7".into(),
                    player_name: "Christian McCaffrey".into(),
                    team: "San Francisco 49ers".into(),
                    opponent: "Dallas Cowboys".into(),
                    stat_category: "Rushing Yards".into(),
                    line: 82.5,
                    league: "NFL".into(),
                    projection: Some(91.0),
                    source: "Mock".into(),
                    game_time: Some(gt(3)),
                    over_odds: Some(-115),
                    under_odds: Some(-105),
                },
                PrizePicksProp {
                    external_id: "mock-nfl-8".into(),
                    player_name: "CeeDee Lamb".into(),
                    team: "Dallas Cowboys".into(),
                    opponent: "San Francisco 49ers".into(),
                    stat_category: "Receiving Yards".into(),
                    line: 88.5,
                    league: "NFL".into(),
                    projection: Some(95.0),
                    source: "Mock".into(),
                    game_time: Some(gt(3)),
                    over_odds: Some(-110),
                    under_odds: Some(-110),
                },
                PrizePicksProp {
                    external_id: "mock-nfl-9".into(),
                    player_name: "George Kittle".into(),
                    team: "San Francisco 49ers".into(),
                    opponent: "Dallas Cowboys".into(),
                    stat_category: "Receiving Yards".into(),
                    line: 55.5,
                    league: "NFL".into(),
                    projection: Some(48.0),
                    source: "Mock".into(),
                    game_time: Some(gt(3)),
                    over_odds: Some(-110),
                    under_odds: Some(even_money()),
                },
                // CIN @ BAL
                PrizePicksProp {
                    external_id: "mock-nfl-10".into(),
                    player_name: "Joe Burrow".into(),
                    team: "Cincinnati Bengals".into(),
                    opponent: "Baltimore Ravens".into(),
                    stat_category: "Passing Yards".into(),
                    line: 285.5,
                    league: "NFL".into(),
                    projection: Some(301.0),
                    source: "Mock".into(),
                    game_time: Some(gt(4)),
                    over_odds: Some(-115),
                    under_odds: Some(-105),
                },
                PrizePicksProp {
                    external_id: "mock-nfl-11".into(),
                    player_name: "Ja'Marr Chase".into(),
                    team: "Cincinnati Bengals".into(),
                    opponent: "Baltimore Ravens".into(),
                    stat_category: "Receiving Yards".into(),
                    line: 82.5,
                    league: "NFL".into(),
                    projection: Some(88.0),
                    source: "Mock".into(),
                    game_time: Some(gt(4)),
                    over_odds: Some(-110),
                    under_odds: Some(-110),
                },
                PrizePicksProp {
                    external_id: "mock-nfl-12".into(),
                    player_name: "Lamar Jackson".into(),
                    team: "Baltimore Ravens".into(),
                    opponent: "Cincinnati Bengals".into(),
                    stat_category: "Rushing Yards".into(),
                    line: 52.5,
                    league: "NFL".into(),
                    projection: Some(58.0),
                    source: "Mock".into(),
                    game_time: Some(gt(4)),
                    over_odds: Some(-105),
                    under_odds: Some(-115),
                },
                PrizePicksProp {
                    external_id: "mock-nfl-13".into(),
                    player_name: "Mark Andrews".into(),
                    team: "Baltimore Ravens".into(),
                    opponent: "Cincinnati Bengals".into(),
                    stat_category: "Receiving Yards".into(),
                    line: 48.5,
                    league: "NFL".into(),
                    projection: Some(44.0),
                    source: "Mock".into(),
                    game_time: Some(gt(4)),
                    over_odds: Some(even_money()),
                    under_odds: Some(-120),
                },
            ],
            "NBA" => vec![
                // LAL @ BOS
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
                    game_time: Some(gt(3)),
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
                    game_time: Some(gt(3)),
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
                    game_time: Some(gt(3)),
                    over_odds: Some(-110),
                    under_odds: Some(-110),
                },
                // MIL @ PHI
                PrizePicksProp {
                    external_id: "mock-nba-4".into(),
                    player_name: "Giannis Antetokounmpo".into(),
                    team: "Milwaukee Bucks".into(),
                    opponent: "Philadelphia 76ers".into(),
                    stat_category: "Points".into(),
                    line: 30.5,
                    league: "NBA".into(),
                    projection: Some(33.0),
                    source: "Mock".into(),
                    game_time: Some(gt(3)),
                    over_odds: Some(-115),
                    under_odds: Some(-105),
                },
                PrizePicksProp {
                    external_id: "mock-nba-5".into(),
                    player_name: "Joel Embiid".into(),
                    team: "Philadelphia 76ers".into(),
                    opponent: "Milwaukee Bucks".into(),
                    stat_category: "Points".into(),
                    line: 28.5,
                    league: "NBA".into(),
                    projection: Some(27.0),
                    source: "Mock".into(),
                    game_time: Some(gt(3)),
                    over_odds: Some(-110),
                    under_odds: Some(-110),
                },
                PrizePicksProp {
                    external_id: "mock-nba-6".into(),
                    player_name: "Damian Lillard".into(),
                    team: "Milwaukee Bucks".into(),
                    opponent: "Philadelphia 76ers".into(),
                    stat_category: "Assists".into(),
                    line: 6.5,
                    league: "NBA".into(),
                    projection: Some(7.5),
                    source: "Mock".into(),
                    game_time: Some(gt(3)),
                    over_odds: Some(-110),
                    under_odds: Some(-110),
                },
                PrizePicksProp {
                    external_id: "mock-nba-7".into(),
                    player_name: "Tyrese Maxey".into(),
                    team: "Philadelphia 76ers".into(),
                    opponent: "Milwaukee Bucks".into(),
                    stat_category: "Points".into(),
                    line: 22.5,
                    league: "NBA".into(),
                    projection: Some(20.0),
                    source: "Mock".into(),
                    game_time: Some(gt(3)),
                    over_odds: Some(-110),
                    under_odds: Some(-110),
                },
                // DEN @ OKC
                PrizePicksProp {
                    external_id: "mock-nba-8".into(),
                    player_name: "Nikola Jokic".into(),
                    team: "Denver Nuggets".into(),
                    opponent: "Oklahoma City Thunder".into(),
                    stat_category: "PRA".into(),
                    line: 45.5,
                    league: "NBA".into(),
                    projection: Some(48.0),
                    source: "Mock".into(),
                    game_time: Some(gt(4)),
                    over_odds: Some(-120),
                    under_odds: Some(even_money()),
                },
                PrizePicksProp {
                    external_id: "mock-nba-9".into(),
                    player_name: "Shai Gilgeous-Alexander".into(),
                    team: "Oklahoma City Thunder".into(),
                    opponent: "Denver Nuggets".into(),
                    stat_category: "Points".into(),
                    line: 30.5,
                    league: "NBA".into(),
                    projection: Some(32.0),
                    source: "Mock".into(),
                    game_time: Some(gt(4)),
                    over_odds: Some(-110),
                    under_odds: Some(-110),
                },
                PrizePicksProp {
                    external_id: "mock-nba-10".into(),
                    player_name: "Jamal Murray".into(),
                    team: "Denver Nuggets".into(),
                    opponent: "Oklahoma City Thunder".into(),
                    stat_category: "Points".into(),
                    line: 20.5,
                    league: "NBA".into(),
                    projection: Some(18.0),
                    source: "Mock".into(),
                    game_time: Some(gt(4)),
                    over_odds: Some(-105),
                    under_odds: Some(-115),
                },
            ],
            "MLB" => vec![
                // LAD @ NYY
                PrizePicksProp {
                    external_id: "mock-mlb-1".into(),
                    player_name: "Shohei Ohtani".into(),
                    team: "Los Angeles Dodgers".into(),
                    opponent: "New York Yankees".into(),
                    stat_category: "Total Bases".into(),
                    line: 1.5,
                    league: "MLB".into(),
                    projection: Some(2.2),
                    source: "Mock".into(),
                    game_time: Some(gt(3)),
                    over_odds: Some(-115),
                    under_odds: Some(-105),
                },
                PrizePicksProp {
                    external_id: "mock-mlb-2".into(),
                    player_name: "Aaron Judge".into(),
                    team: "New York Yankees".into(),
                    opponent: "Los Angeles Dodgers".into(),
                    stat_category: "Home Runs".into(),
                    line: 0.5,
                    league: "MLB".into(),
                    projection: Some(0.6),
                    source: "Mock".into(),
                    game_time: Some(gt(3)),
                    over_odds: Some(140),
                    under_odds: Some(-170),
                },
                PrizePicksProp {
                    external_id: "mock-mlb-3".into(),
                    player_name: "Mookie Betts".into(),
                    team: "Los Angeles Dodgers".into(),
                    opponent: "New York Yankees".into(),
                    stat_category: "Hits".into(),
                    line: 1.5,
                    league: "MLB".into(),
                    projection: Some(1.3),
                    source: "Mock".into(),
                    game_time: Some(gt(3)),
                    over_odds: Some(-110),
                    under_odds: Some(-110),
                },
                PrizePicksProp {
                    external_id: "mock-mlb-4".into(),
                    player_name: "Juan Soto".into(),
                    team: "New York Yankees".into(),
                    opponent: "Los Angeles Dodgers".into(),
                    stat_category: "RBIs".into(),
                    line: 0.5,
                    league: "MLB".into(),
                    projection: Some(0.8),
                    source: "Mock".into(),
                    game_time: Some(gt(3)),
                    over_odds: Some(-120),
                    under_odds: Some(even_money()),
                },
                PrizePicksProp {
                    external_id: "mock-mlb-5".into(),
                    player_name: "Freddie Freeman".into(),
                    team: "Los Angeles Dodgers".into(),
                    opponent: "New York Yankees".into(),
                    stat_category: "RBIs".into(),
                    line: 0.5,
                    league: "MLB".into(),
                    projection: Some(0.4),
                    source: "Mock".into(),
                    game_time: Some(gt(3)),
                    over_odds: Some(-105),
                    under_odds: Some(-115),
                },
                // ATL @ PHI
                PrizePicksProp {
                    external_id: "mock-mlb-6".into(),
                    player_name: "Ronald Acuna Jr.".into(),
                    team: "Atlanta Braves".into(),
                    opponent: "Philadelphia Phillies".into(),
                    stat_category: "Stolen Bases".into(),
                    line: 0.5,
                    league: "MLB".into(),
                    projection: Some(0.7),
                    source: "Mock".into(),
                    game_time: Some(gt(4)),
                    over_odds: Some(130),
                    under_odds: Some(-160),
                },
                PrizePicksProp {
                    external_id: "mock-mlb-7".into(),
                    player_name: "Bryce Harper".into(),
                    team: "Philadelphia Phillies".into(),
                    opponent: "Atlanta Braves".into(),
                    stat_category: "Total Bases".into(),
                    line: 1.5,
                    league: "MLB".into(),
                    projection: Some(1.8),
                    source: "Mock".into(),
                    game_time: Some(gt(4)),
                    over_odds: Some(-110),
                    under_odds: Some(-110),
                },
                PrizePicksProp {
                    external_id: "mock-mlb-8".into(),
                    player_name: "Matt Olson".into(),
                    team: "Atlanta Braves".into(),
                    opponent: "Philadelphia Phillies".into(),
                    stat_category: "Home Runs".into(),
                    line: 0.5,
                    league: "MLB".into(),
                    projection: Some(0.3),
                    source: "Mock".into(),
                    game_time: Some(gt(4)),
                    over_odds: Some(150),
                    under_odds: Some(-180),
                },
                PrizePicksProp {
                    external_id: "mock-mlb-9".into(),
                    player_name: "Trea Turner".into(),
                    team: "Philadelphia Phillies".into(),
                    opponent: "Atlanta Braves".into(),
                    stat_category: "Hits".into(),
                    line: 1.5,
                    league: "MLB".into(),
                    projection: Some(1.1),
                    source: "Mock".into(),
                    game_time: Some(gt(4)),
                    over_odds: Some(even_money()),
                    under_odds: Some(-120),
                },
            ],
            "NHL" => vec![
                // EDM @ TOR
                PrizePicksProp {
                    external_id: "mock-nhl-1".into(),
                    player_name: "Connor McDavid".into(),
                    team: "Edmonton Oilers".into(),
                    opponent: "Toronto Maple Leafs".into(),
                    stat_category: "Points".into(),
                    line: 1.5,
                    league: "NHL".into(),
                    projection: Some(1.8),
                    source: "Mock".into(),
                    game_time: Some(gt(3)),
                    over_odds: Some(-120),
                    under_odds: Some(even_money()),
                },
                PrizePicksProp {
                    external_id: "mock-nhl-2".into(),
                    player_name: "Auston Matthews".into(),
                    team: "Toronto Maple Leafs".into(),
                    opponent: "Edmonton Oilers".into(),
                    stat_category: "Goals".into(),
                    line: 0.5,
                    league: "NHL".into(),
                    projection: Some(0.7),
                    source: "Mock".into(),
                    game_time: Some(gt(3)),
                    over_odds: Some(125),
                    under_odds: Some(-155),
                },
                PrizePicksProp {
                    external_id: "mock-nhl-3".into(),
                    player_name: "Leon Draisaitl".into(),
                    team: "Edmonton Oilers".into(),
                    opponent: "Toronto Maple Leafs".into(),
                    stat_category: "Assists".into(),
                    line: 0.5,
                    league: "NHL".into(),
                    projection: Some(0.9),
                    source: "Mock".into(),
                    game_time: Some(gt(3)),
                    over_odds: Some(-115),
                    under_odds: Some(-105),
                },
                PrizePicksProp {
                    external_id: "mock-nhl-4".into(),
                    player_name: "Mitch Marner".into(),
                    team: "Toronto Maple Leafs".into(),
                    opponent: "Edmonton Oilers".into(),
                    stat_category: "Points".into(),
                    line: 0.5,
                    league: "NHL".into(),
                    projection: Some(0.6),
                    source: "Mock".into(),
                    game_time: Some(gt(3)),
                    over_odds: Some(-110),
                    under_odds: Some(-110),
                },
                // COL @ DAL
                PrizePicksProp {
                    external_id: "mock-nhl-5".into(),
                    player_name: "Nathan MacKinnon".into(),
                    team: "Colorado Avalanche".into(),
                    opponent: "Dallas Stars".into(),
                    stat_category: "Points".into(),
                    line: 1.5,
                    league: "NHL".into(),
                    projection: Some(1.3),
                    source: "Mock".into(),
                    game_time: Some(gt(4)),
                    over_odds: Some(-105),
                    under_odds: Some(-115),
                },
                PrizePicksProp {
                    external_id: "mock-nhl-6".into(),
                    player_name: "Mikko Rantanen".into(),
                    team: "Colorado Avalanche".into(),
                    opponent: "Dallas Stars".into(),
                    stat_category: "Shots on Goal".into(),
                    line: 3.5,
                    league: "NHL".into(),
                    projection: Some(4.2),
                    source: "Mock".into(),
                    game_time: Some(gt(4)),
                    over_odds: Some(-115),
                    under_odds: Some(-105),
                },
                PrizePicksProp {
                    external_id: "mock-nhl-7".into(),
                    player_name: "Jason Robertson".into(),
                    team: "Dallas Stars".into(),
                    opponent: "Colorado Avalanche".into(),
                    stat_category: "Points".into(),
                    line: 0.5,
                    league: "NHL".into(),
                    projection: Some(0.8),
                    source: "Mock".into(),
                    game_time: Some(gt(4)),
                    over_odds: Some(-110),
                    under_odds: Some(-110),
                },
                PrizePicksProp {
                    external_id: "mock-nhl-8".into(),
                    player_name: "Miro Heiskanen".into(),
                    team: "Dallas Stars".into(),
                    opponent: "Colorado Avalanche".into(),
                    stat_category: "Assists".into(),
                    line: 0.5,
                    league: "NHL".into(),
                    projection: Some(0.4),
                    source: "Mock".into(),
                    game_time: Some(gt(4)),
                    over_odds: Some(even_money()),
                    under_odds: Some(-120),
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
                game_time: Some(gt(3)),
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
    -100
}
