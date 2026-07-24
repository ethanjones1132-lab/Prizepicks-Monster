#!/usr/bin/env python
"""Add game time horizon filter to PrizePicksView.tsx — 15+ integration points."""
import re, sys

path = "C:/Projects/prizepicks-monster/src-ui/src/components/PrizePicksView.tsx"
with open(path, 'rb') as f:
    data = f.read()

original_len = len(data)
text = data.decode('utf-8')

# 1. Add gameTimeBucket helper after gameTimeRelative function (before formatTimeAgo)
text = text.replace(
    "function formatTimeAgo(ts: number): string {",
    """/**
 * Classify a game time ISO string into a time-horizon bucket for the game time filter.
 * Returns 'today', 'tomorrow', 'this_week', 'future', 'past', or '' (for invalid/missing).
 */
function gameTimeBucket(gameTime: string | undefined | null): string {
  if (!gameTime) return '';
  const now = Date.now();
  const gameDate = new Date(gameTime).getTime();
  if (!Number.isFinite(gameDate)) return '';
  const diffMs = gameDate - now;
  const diffSec = Math.round(diffMs / 1000);
  if (diffSec < 0) return 'past';
  if (diffSec < 86400) return 'today';
  if (diffSec < 172800) return 'tomorrow';
  if (diffSec < 604800) return 'this_week';
  return 'future';
}

/** Human-readable labels for game time horizon buckets. */
const GAME_TIME_LABELS: Record<string, string> = {
  today: 'Today',
  tomorrow: 'Tomorrow',
  this_week: 'This Week',
  future: 'Future',
  past: 'Past',
};

function formatTimeAgo(ts: number): string {""",
    1
)

# 2. Add selectedGameTime to DashboardPreferences interface (after selectedRecommendation)
text = text.replace(
    "  selectedRecommendation: string;",
    "  selectedRecommendation: string;\n  selectedGameTime: string;"
)

# 3. Add selectedGameTime to DEFAULT_PREFERENCES
text = text.replace(
    "  selectedRecommendation: 'All',",
    "  selectedRecommendation: 'All',\n  selectedGameTime: 'All',"
)

# 4. Add selectedGameTime state declaration (after selectedRecommendation state)
text = text.replace(
    "  const [selectedRecommendation, setSelectedRecommendation] = useState(savedPreferences.selectedRecommendation ?? 'All');",
    "  const [selectedRecommendation, setSelectedRecommendation] = useState(savedPreferences.selectedRecommendation ?? 'All');\n  const [selectedGameTime, setSelectedGameTime] = useState(savedPreferences.selectedGameTime ?? 'All');"
)

# 5. Add selectedGameTime to hasActiveFilters
text = text.replace(
    "|| selectedRecommendation !== 'All' || playerFilter !== '' || showWatchlist;",
    "|| selectedRecommendation !== 'All' || selectedGameTime !== 'All' || playerFilter !== '' || showWatchlist;"
)

# 6. Add setSelectedGameTime('All') to resetFilters
text = text.replace(
    "    setSelectedRecommendation('All');",
    "    setSelectedRecommendation('All');\n    setSelectedGameTime('All');"
)

# 7. Add selectedGameTime to saveCurrentAsPreset
text = text.replace(
    "          selectedRecommendation,",
    "          selectedRecommendation,\n          selectedGameTime,"
)

# 8. Add setSelectedGameTime to applyPreset
text = text.replace(
    "    setSelectedRecommendation(preset.selectedRecommendation ?? 'All');",
    "    setSelectedRecommendation(preset.selectedRecommendation ?? 'All');\n    setSelectedGameTime(preset.selectedGameTime ?? 'All');"
)

# 9. Add selectedGameTime comparison to activePresetName
text = text.replace(
    "        p.selectedRecommendation === selectedRecommendation &&",
    "        p.selectedGameTime === selectedGameTime &&\n        p.selectedRecommendation === selectedRecommendation &&"
)

# 10. Add game time to describePreset
text = text.replace(
    "  if (preset.selectedRecommendation !== 'All') {",
    "  if (preset.selectedGameTime !== 'All') {\n    parts.push(GAME_TIME_LABELS[preset.selectedGameTime] || preset.selectedGameTime);\n  }\n  if (preset.selectedRecommendation !== 'All') {"
)

# 11. Add selectedGameTime to savePreferences useEffect deps
text = text.replace(
    "      selectedRecommendation,",
    "      selectedRecommendation,\n      selectedGameTime,"
)

# 12. Add selectedGameTime to savePreferences useEffect value
text = text.replace(
    "      selectedRecommendation,\n      compactView",
    "      selectedRecommendation,\n      selectedGameTime,\n      compactView"
)

# 13. Add setSelectedGameTime('All') to props-reload useEffect reset
text = text.replace(
    "    setSelectedRecommendation('All');\n    setPlayerFilter('');",
    "    setSelectedRecommendation('All');\n    setSelectedGameTime('All');\n    setPlayerFilter('');"
)

# 14. Add game time count/compute hooks (after recommendations/compute block)
text = text.replace(
    "  // Compute per-recommendation prop counts for filter chip badges\n  const recommendationCounts = useMemo(() => {",
    "  // Compute which game time horizon buckets have props\n  const gameTimeBuckets = useMemo(() => {\n    const bkt = new Set(props.map((p) => gameTimeBucket(p.game_time)).filter(Boolean));\n    return ['All', ...GAME_TIME_OPTIONS.filter((x) => x !== 'All' && bkt.has(x))];\n  }, [props]);\n\n  // Compute per-bucket prop counts for filter chip badges\n  const gameTimeCounts = useMemo(() => {\n    const counts: Record<string, number> = {};\n    for (const p of props) {\n      const b = gameTimeBucket(p.game_time);\n      if (b) counts[b] = (counts[b] || 0) + 1;\n    }\n    return counts;\n  }, [props]);\n\n  // Compute per-recommendation prop counts for filter chip badges\n  const recommendationCounts = useMemo(() => {"
)

# 15. Add filter step for selectedGameTime in displayProps (before showWatchlist)
text = text.replace(
    "    if (selectedRecommendation !== 'All') {\n      filtered = filtered.filter((p) => p.recommendation === selectedRecommendation);\n    }",
    "    if (selectedRecommendation !== 'All') {\n      filtered = filtered.filter((p) => p.recommendation === selectedRecommendation);\n    }\n    if (selectedGameTime !== 'All') {\n      filtered = filtered.filter((p) => gameTimeBucket(p.game_time) === selectedGameTime);\n    }"
)

# 16. Add selectedGameTime to displayProps deps
text = text.replace(
    "selectedRecommendation, showWatchlist]);",
    "selectedRecommendation, selectedGameTime, showWatchlist]);"
)

# 17. Update displayProps deps array to include selectedGameTime
# The dep line is already updated above. Good.

# 18. Add GAME_TIME_OPTIONS constant (at module level)
text = text.replace(
    "const GAME_TIME_LABELS: Record<string, string> = {",
    "const GAME_TIME_OPTIONS = ['All', 'today', 'tomorrow', 'this_week', 'future', 'past'];\n\nconst GAME_TIME_LABELS: Record<string, string> = {"
)

# Write back
data = text.encode('utf-8')
with open(path, 'wb') as f:
    f.write(data)

# Verify
lines_added = len(data) - original_len
print(f"OK: PrizePicksView.tsx updated ({len(data)} bytes, ~{lines_added} net new bytes)")

# Check for corruption
cr_lf = data.count(b'\r\n')
lf_only = data.count(b'\n') - cr_lf
literal_n = data.count(b'\\n')
print(f"Line endings: CRLF={cr_lf}, LF-only={lf_only}")
print(f"Literal \\\\n: {literal_n}")
if literal_n > 0 and cr_lf == 0:
    print("WARNING: Literal \\n detected!")
    sys.exit(1)
print("PASS: No literal-\\n corruption detected")
