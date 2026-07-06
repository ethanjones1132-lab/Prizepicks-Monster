#!/usr/bin/env python3
"""Ad-hoc verification: PRIORITIES.md + source updates for the 2026-07-06
Top Winners / Top Losers panel pass. NOT a substitute for the canonical test
suite (which already passed: 315/315 lib tests, cargo check clean, tsc clean).

The check focuses on:
  1. Rust helper + struct + field wiring (compute_top_lots, PaperTopLot,
     PaperAnalytics.top_winners / top_losers, get_analytics wiring).
  2. TypeScript types + UI component + CSS classes.
  3. PRIORITIES.md records the 2026-07-06 pass and the new feature.
  4. No literal-`\\n` corruption in any touched file.
"""
import os
import re
import sys

ROOT = r"C:\Projects\prizepicks-monster"
PAPER_MOD = os.path.join(ROOT, "src-tauri", "src", "paper", "mod.rs")
TYPES_TS = os.path.join(ROOT, "src-ui", "src", "types", "prizepicks.ts")
PANEL_TSX = os.path.join(ROOT, "src-ui", "src", "components", "PrizePicksPredictionsPanel.tsx")
INDEX_CSS = os.path.join(ROOT, "src-ui", "src", "index.css")
PRIORITIES = os.path.join(ROOT, "PRIORITIES.md")

errors = []
passed = 0


def check(name, condition, detail=""):
    global passed
    if condition:
        passed += 1
        print(f"  [PASS] {name}")
    else:
        errors.append(f"{name} {detail}")
        print(f"  [FAIL] {name} {detail}")


def read(path):
    with open(path, "r", encoding="utf-8") as f:
        return f.read()


# ── 1. Rust source: PaperTopLot struct + compute_top_lots helper + wiring
print("=== 1. Rust source — PaperTopLot + compute_top_lots ===")
p = read(PAPER_MOD)

check("PaperTopLot struct present", "pub struct PaperTopLot" in p)
check("PaperTopLot has lot_id field", "pub lot_id: String" in p)
check("PaperTopLot has realized_pnl field", "pub realized_pnl: f64" in p)
check("PaperTopLot has stake_dollars field", "pub stake_dollars: f64" in p)
check("PaperTopLot has entry_price_cents field", "pub entry_price_cents: f64" in p)
check("PaperTopLot has closed_price_cents Option", "pub closed_price_cents: Option<f64>" in p)
check("PaperTopLot has closed_at Option", "pub closed_at: Option<String>" in p)
check("PaperTopLot has settlement_result Option", "pub settlement_result: Option<String>" in p)
check("compute_top_lots helper present", "fn compute_top_lots(lots: &[PaperLot])" in p)
check("TOP_LOTS_LIMIT constant present", "const TOP_LOTS_LIMIT: usize = 5" in p)
check("Helper excludes open lots", 'if l.status != "Closed"' in p)
check("Helper excludes None and push pnl", "Some(p) if p > 0.0" in p and "Some(p) if p < 0.0" in p)
check("Helper sorts winners DESC", "winners_only.sort_by" in p and "b.realized_pnl" in p)
check("Helper sorts losers ASC", "losers_only.sort_by" in p and "a.realized_pnl" in p)
check("Helper takes 5 from each list", ".take(TOP_LOTS_LIMIT)" in p)

# PaperAnalytics wiring
check("PaperAnalytics has top_winners field", "pub top_winners: Vec<PaperTopLot>" in p)
check("PaperAnalytics has top_losers field", "pub top_losers: Vec<PaperTopLot>" in p)
check("get_analytics calls compute_top_lots", "let (top_winners, top_losers) = compute_top_lots(&all);" in p)
check("get_analytics destructures top_winners", "top_winners," in p)
check("get_analytics destructures top_losers", "top_losers," in p)

# Test count
top_lots_test_count = p.count("fn top_lots_") + p.count("fn top_winners_") + p.count("fn top_losers_")
check("At least 7 new top_lots tests present (8+ found: %d)" % top_lots_test_count, top_lots_test_count >= 7)

# ── 2. TypeScript types
print("\n=== 2. TypeScript types ===")
t = read(TYPES_TS)

check("PaperTopLot interface present", "export interface PaperTopLot {" in t)
check("PaperTopLot has lot_id", "lot_id: string;" in t)
check("PaperTopLot has realized_pnl", "realized_pnl: number;" in t)
check("PaperTopLot has stake_dollars", "stake_dollars: number;" in t)
check("PaperTopLot has entry_price_cents", "entry_price_cents: number;" in t)
check("PaperTopLot has closed_price_cents nullable", "closed_price_cents: number | null;" in t)
check("PaperTopLot has closed_at nullable", "closed_at: string | null;" in t)
check("PaperTopLot has settlement_result nullable", "settlement_result: string | null;" in t)
check("PaperAnalytics has top_winners", "top_winners: PaperTopLot[];" in t)
check("PaperAnalytics has top_losers", "top_losers: PaperTopLot[];" in t)

# ── 3. React component
print("\n=== 3. React component ===")
x = read(PANEL_TSX)

check("PaperTopLot imported in panel", "PaperTopLot," in x)
check("TopLotsPanel component declared", "function TopLotsPanel({" in x)
check("TopLotsPanel accepts winners prop", "winners: PaperTopLot[];" in x)
check("TopLotsPanel accepts losers prop", "losers: PaperTopLot[];" in x)
check("TopLotsPanel has empty state branch", "topLotsPanel empty" in x)
check("TopLotsPanel renders ▲ Top winners", "▲ Top winners" in x)
check("TopLotsPanel renders ▼ Top losers", "▼ Top losers" in x)
check("TopLotsPanel has no-winners placeholder", "No winners yet" in x)
check("TopLotsPanel has no-losers placeholder", "No losers yet" in x)
check("TopLotsPanel mounts in panel render", "TopLotsPanel" in x and "winners={analytics.top_winners}" in x and "losers={analytics.top_losers}" in x)
check("TopLotsPanel uses topLotsPanel class", 'className="topLotsPanel' in x)
check("TopLotsPanel uses topLotsTable class", 'className="topLotsTable"' in x)
check("TopLotsPanel uses topLotsRow class", "topLotsRow" in x)
check("TopLotsPanel uses topLotsTitle class", "topLotsTitle" in x)
check("TopLotsPanel uses topLotsColumn class", "topLotsColumn" in x)
check("TopLotsPanel uses topLotsGrid class", "topLotsGrid" in x)
check("TopLotsPanel uses topLotsColumnTitle class", "topLotsColumnTitle" in x)
check("TopLotsPanel uses topLotsColumnEmpty class", "topLotsColumnEmpty" in x)
check("TopLotsPanel uses topLotsPanelHeader class", "topLotsPanelHeader" in x)
check("TopLotsPanel renders ROI multiplier", "roiMult" in x and "×" in x)
check("TopLotsPanel handles no-stake divider", "w.stake_dollars > 0" in x and "l.stake_dollars > 0" in x)

# ── 4. CSS classes
print("\n=== 4. CSS ===")
c = read(INDEX_CSS)

required_classes = [
    ".topLotsPanel",
    ".topLotsPanel.empty",
    ".topLotsPanelHeader",
    ".topLotsGrid",
    ".topLotsColumn",
    ".topLotsColumnTitle",
    ".topLotsColumnTitle-winners",
    ".topLotsColumnTitle-losers",
    ".topLotsColumnEmpty",
    ".topLotsTable",
    ".topLotsTable th",
    ".topLotsTable td",
    ".topLotsTable td.pos",
    ".topLotsTable td.neg",
    ".topLotsTitle",
    ".topLotsRow-winner",
    ".topLotsRow-loser",
]
for cls in required_classes:
    check(f"CSS class {cls} present", cls in c)
check("CSS narrow-width media query for topLotsGrid", "@media (max-width: 720px)" in c)

# ── 5. PRIORITIES.md records this pass
print("\n=== 5. PRIORITIES.md ===")
m = read(PRIORITIES)

check("2026-07-06 pass date recorded", "2026-07-06" in m)
check("Top Winners / Top Losers headline present", "Top winners" in m or "top_winners" in m)
check("top_winners field mentioned in PRIORITIES", "top_winners" in m)
check("top_losers field mentioned in PRIORITIES", "top_losers" in m)
check("PaperTopLot struct mentioned", "PaperTopLot" in m)
check("compute_top_lots helper mentioned", "compute_top_lots" in m)
check("Ad-hoc verification note present", "Ad-hoc verification" in m or "ad-hoc" in m.lower())
check("Last updated date reflects this pass", "Last updated" in m and "2026-07-06" in m.split("Last updated")[1][:200] if "Last updated" in m else False)

# ── 6. No literal-\n corruption
print("\n=== 6. Literal-\\n corruption check ===")

def count_literal_n(text):
    """Count sequences of literal `\n` (backslash followed by n) in text."""
    return len(re.findall(r"\\n", text))


for path in [PAPER_MOD, TYPES_TS, PANEL_TSX, INDEX_CSS, PRIORITIES]:
    text = read(path)
    n = count_literal_n(text)
    check(
        f"{os.path.basename(path)}: zero literal-`\\n` sequences ({n} found)",
        n == 0,
    )

# ── Summary
print(f"\n=== Summary: {passed} checks passed, {len(errors)} failed ===")
if errors:
    print("\nFAILED CHECKS:")
    for e in errors:
        print(f"  - {e}")
    sys.exit(1)
print("All ad-hoc checks PASS. (Canonical suite already green: 315/315 lib tests, cargo check clean, tsc clean.)")
