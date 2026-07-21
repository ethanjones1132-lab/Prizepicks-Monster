/**
 * What's New — changelog entries for the in-app modal.
 * Each entry has a date, a concise title, and a bullet list of features.
 * The most recent entries appear first.
 */

export interface WhatsNewEntry {
  date: string;       // e.g. '2026-07-17'
  title: string;      // e.g. 'Prop watchlist & risk badges'
  bullets: string[];  // key features shipped
}

export const WHATS_NEW_STORAGE_KEY = 'prizepicks_whatsnew_last_seen';

export const WHATS_NEW_ENTRIES: WhatsNewEntry[] = [
  {
    date: '2026-07-21',
    title: 'Quick-confidence preset chips on dashboard',
    bullets: [
      'One-click confidence preset chips (≥60%, ≥70%, ≥80%) alongside the Min conf input — matching the edge preset pattern for instant threshold toggling',
      'Active chip highlighted when confidence matches a preset; custom values still supported via the number input',
      'Reuses the existing .chip.mini CSS class for a consistent compact look',
    ],
  },
  {
    date: '2026-07-17',
    title: 'Prop watchlist & risk badges',
    bullets: [
      'Star bookmark button (☆/⭐) on each prop card — toggle watchlist state persisted to localStorage',
      'Watchlist filter chip alongside league tabs — only bookmarked props shown when active',
      'Risk badge (low/medium/high) on each prop card with green/amber/red color-coded borders',
      'Empty-state messages guide users to bookmark props when watchlist is active',
    ],
  },
  {
    date: '2026-07-16',
    title: 'Relative game time labels',
    bullets: [
      'Human-readable relative labels alongside absolute game times ("in 3h", "tomorrow", "just now")',
      'One-click ↺ Reset button when any filter deviates from defaults',
      'Dashboard UI preferences (sort, filters, min edge) persisted via localStorage',
    ],
  },
  {
    date: '2026-07-15',
    title: 'League tab prop count badges & player name filter',
    bullets: [
      'Per-league prop count badges on each league tab (All / NFL / NBA / MLB / NHL)',
      'Compact player name text input for client-side filtering by player name',
      'Empty-state messages differentiate player filter from other empty-state reasons',
    ],
  },
  {
    date: '2026-07-14',
    title: 'Team filter, collapsible game groups & game-grouped grid',
    bullets: [
      'Clickable team abbreviation filter chips with gold-accent styling',
      'Game group headers are now clickable to collapse/expand — state persists in localStorage',
      'Props grid organized by game/matchup with chronological headers',
      'Clear PrizePicks cache button in Settings',
    ],
  },
  {
    date: '2026-07-13',
    title: 'Edge-strength visual coloring & last updated timestamp',
    bullets: [
      'Colored left-border indicator on each prop card (green for high edge, red for poor)',
      'Human-readable "Updated X ago" timestamp on the dashboard header',
      'CSV export download for player props on the dashboard',
    ],
  },
  {
    date: '2026-07-12',
    title: 'Team & game context on prop cards + min edge filter + data source chip',
    bullets: [
      'Team abbreviation and game info on every prop card',
      'Minimum edge threshold filter ("Min edge: N%") to hide low-edge props',
      'Data source indicator chip (OpticOdds / The Odds API / ESPN / Sleeper / Mock)',
    ],
  },
  {
    date: '2026-07-11',
    title: 'Notification settings, API key UI & SQLite cache persistence',
    bullets: [
      'In-app notification type toggles in Settings (enable/disable notification types)',
      'API key configuration fields for OpticOdds & The Odds API in Settings UI',
      'Cache now persists to SQLite — instant data on next launch',
      'Capture CLV button on predictions panel',
    ],
  },
  {
    date: '2026-07-10',
    title: 'Props sort dropdown & multi-source data pipeline',
    bullets: [
      'Sort controls for the main props grid (Edge / Confidence / Projection / Name)',
      'Multi-source prop data pipeline — OpticOdds → The Odds API → ESPN → Sleeper → Mock',
      'Slim cache to PrizePicksMarketSummary for faster dashboard loads',
    ],
  },
  {
    date: '2026-07-09',
    title: 'Profit Factor, stat category filter, OTel SDK & tracing bridge',
    bullets: [
      'Profit Factor cell in the paper summary card',
      'Client-side stat category filter chip row on the dashboard',
      'OpenTelemetry SDK with stdout exporter',
      'tracing-opentelemetry bridge for automatic span conversion',
    ],
  },
  {
    date: '2026-07-08',
    title: 'Notification Center UI & OTel foundation',
    bullets: [
      'Full Notification Center with click-to-mark-read, dismiss, and unread polling badge',
      'OpenTelemetry structural scaffold wired into app startup',
    ],
  },
];
