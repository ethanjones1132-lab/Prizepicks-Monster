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
    date: '2026-07-23',
    title: 'Player quick-view — click a player name to see all their props',
    bullets: [
      'Click any player name on a prop card (in detailed or compact view) to open a focused quick-view panel showing all that player\'s props across all stat categories',
      'Player quick-view appears between the Top Picks section and the main props grid — keeps your filter context intact while zooming into a specific player\'s portfolio',
      'Each player card in the quick-view shows prop type, line, projection, edge%, confidence%, risk badge, league chip, and recommendation — everything you need at a glance',
      'Blue-accented container with a × close button; player name links on prop cards have a dotted underline with blue hover highlight for discoverability',
    ],
  },
  {
    date: '2026-07-23',
    title: 'Game time indicators on prop cards — time context at a glance',
    bullets: [
      'Relative game time badge (e.g. "in 3h", "tomorrow", "2h ago") on every prop card in both detailed and compact views',
      'Detailed card view shows game time alongside team and matchup in the card meta area',
      'Compact card view shows game time after the recommendation text — never lose temporal context when scrolling',
      'Game time uses the existing gameTimeRelative() helper for human-readable labels the app already uses in group headers',
    ],
  },
  {
    date: '2026-07-23',
    title: 'Prop quality score — edge×confidence combined metric',
    bullets: [
      'New "Score" sort option that combines edge% × confidence% into a single 0-100 quality metric — highest-value props float to the top',
      'Quality score badge (e.g. "72") on every prop card in both detailed and compact views, color-coded green (top ≥40), amber (good ≥20), or gray (ok)',
      'One-click sort by Score from the dropdown to instantly see the best overall opportunities',
    ],
  },
  {
    date: '2026-07-23',
    title: 'Compact prop card layout — density toggle for power users',
    bullets: [
      'One-click density toggle (☐/▣) in the props section header switches between detailed card view and compact row view',
      'Compact rows show key stats inline: player, category, line/projection, edge%, confidence%, risk dot, league, recommendation',
      'Watchlist star, copy, and insight buttons still available in compact mode with smaller click targets',
      'Expanded insight panel opens inline within the compact row on 🔍 click',
      'Preference persists in localStorage and survives page reloads',
    ],
  },
  {
    date: '2026-07-22',
    title: 'Recommendation filter chips — filter props by Over/Under recommendation type',
    bullets: [
      'One-click recommendation filter chips dynamically populated from loaded props (e.g. "🔥 ELITE PICK", "👍 PLAYABLE", "Strong Over")',
      'Compact row follows the same chip pattern as risk/category/team filters with count badges per recommendation',
      'Integrates with all existing filters — filter presets, localStorage persistence, empty-state messages, and ↺ Reset',
    ],
  },
  {
    date: '2026-07-22',
    title: 'Edge distribution mini-bar — visual edge tier breakdown',
    bullets: [
      'Compact stacked horizontal bar showing the proportional breakdown of props by edge tier (High ≥10%, Good ≥5%, Modest ≥2%, Neutral, Poor ≤-2%)',
      'Colored segments map to the same edge-strength colors used on prop cards — instant visual sense of overall prop quality',
      'Legend row below the bar shows per-tier count with colored dots for quick scanning',
      'Tooltip on each bar segment gives exact tier name, threshold, and prop count',
    ],
  },
  {
    date: '2026-07-22',
    title: 'Prop insight detail panel',
    bullets: [
      'Click 🔍 on any prop card to expand an inline detail panel with model reasoning, probability breakdown, source, and update time',
      'Model probability vs Market (implied) probability shown side-by-side for at-a-glance confidence comparison',
      'Data source and last-updated timestamp per prop for transparency',
      'Active insight button highlighted with gold glow when panel is open',
    ],
  },
  {
    date: '2026-07-21',
    title: 'Multi-select team filter',
    bullets: [
      'Team filter chips now support multi-select — toggle multiple teams at once instead of single-select',
      '"All" button clears all team selections; active teams highlighted with gold accent',
      'Empty-state messages list all selected teams when multiple are active',
      'Legacy single-team localStorage data migrates automatically on first load',
    ],
  },
  {
    date: '2026-07-21',
    title: 'Risk level filter + filtered-props CSV & confidence presets',
    bullets: [
      'Risk level filter chips (Low / Medium / High) — filter props by risk tier for quick low/high-risk scoping',
      'Per-risk-level count badges on filter chips showing prop distribution',
      'CSV export now generates from the currently visible (filtered/sorted) props instead of all backend props — what you see is what you get',
      'One-click confidence preset chips (≥60%, ≥70%, ≥80%) alongside the Min conf input — matching the edge preset pattern for instant threshold toggling',
      'Active chip highlighted when confidence matches a preset; custom values still supported via the number input',
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
