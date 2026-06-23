# PrizePicks Monster v1.0.0

**AI-powered DFS player prop intelligence engine** — A Tauri 2 + Rust desktop application that connects to OpenRouter API to deliver probability-weighted player prop assessments with real-time sports data, matchup analysis, and risk-adjusted decision modeling.

> **Analytics and research only.** This app never places, sizes, sends, or
> auto-submits a real bet. It reads public data and produces analysis.

## What It Is

PrizePicks Monster is a desktop app designed to give AI models deep analytical capabilities for DFS player props. Every assessment provides:

- **Real-time player prop data** for NFL, NBA, MLB, NHL
- **Risk-adjusted expected value (EV) analysis** with Kelly criterion sizing
- **Matchup-aware projections** with defensive rankings, pace, and usage rates
- **Weather and injury integration** for game-day context
- **Structured prediction tracking** and performance calibration
- **Multi-source prop data** with automated failover (OpticOdds → Apify → Direct → Mock)
- **Intelligent risk-flagging** for extreme projections or data gaps

## Architecture

```
prizepicks-monster/
├── src-tauri/           # Rust backend
│   ├── src/
│   │   ├── lib.rs       # App entry, state management
│   │   ├── config.rs    # App config, model list, API status
│   │   ├── commands/    # Tauri command handlers
│   │   ├── chat/        # Chat sessions + OpenRouter API
│   │   ├── predictions/ # Prediction tracking + calibration
│   │   ├── prizepicks/  # PrizePicks data modules
│   │   │   ├── client.rs        # Trading API client (odds comparison)
│   │   │   ├── prop_fetcher.rs  # Player prop data fetcher
│   │   │   ├── grading.rs       # Prop grading engine
│   │   │   ├── portfolio_risk.rs # Kelly scaling
│   │   │   └── price_tracker.rs # Line movement tracking
│   │   ├── football/    # Sports data (ESPN, Sleeper)
│   │   ├── analysis/    # Edge calculator, matchup analyzer, prop scorer
│   │   ├── bankroll/    # Bankroll management
│   │   ├── paper/       # Paper trading journal
│   │   └── ml_predictor/# ML model integration
│   └── Cargo.toml
├── src-ui/              # React + TypeScript frontend
│   ├── src/
│   │   ├── App.tsx              # Main app shell
│   │   ├── components/
│   │   │   ├── PropsView.tsx         # Prop board with edge analysis
│   │   │   ├── PrizePicksView.tsx    # PrizePicks dashboard
│   │   │   ├── ChatView.tsx          # AI chat interface
│   │   │   ├── PrizePicksPredictionsPanel.tsx  # Pick log + analytics
│   │   │   └── SettingsView.tsx      # App settings
│   │   └── types/           # TypeScript type definitions
│   └── package.json
└── README.md
```

## Getting Started

### Prerequisites
- [Rust](https://rustup.rs/) (1.85+)
- [Node.js](https://nodejs.org/) (18+)
- [Tauri prerequisites](https://tauri.app/start/prerequisites/) for your OS

### Development
1. Clone the repository.
2. Install dependencies: `npm install --prefix src-ui`
3. Run in dev mode: `npm run tauri dev`

```bash
# Install frontend dependencies
cd src-ui && npm install

# Run in development mode (from project root)
npm run tauri dev

# Build for production
npm run tauri build
```

### First Run
1. Launch the app
2. Go to **Settings** → Enter your [OpenRouter API key](https://openrouter.ai/keys)
3. Click **Test Connection** to verify
4. Select your preferred model (Claude Sonnet 4 recommended)
5. Navigate to **Prop board** to browse player props
6. Use **Analyst chat** for AI-powered prop analysis

## Tech Stack

- **Desktop Framework:** Tauri 2 (Rust)
- **Frontend:** React 18 + TypeScript + Tailwind CSS v4
- **AI API:** OpenRouter (multi-model gateway)
- **Styling:** Custom dark theme
- **State:** Tokio async runtime + Tauri managed state
- **Storage:** SQLite for predictions and paper trading

## License

MIT
