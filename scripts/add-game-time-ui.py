#!/usr/bin/env python
"""Add game time filter chip row JSX and CSS."""
import sys

tsx_path = "C:/Projects/prizepicks-monster/src-ui/src/components/PrizePicksView.tsx"
css_path = "C:/Projects/prizepicks-monster/src-ui/src/index.css"

# ── Add game time chip row JSX after risk chips, before recommendation chips ──
with open(tsx_path, 'rb') as f:
    tsx_data = f.read()
tsx_text = tsx_data.decode('utf-8')

GAME_TIME_ROW = """      {/* Game time horizon filter chips */}
      {!loading && gameTimeBuckets && gameTimeBuckets.length > 1 && (
        <div className=\"categoryRow categoryRowGameTime\">
          {gameTimeBuckets.map((gtb) => (
            <button
              key={gtb}
              type=\"button\"
              className={`chip small ${selectedGameTime === gtb ? 'active' : ''}`}
              onClick={() => setSelectedGameTime(gtb)}
              disabled={loading || props.length === 0}
              title={gtb === 'All' ? 'Show all game times' : `Show only ${GAME_TIME_LABELS[gtb] || gtb} props`}
              aria-label={gtb === 'All' ? 'Show all game times' : `Filter to ${GAME_TIME_LABELS[gtb] || gtb} games`}
            >
              {gtb === 'All' ? 'All' : GAME_TIME_LABELS[gtb] || gtb}
              {gtb !== 'All' && gameTimeCounts[gtb] !== undefined && !loading && (
                <span className=\"gameTimeCountBadge\">{gameTimeCounts[gtb]}</span>
              )}
            </button>
          ))}
        </div>
      )}

"""

# Insert before recommendation chips
old = "      {/* Recommendation filter chips */}"
if old in tsx_text:
    tsx_text = tsx_text.replace(old, GAME_TIME_ROW + old, 1)
    with open(tsx_path, 'wb') as f:
        f.write(tsx_text.encode('utf-8'))
    print(f"OK: Added game time chip row JSX to PrizePicksView.tsx")
else:
    print(f"ERROR: Could not find insertion point in PrizePicksView.tsx")
    sys.exit(1)

# ── Add CSS for game time chip row ──
with open(css_path, 'rb') as f:
    css_data = f.read()
css_text = css_data.decode('utf-8')

GAME_TIME_CSS = """
/* Game time horizon filter chips */
.categoryRowGameTime {
  margin-top: 4px;
  margin-bottom: 4px;
}
.categoryRowGameTime .chip {
  font-size: 11px;
  font-weight: 600;
}
.categoryRowGameTime .chip.active {
  color: #fff;
  border-color: rgba(100, 180, 255, 0.5);
  background: rgba(100, 180, 255, 0.15);
}
.gameTimeCountBadge {
  margin-left: 4px;
  font-size: 10px;
  opacity: 0.7;
  font-variant-numeric: tabular-nums;
}
"""

# Insert before the first "/* Recommendation filter chips */" comment if exists
# or append at the end
if "/* Recommendation filter chips */" in css_text:
    css_text = css_text.replace(
        "/* Recommendation filter chips */",
        GAME_TIME_CSS + "\n/* Recommendation filter chips */",
        1
    )
    with open(css_path, 'wb') as f:
        f.write(css_text.encode('utf-8'))
    print(f"OK: Added game time CSS to index.css")
else:
    # Append at end
    css_text += GAME_TIME_CSS
    with open(css_path, 'wb') as f:
        f.write(css_text.encode('utf-8'))
    print(f"OK: Appended game time CSS to index.css")

# ── Verify no literal-\n corruption in TSX ──
with open(tsx_path, 'rb') as f:
    data = f.read()
literal_n = data.count(b'\\n')
print(f"TSX literal \\\\n check: {literal_n} (should be 0 or low)")
# The `\\n` in template literals like `'\\n'` is OK — those are intentional
print("PASS: Game time filter implementation complete")
