#!/usr/bin/env python3
"""Replay Marklane's bounded real-executable visual/protocol scenarios.

python3 tests/ui/visual/run.py --tool /path/to/tui-test --binary /path/to/marklane \
    --output target/visual-baseline --keyboard baseline

Use --keyboard enhanced for the terminal fix; --suite captures lets the UI
agent inspect its own executable. Output must be a NEW directory. No original
fixture is modified, no OS clipboard keys are used, and no GUI is opened.
"""

import argparse
from pathlib import Path
import re
import shutil
import sys

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[3]
HELPER = ROOT / "tools/tui-test/session.py"
if not HELPER.exists():
    HELPER = Path(__file__).with_name("session.py")  # Saved artifact replay bundle.
sys.path.insert(0, str(HELPER.parent))
from session import Session, write_json  # noqa: E402


def match_number(session):
    text = session.state()["text"]
    result = re.search(r"\b([123])\s*/\s*3\b", text)
    session.check(result is not None, "Visible count reports exactly three results", text=text)
    return int(result.group(1))


def expect_match(session, expected, reason):
    actual = match_number(session)
    session.check(actual == expected, reason, expected=expected, actual=actual)


def find_cells(cells, word):
    for y in sorted({c["y"] for c in cells}):
        row = sorted((c for c in cells if c["y"] == y), key=lambda c: c["x"])
        for index in range(len(row) - len(word) + 1):
            chunk = row[index:index + len(word)]
            if "".join(c["char"] for c in chunk) == word:
                yield chunk


def click_word(session, word):
    chunks = list(find_cells(session.cells(), word))
    session.check(len(chunks) == 1, "Unambiguous visible click target: " + word)
    c = chunks[0][len(word) // 2]
    session.call("mouse", "click", c["x"], c["y"])
    session.settle()


def check_matches(session):
    cells = session.cells()
    # Identify document occurrences by fixture context. The Find field contains
    # the same query and may have its own background or selection styling.
    chunks = []
    for phrase in ["probe first", "probe second", "probe third"]:
        occurrences = list(find_cells(cells, phrase))
        session.check(len(occurrences) == 1, "Visible document match: " + phrase)
        chunks.append(occurrences[0][:len("probe")])
    active = [v for v in chunks if all(c["bold"] and c["underline"] and c["bg"] != "default" for c in v)]
    passive = [v for v in chunks if all(c["bg"] != "default" and not c["underline"] for c in v)]
    session.check(len(active) == 1 and len(passive) == 2,
                  "Actual cells distinguish one active and two passive match backgrounds",
                  active=len(active), passive=len(passive))
    session.check(active[0][0]["bg"] != passive[0][0]["bg"], "Active/passive colors differ")


def open_find(session):
    session.key("Ctrl+f")
    session.call("type", "probe")
    session.settle()
    expect_match(session, 1, "Find starts on first of three matches")


def captures(session):
    state, cells = session.capture("01-idle-80x24")
    session.check(state["cursor"]["visible"] and state["modes"]["alternate_screen"]
                  and state["modes"]["bracketed_paste"], "App initializes cursor, alternate screen and paste mode")
    session.check(any(c["fg"] != "default" for c in cells), "Application emits real colored cells")
    session.check(any(c["italic"] for c in cells), "Markdown emphasis reaches terminal cells")
    session.check(any(c["bold"] and c["y"] > 0 for c in cells), "Markdown strong text reaches terminal cells")
    wide = next(c for c in cells if c["char"] == "界")
    face = next(c for c in cells if c["char"] == "面")
    session.check(face["x"] == wide["x"] + 2 and face["y"] == wide["y"], "CJK glyphs occupy two cells")
    session.key("Ctrl+f")
    session.capture("02-empty-find-80x24")
    session.call("type", "probe")
    expect_match(session, 1, "Typed query is visible")
    check_matches(session)
    session.capture("03-find-80x24")
    click_word(session, "[Next]")
    expect_match(session, 2, "Rendered Next control advances")
    click_word(session, "[Prev]")
    expect_match(session, 1, "Rendered Prev control goes backward")
    session.key("Ctrl+r")
    session.capture("04-replace-80x24")
    session.key("Tab")
    session.call("type", "replacement")
    session.capture("05-replace-with-focused-80x24")
    session.call("resize", 42, 16)
    session.capture("06-replace-42x16")
    for cols, rows in [(120, 36), (160, 48)]:
        session.call("resize", cols, rows)
        session.capture(f"07-replace-{cols}x{rows}")
        session.key("Escape")
        session.key("Ctrl+Home")
        session.capture(f"08-idle-{cols}x{rows}")
        session.key("Ctrl+f")
        session.capture(f"09-find-{cols}x{rows}")
        session.key("Ctrl+r")
    session.key("Escape")
    session.key("Ctrl+Home")
    session.call("resize", 80, 24)
    session.key("Shift+Right")
    session.key("Shift+Right")
    _, cells = session.capture("10-source-selection-80x24")
    session.check(any(c["inverse"] and c["y"] > 0 for c in cells), "Source selection reaches reversed cells")
    session.check(not any(c["png_error"] for c in session.captures), "Simplified fixture exports all native PNGs")


def protocol(session, expected):
    open_find(session)
    session.key("Enter")
    expect_match(session, 2, "Plain Enter advances to result two")
    activation = b"\x1b[>1u" in session.traffic("READ")
    session.check(activation == (expected == "enhanced"), "Observed application keyboard activation matches profile",
                  requested=expected, activated=activation)
    offset = session.log.stat().st_size
    session.key("Shift+Enter")
    actual_bytes = session.traffic("WRITE", offset)
    actual_match = match_number(session)
    report = {"expected_profile": expected, "app_activation_observed": activation,
              "backend_shift_enter_hex": actual_bytes.hex(), "backend_shift_enter_repr": repr(actual_bytes),
              "result_after_backend_shift_enter": actual_match,
              "installed_ghostty_1_3_1_default_hex": b"\x1b[27;2;13~".hex(),
              "backend_default_differs_from_ghostty_1_3_1": actual_bytes != b"\x1b[27;2;13~" if expected == "baseline" else None,
              "note": "Only backend-generated input after observed activation proves negotiation. Raw cases below are parser regressions."}
    write_json(session.output / "protocol.json", report)
    if expected == "enhanced":
        session.check(actual_bytes == b"\x1b[13;2u" and actual_match == 1,
                      "Backend encoder responds to app-requested enhancement and selects previous", **report)
    else:
        known_results = {b"\x1b[27;2;13~": 2, b"\r": 3, b"\x1b[13;2u": 1}
        session.check(actual_bytes in known_results and actual_match == known_results.get(actual_bytes),
                      "Baseline backend bytes and app result are recorded, without assuming Ghostty equivalence", **report)
    session.capture("01-backend-shift-enter")
    for _ in range(3):
        if match_number(session) == 2:
            break
        session.key("F3")
    expect_match(session, 2, "Reset to result two for raw-byte profiles")
    session.raw("\x1b[27;2;13~")
    expect_match(session, 2, "Raw Ghostty 1.3.1 default ShiftEnter is ignored by Crossterm 0.28")
    session.capture("02-raw-ghostty-default-noop")
    session.raw("\x1b[13;2u")
    expect_match(session, 1, "Raw enhanced CSI-u parser regression selects previous")
    session.key("F3")
    expect_match(session, 2, "Reset to result two for legacy CR")
    session.raw("\r")
    expect_match(session, 3, "Raw legacy CR is next, a separate profile")
    session.key("Shift+F3")
    expect_match(session, 2, "Legacy previous binding remains usable")
    session.capture("03-legacy-previous-fallback")


def unicode_and_edits(session):
    state, cells = session.capture("01-original-unicode")
    session.check(any(c["char"] == "e\u0301" for c in cells), "Combining sequence retained in actual cells")
    session.check("👩🏽\u200d💻".encode() in session.traffic("READ"), "Application emits complete original ZWJ bytes")
    write_json(session.output / "unicode-rendering-limitations.json", {
        "full_visual_acceptance": False,
        "combining_cell_preserved": True,
        "zwj_cluster_in_single_cell": any(c["char"] == "👩🏽\u200d💻" for c in cells),
        "dec_2027_grapheme_mode_requested": b"\x1b[?2027h" in session.traffic("READ"),
        "observation": "Native PNG rejects combining cells. Raw ZWJ bytes survive, but backend cells split the emoji and app cursor positioning overlaps it. SVG conversion cannot repair cell state.",
    })
    session.key("Ctrl+Home")
    click_word(session, "☐")
    session.key("Ctrl+s")
    toggled = session.original_bytes.replace(b"- [ ] Toggle", b"- [x] Toggle")
    session.check(session.fixture.read_bytes() == toggled, "Checkbox click changes only intended source marker")
    session.capture("02-checkbox-toggled")
    session.key("Ctrl+z")
    session.key("Ctrl+s")
    session.check(session.fixture.read_bytes() == session.original_bytes, "Checkbox undo restores exact Unicode source")
    session.key("Ctrl+End")
    session.paste("\nPASTE α\nsecond line 界\n")
    session.key("Ctrl+s")
    session.check(session.fixture.read_bytes() == session.original_bytes + "\nPASTE α\nsecond line 界\n".encode("utf-8"),
                  "Mode-aware multiline paste preserves exact bytes in saved scratch source")
    session.capture("03-multiline-paste")
    session.key("Ctrl+z")
    session.key("Ctrl+s")
    session.check(session.fixture.read_bytes() == session.original_bytes, "One undo restores source before paste")
    session.key("Ctrl+Home")
    before = session.state()["text"]
    session.wheel(10, 8, "down", 2)
    session.check(session.state()["text"] != before, "Wheel at body cell scrolls the document")
    session.capture("04-wheel-at-body")


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--tool", required=True)
    parser.add_argument("--binary", required=True)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--keyboard", choices=["baseline", "enhanced"], default="baseline")
    parser.add_argument("--suite", choices=["all", "captures", "protocol", "unicode"], default="all")
    parser.add_argument("--palette", choices=["dark", "light"], default="dark")
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=False)
    shutil.copyfile(__file__, args.output / "scenario.py")
    shutil.copyfile(HELPER, args.output / "session.py")
    for fixture in ["acceptance.md", "screenshots.md"]:
        shutil.copyfile(Path(__file__).with_name(fixture), args.output / fixture)
    write_json(args.output / "invocation.json", vars(args) | {"output": str(args.output.resolve())})
    suites = ["captures", "protocol", "unicode"] if args.suite == "all" else [args.suite]
    for suite in suites:
        fixture = "acceptance.md" if suite == "unicode" else "screenshots.md"
        session = Session(args.tool, args.binary, args.output / suite, Path(__file__).with_name(fixture), args.palette)
        try:
            session.start()
            session.call("expect", "text", "Terminal acceptance", "--timeout", 3000)
            if suite == "captures":
                captures(session)
            elif suite == "protocol":
                protocol(session, args.keyboard)
            else:
                unicode_and_edits(session)
            session.key("Escape")
            session.key("Ctrl+q")
            output = session.traffic("READ")
            if args.keyboard == "enhanced":
                session.check(output.count(b"\x1b[<1u") == 1 and output.index(b"\x1b[<1u") < output.index(b"\x1b[?1049l"),
                              "Graceful exit pops keyboard enhancement once before leaving alternate screen")
            write_json(session.output / "result.json", {
                "assertions_passed": True, "suite": suite,
                "full_renderer_acceptance": False,
                "png_failures": [c["name"] for c in session.captures if c["png_error"]],
                "scope": "Simplified visual fixture and protocol/source checks; Unicode rendering limits remain.",
            })
            print(f"PASS {suite} assertions (renderer limitations remain): {session.output}", flush=True)
        except Exception as error:
            write_json(session.output / "result.json", {"passed": False, "error": str(error), "suite": suite})
            raise
        finally:
            session.close()


if __name__ == "__main__":
    main()
