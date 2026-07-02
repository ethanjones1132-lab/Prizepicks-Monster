<div align="center">

# 🏆 PrizePicks Monster

**AI-powered DFS player prop intelligence engine** — Real-time prop analysis, edge calculation, and portfolio-aware Kelly sizing for NFL, NBA, MLB, and NHL.

[![Version](https://img.shields.io/badge/version-1.0.0-blue)]()
[![License](https://img.shields.io/badge/license-MIT-green)]()
[![Platform](https://img.shields.io/badge/platform-Windows%20|%20macOS%20|%20Linux-lightgrey)]()
[![Rust](https://img.shields.io/badge/built%20with-Rust-orange)]()
[![React](https://img.shields.io/badge/UI-React-61DAFB)]()

[Buy Now](https://coppercreekcoffee.myshopify.com/) · [Issues](https://github.com/JonesinSRC/prizepicks-monster/issues)

</div>

---

## Overview

PrizePicks Monster is a **desktop application** that supercharges your DFS player prop research with AI-powered analysis. It connects to the OpenRouter API to deliver probability-weighted assessments backed by real-time sports data, matchup analysis, and risk-adjusted position sizing — all running locally on your machine.

### Who is this for?

Daily Fantasy Sports players who want **an edge beyond the public narrative** — whether you're a casual prop picker looking for data-backed confidence, or a sharp player who wants portfolio-aware Kelly sizing across correlated legs.

### What makes it different?

- **AI analyst, not a feed.** Connect any frontier LLM (Claude, GPT, Gemini, DeepSeek) and get reasoned prop evaluations — not just raw stats.
- **Portfolio-aware sizing.** Kelly criterion with automatic correlation scaling ensures correlated positions (same player, same game) don't over-weight your paper portfolio.
- **Multi-source failover.** Data from OpticOdds → Apify → Direct API → Mock ensures you see props even when upstream sources are down.
- **Everything local.** Your API keys, predictions, and analysis never leave your machine. The app stores data in a local SQLite database.

```
Real-time sports data → [Multi-source fetcher] → [AI analyst (OpenRouter)] → [Edge calculator] → [Prop score + Kelly size]
```

---

## Quick Start

1. **Download and install** the latest release for your OS.
2. **Launch the app** and navigate to **Settings**.
3. **Enter your OpenRouter API key** (get one at [openrouter.ai/keys](https://openrouter.ai/keys)).
4. **Click Test Connection** to verify.
5. **Select a model** (Claude Sonnet 4 recommended for best results).
6. **Browse the Prop board** to see live player props.
7. **Use the Analyst chat** for AI-driven prop analysis and paper trade decisions.

---

## Features

| Feature | Description |
|---------|-------------|
| 🎯 **Prop board** | Browse live player props for NFL, NBA, MLB, NHL with edge analysis |
| 🧠 **AI Analyst chat** | Natural language prop analysis via any OpenRouter-compatible model |
| 📊 **PrizePicks dashboard** | Full catalog with category filters, league switches, and price history |
| 📈 **Prediction log** | Track every pick with Over/Under grading, PnL tracking, and equity curves |
| 🔍 **Portfolio risk** | Kelly criterion sizing with correlation-aware scaling across legs |
| 📝 **Paper journal** | Annotate trades with notes and tags; slice performance by tag, player, category, side, hold time, entry price, confidence tier, and model disagreement |
| 🤖 **ML predictor** | Train per-category ML classifiers on your resolved picks for live scoring |
| 📐 **Calibration scatter** | Visualize model confidence vs. realized PnL to detect over/under-confidence |
| ⚡ **Auto-grading** | Background polling resolves your paper trades automatically |
| 🔒 **Local-first** | All data stored in SQLite on your machine — nothing leaves without your permission |

---

## Screenshots

> **📸 Screenshots coming soon.** The app features a dark-themed UI with the following views:
> - **Prop board** — Scored player props with edge indicators and green/red confidence bars
> - **PrizePicks dashboard** — Full market catalog with category/league filters and price history charts
> - **Analyst chat** — Conversational AI prop analysis with structured decision output
> - **Prediction log** — Paper equity curve, win/loss streak chip, per-player/per-category/per-side/per-hold-time/per-tag/per-confidence-tier/per-disagreement PnL breakdowns, journal editor, and calibration scatter plot
> - **ML predictor** — Feature importance chart, per-category classifier metrics, live prop scoring
>
> Add visuals in the `assets/` directory and reference them with standard markdown image syntax (`![alt](assets/screenshot.png)`).

---

## Installation

### System Requirements

| Platform | Minimum Requirement |
|----------|-------------------|
| **Windows** | Windows 10 or later, 4GB RAM |
| **macOS** | macOS 12 (Monterey) or later, Apple Silicon or Intel |
| **Linux** | Modern distro with WebKitGTK, 4GB RAM |

### Download

Grab the latest installer for your platform from the [releases page](https://github.com/JonesinSRC/prizepicks-monster/releases) or purchase a license at [coppercreekcoffee.myshopify.com](https://coppercreekcoffee.myshopify.com/).

### From Source (developers)

```bash
# Prerequisites: Rust 1.85+, Node.js 18+, Tauri prerequisites
git clone https://github.com/JonesinSRC/prizepicks-monster.git
cd prizepicks-monster
npm install --prefix src-ui
npm run tauri dev
```

---

## Usage

### AI prop analysis workflow

1. **Open the Prop board** to see scored props with edge percentages and expected value.
2. **Click any prop** to open the detail panel with price history, correlation warnings, and Kelly sizing.
3. **Switch to the Analyst chat** and ask questions like:
   - *"What's the best value on tonight's NBA slate?"*
   - *"Analyze Josh Allen Over 275.5 passing yards"*
   - *"How confident are you in the SGA points prop?"*
   - *"Show me props with model disagreement >12pp"*
4. **Review predictions** in the Prediction log with full PnL breakdowns by:
   - Player, stat category, Over/Under side
   - Hold time buckets (intraday, same-day, multi-day, long)
   - Entry price buckets (0-20¢, 20-40¢, etc.)
   - Confidence tier (High/Medium/Low)
   - Model disagreement vs. consensus
   - Custom tags via the paper journal
5. **Train the ML predictor** once you have 10+ resolved picks for per-category scoring.

### Configuration

| Setting | Description | Default |
|---------|-------------|---------|
| OpenRouter API key | Required. Get one at openrouter.ai/keys | — |
| AI model | Any OpenRouter-compatible model | Claude Sonnet 4 |
| Kelly multiplier | Fraction of full Kelly to use | 0.25 |
| Max bet % | % of portfolio per leg | configurable |
| Min quality threshold | Minimum edge quality to show | configurable |

---

## How It Works

```
                    ┌───────────────────┐
                    │   OpenRouter AI   │
                    │  (your model of   │
                    │     choice)       │
                    └────────┬──────────┘
                             │
┌──────────────┐    ┌────────▼──────────┐    ┌───────────────┐
│ Multi-source  │───▶│  Prop Analyzer    │───▶│  Edge Calc    │
│ Data Fetcher  │    │ (matchup, inj.,   │    │  + Kelly      │
│ (OpticOdds →  │    │  weather, usage)  │    │  Sizing       │
│  Apify → API) │    └───────────────────┘    └───────┬───────┘
└──────────────┘                                     │
                                              ┌──────▼───────┐
                                              │ Paper Trading │
                                              │  (local SQL)  │
                                              │  + Auto-grade │
                                              │  + Calibration│
                                              └──────────────┘
```

### Data pipeline

1. **Fetch** — Multi-source failover (OpticOdds → Apify → Direct API → Mock) pulls live player props.
2. **Analyze** — The AI model evaluates each prop against matchup data, defensive rankings, pace, weather, and injury context.
3. **Score** — The edge calculator produces risk-adjusted expected value with Kelly criterion sizing scaled for correlated positions.
4. **Track** — Every paper trade is stored locally with full decision context, graded automatically on resolution, and analyzed via performance breakdowns + calibration scatter.

---

## Technical Highlights

- **Desktop-native** — Built with Tauri 2 (Rust) for a snappy, low-memory footprint — no Electron bloat.
- **Multi-model AI** — OpenRouter gateway means you can use Claude, GPT, Gemini, DeepSeek, or any model behind a single API key.
- **Local-first architecture** — SQLite-backed storage with zero telemetry. Your data is yours.
- **Kelly portfolio engine** — Sophisticated correlation-aware position sizing with event, series, category, and same-ticker scaling.
- **Isotonic calibration** — Live Brier-score-driven Kelly shrinkage adapts to your actual forecasting accuracy.
- **Per-category ML** — Per-stat-category gradient-boosted classifiers trained on your resolved picks for live prop scoring.
- **10 performance breakdowns** — Slice paper PnL by category, side, hold time, player, entry price, disagreement, confidence tier, tags, Brier calibration, and equity curve — all computed locally.

### Architecture

```
prizepicks-monster/
├── src-tauri/           # Rust backend
│   ├── src/
│   │   ├── lib.rs       # App entry, state management
│   │   ├── commands/    # Tauri command handlers
│   │   ├── chat/        # OpenRouter API chat sessions
│   │   ├── prizepicks/  # Data client, fetcher, grading, portfolio
│   │   ├── analysis/    # Edge calc, matchup analyzer, prop scorer
│   │   ├── predictions/ # Prediction storage + CLV tracking
│   │   ├── paper/       # Paper trading + performance analytics
│   │   └── ml_predictor/# Python ML subprocess + per-category models
│   └── Cargo.toml
├── src-ui/              # React + TypeScript + Tailwind CSS
│   └── src/components/  # Prop board, dashboard, chat, analytics
└── README.md
```

---

## FAQ

**Q: Do I need an OpenRouter API key?**
A: Yes. OpenRouter is the AI gateway. It costs pennies per session (typically <$0.05 for a full prop analysis). Get a free key at [openrouter.ai/keys](https://openrouter.ai/keys).

**Q: Does this place real bets?**
A: **No.** PrizePicks Monster is an analytics and research tool only. It never places, sends, or auto-submits a real wager. Paper trading is entirely simulated for research and calibration purposes.

**Q: Which sports are supported?**
A: NFL, NBA, MLB, and NHL — the four major US leagues on PrizePicks.

**Q: What models work best?**
A: Claude Sonnet 4 is the recommended default for best analytical reasoning. GPT-4o and DeepSeek V4 are also solid. Smaller models work but produce less nuanced analysis.

**Q: Where is my data stored?**
A: Everything is stored locally in SQLite at `~/.openclaw/prizepicks-monster/`. No telemetry, no cloud sync, no data leaving your machine.

**Q: Can I use this on macOS or Linux?**
A: Yes — the app is built with Tauri 2 and targets Windows, macOS, and Linux.

---

## Changelog

| Date | Version | Highlights |
|------|---------|------------|
| 2026-07-02 | 1.0.0 | Per-confidence-tier & per-tag paper breakdowns; paper journal UI (notes/tags editor); calibration scatter plot; 10 performance views |
| 2026-06-30 | 0.9.x | Per-entry-price paper breakdown; disagreement-bucket PnL; startup prefetch |
| 2026-06-29 | 0.8.x | Per-player & per-hold-time paper breakdowns; session PnL chips |
| 2026-06-28 | 0.7.x | Per-side paper breakdown; dashboard bootstrap IPC; full-catalog prefetch |
| 2026-06-27 | 0.6.x | Equity curve chart; streak indicator; ML predictor UI; per-category ML classifiers |
| 2026-06-26 | 0.5.x | ML predictor pipeline; Brier-shrunk Kelly wiring; auto-grade for PrizePicks |
| 2026-06-25 | 0.4.x | CLV tracking; model disagreement flags; volatility-adjusted Kelly |
| 2026-06-24 | 0.3.x | Bankroll sync; correlation warnings; config persistence |
| 2026-06-17 | 0.2.x | Quick cache; dashboard performance; multi-source failover |
| 2026-06-10 | 0.1.x | Initial Tauri 2 rebuild from Kalshi Monster |

---

## Contributing

Pull requests are welcome. For major changes, please open an issue first to discuss what you'd like to change.

```bash
# Set up dev environment
git clone https://github.com/JonesinSRC/prizepicks-monster.git
cd prizepicks-monster
npm install --prefix src-ui
npm run tauri dev

# Run tests
cd src-tauri && cargo test
```

---

## License

MIT — see [LICENSE](LICENSE).

---

<div align="center">

**PrizePicks Monster** — Built with ❤️ by [JonesinSRC](https://coppercreekcoffee.myshopify.com/)

[Buy PrizePicks Monster](https://coppercreekcoffee.myshopify.com/) · [Report an Issue](https://github.com/JonesinSRC/prizepicks-monster/issues)

**Analytics and research only. Not affiliated with PrizePicks. Never place real wagers through this software.**

</div>