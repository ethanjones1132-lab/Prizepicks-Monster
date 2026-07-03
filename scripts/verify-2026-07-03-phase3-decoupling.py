#!/usr/bin/env python3
"""Ad-hoc verification: PRIORITIES.md + ROADMAP.md + source updates for the
2026-07-03 Phase 3 cache decoupling pass. NOT a substitute for the canonical
test suite (which already passed: 290/290 lib tests, cargo check clean, tsc clean).
"""
import os
import re
import sys

ROOT = r"C:\Projects\prizepicks-monster"
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


# Read all source files
with open(os.path.join(ROOT, "PRIORITIES.md"), "r", encoding="utf-8") as f:
    p = f.read()
with open(os.path.join(ROOT, "ROADMAP.md"), "r", encoding="utf-8") as f:
    r = f.read()
with open(os.path.join(ROOT, "src-tauri/src/prizepicks/client.rs"), "r", encoding="utf-8") as f:
    c = f.read()
with open(os.path.join(ROOT, "src-tauri/src/commands/prizepicks_cmd.rs"), "r", encoding="utf-8") as f:
    cmd = f.read()
with open(os.path.join(ROOT, "src-tauri/src/lib.rs"), "r", encoding="utf-8") as f:
    lib = f.read()

# 1. PRIORITIES.md updates
check("PRIORITIES.md has 'Phase 3 cache decoupling done' in Quick status",
      "Phase 3 cache decoupling done" in p)
check("PRIORITIES.md last-updated line mentions Phase 3",
      "Phase 3 cache decoupling shipped" in p or
      "Phase 3 dashboard cache decoupling shipped" in p)
check("PRIORITIES.md has 'Brainstormed & shipped (2026-07-03 afternoon)' section",
      "Brainstormed & shipped (2026-07-03 afternoon)" in p)
check("PRIORITIES.md mentions Arc<RwLock<Option<PrizePicksCache>>>",
      "Arc<RwLock<Option<PrizePicksCache>>>" in p or "Arc<RwLock" in p)
check("PRIORITIES.md mentions AtomicBool fetch guard",
      "AtomicBool" in p)
check("PRIORITIES.md mentions 290 lib tests",
      "290" in p and ("lib tests" in p or "tests pass" in p))
check("PRIORITIES.md mentions 15 new tests",
      "+15 new" in p or "15 new" in p)
check("PRIORITIES.md no literal \\n corruption in new section",
      r"\\n" not in p.split("Brainstormed & shipped (2026-07-03 afternoon)")[-1])

# 2. ROADMAP.md updates
check("ROADMAP.md Phase 3 decoupling row updated to Done",
      "Done 2026-07-03" in r and "Decouple cache reads from long fetches" in r)
check("ROADMAP.md Next Actionable Items #1 struck through",
      "~~**Complete Phase 3 decoupling**~~" in r)
check("ROADMAP.md has 'Last updated: 2026-07-03'", "Last updated: 2026-07-03" in r)

# 3. client.rs source changes
check("client.rs cache field is Arc<RwLock<Option<PrizePicksCache>>>",
      "Arc<RwLock<Option<PrizePicksCache>>>" in c)
check("client.rs has fetch_in_progress: Arc<AtomicBool>",
      "fetch_in_progress: Arc<AtomicBool>" in c)
check("client.rs has try_begin_fetch", "fn try_begin_fetch" in c)
check("client.rs has end_fetch", "fn end_fetch" in c)
check("client.rs has wait_for_in_flight_fetch", "fn wait_for_in_flight_fetch" in c)
check("client.rs has ≥ 15 new tests", c.count("#[tokio::test]") >= 15,
      f"found {c.count('#[tokio::test]')}")

# 4. prizepicks_cmd.rs callsite fixes
check("prizepicks_cmd.rs invalidate_cache has .await",
      ".invalidate_cache().await" in cmd)
check("prizepicks_cmd.rs cache_status has .await",
      ".cache_status().await" in cmd)
check("prizepicks_cmd.rs category_stats has .await",
      ".category_stats().await" in cmd)
check("prizepicks_cmd.rs has 0 'let mut client' in read-only commands",
      "let mut client = prizepicks.lock().await;\n    client.get_markets_by_category" not in cmd
      and "let mut client = prizepicks.lock().await;\n    client.search_markets" not in cmd
      and "let mut client = prizepicks.lock().await;\n    client.get_top_markets" not in cmd
      and "let mut client = prizepicks.lock().await;\n    client.cache_status" not in cmd
      and "let mut client = prizepicks.lock().await;\n    client.category_stats" not in cmd
      and "let mut client = prizepicks.lock().await;\n    client.fetch_all_markets" not in cmd)

# 5. lib.rs startup-warm callsite fix
check("lib.rs startup warm has .await on needs_full_catalog",
      ".needs_full_catalog().await" in lib)

# 6. Quick sanity
check("PRIORITIES.md has ≥ 25 '## ' section headers", p.count("## ") >= 25)

print()
print(f"PASSED: {passed} / {passed + len(errors)}")
if errors:
    print("FAILED:")
    for e in errors:
        print(f"  - {e}")
    sys.exit(1)
sys.exit(0)
