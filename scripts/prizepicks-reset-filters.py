#!/usr/bin/env python3
"""Insert clear-all-filters functionality into PrizePicksView.tsx.

Adds:
1. A `hasActiveFilters` getter + `resetFilters()` callback after collapsedGames state
2. A "\u21ba Reset" button in the section header when any filter is non-default
3. CSS rules for the reset button in index.css

Safe binary-mode editing to avoid line-ending corruption.
"""
import sys
import os

PROJECT = r'C:\Projects\prizepicks-monster'

def main():
    tsx_path = os.path.join(PROJECT, 'src-ui', 'src', 'components', 'PrizePicksView.tsx')
    css_path = os.path.join(PROJECT, 'src-ui', 'src', 'index.css')

    # ---- Phase 1: Edit TSX -------------------------------------------------
    with open(tsx_path, 'rb') as f:
        tsx = f.read()

    nl = b'\r\n'

    # 1a. Insert hasActiveFilters + resetFilters after collapsedGames line
    anchor1 = b"  const [collapsedGames, setCollapsedGames] = useState<Record<string, boolean>>(loadCollapsed);" + nl
    insert1 = (
        "  // True when any filter control is set to a non-default value\r\n"
        "  const hasActiveFilters = sortKey !== DEFAULT_PREFERENCES.sortKey || sortDir !== DEFAULT_PREFERENCES.sortDir || minEdge > 0 || selectedCategory !== 'All' || selectedTeam !== 'All' || playerFilter !== '';\r\n"
        "\r\n"
        "  const resetFilters = () => {\r\n"
        "    setSortKey(DEFAULT_PREFERENCES.sortKey);\r\n"
        "    setSortDir(DEFAULT_PREFERENCES.sortDir);\r\n"
        "    setMinEdge(0);\r\n"
        "    setSelectedCategory('All');\r\n"
        "    setSelectedTeam('All');\r\n"
        "    setPlayerFilter('');\r\n"
        "  };\r\n"
        "\r\n"
    ).encode('utf-8')

    if anchor1 not in tsx:
        print("FAIL: anchor1 not found in TSX", file=sys.stderr)
        sys.exit(1)
    tsx = tsx.replace(anchor1, anchor1 + insert1, 1)

    # 1b. Insert reset button after the CSV export button (before </h3>)
    csv_emoji = '\U0001f4e5'.encode('utf-8')
    anchor2 = b"              " + csv_emoji + b" CSV" + nl + b"            </button>" + nl + b"          </h3>"
    insert2 = (
        "              \U0001f4e5 CSV\r\n"
        "            </button>\r\n"
        "            {hasActiveFilters && (\r\n"
        "              <button\r\n"
        '                type="button"\r\n'
        '                className="resetFiltersBtn"\r\n'
        "                onClick={resetFilters}\r\n"
        '                title="Reset all filters to defaults"\r\n'
        '                aria-label="Reset all filters"\r\n'
        "              >\r\n"
        "                \u21ba Reset\r\n"
        "              </button>\r\n"
        "            )}\r\n"
        "          </h3>"
    ).encode('utf-8')

    if anchor2 not in tsx:
        print("FAIL: anchor2 not found in TSX", file=sys.stderr)
        sys.exit(1)
    tsx = tsx.replace(anchor2, insert2, 1)

    with open(tsx_path, 'wb') as f:
        f.write(tsx)
    print("PrizePicksView.tsx updated")

    # ---- Phase 2: Edit CSS -------------------------------------------------
    with open(css_path, 'rb') as f:
        css = f.read()

    # Insert between .minEdgeInput:focus closing } and .playerFilter opening
    # Anchor: the closing of .minEdgeInput:focus
    anchor3 = b"  border-color: rgba(110, 200, 163, 0.5);\r\n}\r\n\r\n.playerFilter {"
    insert3 = (
        "  border-color: rgba(110, 200, 163, 0.5);\r\n"
        "}\r\n"
        "\r\n"
        "/* Reset filters button -- appears in the section header when any filter is non-default */\r\n"
        ".resetFiltersBtn {\r\n"
        "  background: none;\r\n"
        "  border: 1px solid rgba(255, 255, 255, 0.15);\r\n"
        "  color: rgba(255, 255, 255, 0.7);\r\n"
        "  font: inherit;\r\n"
        "  font-size: 0.75rem;\r\n"
        "  cursor: pointer;\r\n"
        "  padding: 2px 10px;\r\n"
        "  border-radius: 4px;\r\n"
        "  margin-left: 10px;\r\n"
        "  transition: border-color 0.15s, color 0.15s, background 0.15s;\r\n"
        "  white-space: nowrap;\r\n"
        "}\r\n"
        ".resetFiltersBtn:hover {\r\n"
        "  border-color: #e5a060;\r\n"
        "  color: #e5a060;\r\n"
        "  background: rgba(229, 160, 96, 0.08);\r\n"
        "}\r\n"
        ".resetFiltersBtn:active {\r\n"
        "  background: rgba(229, 160, 96, 0.15);\r\n"
        "}\r\n"
        "\r\n"
        ".playerFilter {"
    ).encode('utf-8')

    if anchor3 not in css:
        print("FAIL: anchor3 not found in CSS", file=sys.stderr)
        idx = css.find(b".playerFilter {")
        if idx >= 0:
            before = css[max(0, idx-100):idx]
            print(f"  Found .playerFilter at byte {idx}, before: {repr(before)}", file=sys.stderr)
            # Let me see the exact content
            print(f"  Looking for: {repr(anchor3[:100])}", file=sys.stderr)
        sys.exit(1)
    css = css.replace(anchor3, insert3, 1)

    with open(css_path, 'wb') as f:
        f.write(css)
    print("index.css updated")

    # ---- Phase 3: Verify no corruption ------------------------------------
    for path, label in [(tsx_path, 'PrizePicksView.tsx'), (css_path, 'index.css')]:
        with open(path, 'rb') as f:
            content = f.read()
        lines = content.count(b'\n')
        crlf = content.count(b'\r\n')
        lf_only = lines - crlf
        literal_bs_n = 0
        i = 0
        while i < len(content) - 1:
            if content[i] == 0x5C and content[i+1] == 0x6E:
                literal_bs_n += 1
                i += 2
            else:
                i += 1
        print(f"  {label}: {lines} lines, {crlf} CRLF, {lf_only} LF-only, literal \\\\n = {literal_bs_n}")
        if literal_bs_n > lines * 1.5:
            print(f"  CORRUPTION DETECTED in {label}!", file=sys.stderr)
            sys.exit(1)

    print("No file-level corruption detected")
    print("Done -- run health checks next")


if __name__ == '__main__':
    main()
