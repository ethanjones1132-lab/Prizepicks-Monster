# AGENTS.md — PrizePicks Monster Working Rules

This file gives autonomous AI agents the minimal project-specific context needed to work safely in this repo.

## What this repo is
A DFS player prop intelligence app branded as PrizePicks Monster. It provides AI-powered analysis of player props for NFL, NBA, MLB, and NHL.

## Read first
- `README.md` — current repo overview and calibration posture
- `PRIORITIES.md` — ranked P0–P3 improvement backlog and completion status (if present)

## Key areas
- `src-tauri/src/` — Rust backend code
- `src-tauri/src/prizepicks/` — PrizePicks data modules (client, fetcher, grading, etc.)
- `src-tauri/src/football/` — Sports data (ESPN, Sleeper)
- `src-tauri/src/analysis/` — Edge calculator, matchup analyzer, prop scorer
- `src-tauri/src/chat/` — Chat sessions + OpenRouter API
- `src-tauri/src/predictions/` — Prediction tracking + calibration
- `src-ui/src/` — React frontend
- `reports/` — Evaluation and calibration artifacts
- `scripts/` — Helper scripts

## Working rules
1. This is a **PrizePicks Monster** application. Do not import Kalshi assumptions, market logic, or prediction-market terminology unless explicitly referenced.
2. Preserve the app's **research / analytics posture**. Do not imply real order execution or betting.
3. Player props use **Over/Under** format, not YES/NO contract format.
4. Data sources: OpticOdds → Apify → Direct API → Mock (multi-source failover).
5. Keep conclusions evidence-first. If a claim is not supported by the repo, a real artifact, or a concrete verification step, do not present it as fact.
6. If evidence for a product-specific claim is thin, say so clearly.
7. Never commit, publish, place bets, or start long-lived services.

## Config
- Config dir: `~/.openclaw/prizepicks-monster/`
- Config file: `config.json`
- Predictions DB: `predictions.db`
