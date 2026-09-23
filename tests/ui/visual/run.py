#!/usr/bin/env python3
"""Replay Eymi's bounded real-executable visual/protocol scenarios.

python3 tests/ui/visual/run.py --tool /path/to/tui-test --binary /path/to/eymi \
    --output target/visual-baseline --keyboard baseline

Use --keyboard enhanced for the terminal fix; --suite captures lets the UI
agent inspect its own executable. Output must be a NEW directory. No original
fixture is modified, no OS clipboard keys are used, and no GUI is opened.
"""

import argparse
import json
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


def find_cells(cells, word, casefold=False):
    for y in sorted({c["y"] for c in cells}):
        row = sorted((c for c in cells if c["y"] == y), key=lambda c: c["x"])
        for index in range(len(row) - len(word) + 1):
            chunk = row[index:index + len(word)]
            text = "".join(c["char"] for c in chunk)
            matches = text.casefold() == word.casefold() if casefold else text == word
            if matches:
                yield chunk


def selection_visible(before, after):
    """A selection is visible when text cells gain a background of their own."""
    old = {(c["x"], c["y"]): c["bg"] for c in before}
    return any(c["y"] > 0 and c["char"].strip() and old.get((c["x"], c["y"])) != c["bg"]
               for c in after)


def highlighted(cells, word, page):
    """Every cell of the visible word sits on a surface other than the page."""
    return any(all(c["bg"].lower() != page.lower() for c in chunk) for chunk in find_cells(cells, word))


def click_button(session, word):
    """Click a search panel button; the footer may repeat its label as guidance."""
    cells = session.cells()
    footer = max(c["y"] for c in cells)
    chunks = [chunk for chunk in find_cells(cells, word) if chunk[0]["y"] < footer]
    session.check(len(chunks) == 1, "Unambiguous visible button: " + word)
    c = chunks[0][len(word) // 2]
    session.call("mouse", "click", c["x"], c["y"])
    session.settle()


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
    click_button(session, "Next")
    expect_match(session, 2, "Rendered Next control advances")
    click_button(session, "Prev")
    expect_match(session, 1, "Rendered Prev control goes backward")
    session.key("Ctrl+r")
    session.capture("04-replace-80x24")
    session.key("Tab")
    session.call("type", "replacement")
    session.capture("05-replace-with-focused-80x24")
    session.call("resize", 42, 16)
    session.capture("06-replace-42x16")
    for cols, rows in [(120, 36), (160, 45)]:
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
    before = session.cells()
    session.key("Shift+Right")
    session.key("Shift+Right")
    _, cells = session.capture("10-source-selection-80x24")
    session.check(selection_visible(before, cells), "Source selection reaches highlighted cells")
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
    click_word(session, "□")
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


def workspace(session):
    """Exercise the new workspace and editing controls through the real PTY."""
    source = session.original_source
    state, cells = session.capture("01-writing-160x45")
    session.check("DOCUMENTS" not in state["text"] and bool(outline_labels(cells)),
                  "Wide workspace exposes a heading-only outline")
    heading = "Later, with confidence"
    target = outline_target(session, heading, cells)[1]
    session.call("mouse", "click", target["x"], target["y"])
    heading_line = source.splitlines().index("## " + heading) + 1
    session.check(f"Ln {heading_line}, Col 1" in session.state()["text"],
                  "Outline pointer jumps to the exact source heading")
    session.capture("02-outline-navigation")
    session.key("F9")
    session.key("End")
    session.key("Enter")
    session.check(f"Ln {heading_line}, Col 1" in session.state()["text"],
                  "Sidebar is also keyboard accessible")

    session.key("Ctrl+g")
    session.call("type", "3")
    session.capture("03-go-to-line")
    session.key("Enter")
    session.check("Ln 3, Col 1" in session.state()["text"], "Go to line uses source lines")

    def command(query):
        session.key("F2")
        session.call("type", query)
        session.settle()
        session.key("Enter")

    def save_expected(expected, message):
        session.key("Ctrl+s")
        session.check(session.fixture.read_text() == expected, message)

    session.key("F2")
    session.call("type", "theme")
    session.capture("04-filtered-command-palette")
    session.key("Escape")
    background = next(c["bg"] for c in session.cells() if (c["x"], c["y"]) == (159, 3))
    command("theme")
    session.call("type", "Sage Light" if "light" not in session.app_args else "Sage Dark")
    session.settle()
    session.capture("05-theme-picker-preview")
    session.key("Enter")
    changed = next(c["bg"] for c in session.cells() if (c["x"], c["y"]) == (159, 3))
    session.check(changed != background, "Theme command changes actual document cell colors")
    session.capture("05-opposite-theme")
    command("theme")
    session.call("type", "Sage Light" if "light" in session.app_args else "Sage Dark")
    session.settle()
    session.key("Enter")

    session.key("Ctrl+Shift+Right")
    command("Format bold")
    save_expected(source.replace("Write", "**Write**", 1), "Palette formatting edits selected source")
    session.capture("06-formatting-selection")
    session.key("Ctrl+z")
    save_expected(source, "One undo restores formatting and original source")
    session.key("Ctrl+]")
    save_expected(source.replace("Write", "    Write", 1), "Indent command edits the selected source line")
    session.key("Ctrl+z")
    save_expected(source, "Indent is one undo step")

    session.key("Ctrl+End")
    session.call("type", "draft")
    session.key("Ctrl+z")
    save_expected(source, "Contiguous keyboard typing undoes as a group")
    session.call("type", "first")
    session.key("F8")
    session.call("type", "second")
    session.key("Ctrl+z")
    save_expected(source + "first", "A one-tab navigation command ends the preceding typing group")
    session.key("Ctrl+z")
    save_expected(source, "The earlier typing group remains separately undoable")
    session.paste("literal\n- [ ] paste\n")
    save_expected(source + "literal\n- [ ] paste\n", "Bracketed paste stays literal")
    session.key("Ctrl+z")
    save_expected(source, "Pasted text remains one undo transaction")

    command("New document")
    session.check("Untitled 2.md" in session.state()["text"], "Palette creates an independent document tab")
    session.key("F7")
    session.key("Ctrl+Home")
    session.capture("07-document-tabs")
    for width, height in [(120, 36), (80, 24), (42, 16), (20, 10)]:
        session.call("resize", width, height)
        session.capture(f"08-workspace-{width}x{height}")
        session.check(bool(outline_labels(session.cells())) == (width >= 110),
                      f"Sidebar adapts to {width}x{height}")

    session.key("F2")
    session.call("resize", 20, 4)
    session.key("Enter")
    session.capture("09-tiny-palette")
    session.key("Escape")
    session.call("resize", 160, 45)
    save_expected(source, "Tiny palette and navigation never modify the document")
    session.key("Ctrl+Home")
    session.capture("10-finished-writing-160x45")
    session.check(not any(c["png_error"] for c in session.captures),
                  "Workspace captures all export as native PNGs")


def outline_labels(cells):
    # Ordinary prose may mention "outline". The rail label sits after decorative
    # padding and before a vertical boundary, unlike a body occurrence.
    labels = []
    for chunk in find_cells(cells, "Outline", casefold=True):
        row = [c for c in cells if c["y"] == chunk[0]["y"]]
        left = [c for c in row if c["x"] < chunk[0]["x"]]
        boundary = any(c["x"] > chunk[-1]["x"] and c["char"] in "│┃┆┊╎▏▕┤┐╮" for c in row)
        if boundary and not any(any(char.isalnum() for char in c["char"]) for c in left):
            labels.append(chunk)
    return labels


def outline_target(session, heading, cells=None):
    """Pick the rail label from actual cells, independently of its current width."""
    cells = cells if cells is not None else session.cells()
    labels = outline_labels(cells)
    session.check(len(labels) == 1, "One visible outline rail label")
    label = labels[0]
    chunks = [chunk for chunk in find_cells(cells, heading)
              if chunk[0]["y"] > label[0]["y"]
              and max(0, label[0]["x"] - 3) <= chunk[0]["x"] <= label[-1]["x"] + 4]
    session.check(len(chunks) == 1, "Unambiguous outline heading: " + heading)
    return chunks[0]


def private_use(char):
    return any(0xE000 <= ord(c) <= 0xF8FF or 0xF0000 <= ord(c) <= 0xFFFFD
               or 0x100000 <= ord(c) <= 0x10FFFD for c in char)


def contrast_ratio(foreground, background):
    def luminance(color):
        if not re.fullmatch(r"#[0-9a-fA-F]{6}", color):
            return None
        values = [int(color[i:i + 2], 16) / 255 for i in (1, 3, 5)]
        linear = [v / 12.92 if v <= 0.04045 else ((v + 0.055) / 1.055) ** 2.4 for v in values]
        return sum(v * weight for v, weight in zip(linear, (0.2126, 0.7152, 0.0722)))
    front, back = luminance(foreground), luminance(background)
    if front is None or back is None:
        return 0
    return (max(front, back) + 0.05) / (min(front, back) + 0.05)


def chrome(session):
    """Accept the quiet one-row workspace using real cells and source transactions."""
    source = session.original_source
    name = session.fixture.name
    title = source.splitlines()[0].removeprefix("# ")
    nerd = "--icons" in session.app_args and session.app_args[session.app_args.index("--icons") + 1] == "nerd"

    def saved_exact(message):
        session.key("Ctrl+s")
        session.check(session.fixture.read_bytes() == session.original_bytes, message)
        # Save feedback intentionally includes its path; dismiss it before any
        # idle-chrome uniqueness checks, preserving the document selection.
        session.key("Escape")

    def idle(label, expect_outline=None, first_line=False):
        state, cells = session.capture(label)
        filenames = list(find_cells(cells, name))
        wordmarks = list(find_cells(cells, "eymi", casefold=True))
        session.check(len(filenames) == 1, "Idle filename appears exactly once: " + label,
                      filename=name, occurrences=len(filenames))
        header_y = filenames[0][0]["y"]
        session.check(not wordmarks, "No wordmark competes with the document for header space: " + label)
        labels = outline_labels(cells)
        if labels:
            session.check(labels[0][0]["y"] == header_y and labels[0][-1]["x"] < filenames[0][0]["x"],
                          "Outline header shares the tab row, left of the tabs: " + label)
        session.check(not re.search(r"\b(?:DOCUMENTS|Live|F1|F2|F9|Ctrl[+-][A-Za-z])\b", state["text"]),
                      "Idle chrome has no document list, Live label, or persistent shortcut labels: " + label)
        if expect_outline is not None:
            session.check(bool(outline_labels(cells)) == expect_outline,
                          "Outline visibility follows available width: " + label)
        if first_line:
            headings = list(find_cells(cells, title))
            session.check(bool(headings), "First document heading is visible: " + label)
            body = max(headings, key=lambda chunk: chunk[0]["x"])
            session.check(body[0]["y"] == header_y + 1,
                          "Document begins immediately below the single header: " + label,
                          header_row=header_y, body_row=body[0]["y"])
        session.check(any(private_use(c["char"]) for c in cells) == nerd,
                      "Nerd icons are opt-in and plain mode emits no private-use glyphs: " + label,
                      expected_icons="nerd" if nerd else "plain")
        color_samples = [("active filename", filenames[0])]
        if labels:
            color_samples.append(("outline header", labels[0]))
        for part, chunk in color_samples:
            ratios = [contrast_ratio(c["fg"], c["bg"]) for c in chunk]
            session.check(min(ratios) >= 3, f"Readable explicit {part} colors: {label}", minimum_contrast=min(ratios))
        header_backgrounds = {c["bg"] for c in cells if c["y"] == header_y and c["char"].strip()}
        if state["cols"] >= 80:
            session.check(len(header_backgrounds) >= 2, "Header segments use distinct backgrounds: " + label)
            positions = list(find_cells(cells, "Ln "))
            session.check(len(positions) == 1, "Cursor position remains visible in statusline: " + label)
            position = positions[0]
            footer = "".join(c["char"] for c in sorted(
                (c for c in cells if c["y"] == position[0]["y"]), key=lambda c: c["x"]))
            session.check(footer.count("MARKDOWN") == 1 and len(re.findall(r"\b\d+ words\b", footer)) == 1,
                          "Footer file type and word count each appear only once: " + label, footer=footer)
            session.check(position[0]["y"] > header_y, "Position belongs to the footer: " + label)
            backgrounds = {c["bg"] for c in cells if c["y"] == position[0]["y"]}
            session.check(len(backgrounds) >= 2, "Statusline segments use distinct backgrounds: " + label)
        return state, cells

    session.key("Ctrl+Home")
    idle("01-single-header-160x45", expect_outline=True, first_line=True)
    for width, height in [(120, 36), (80, 24), (42, 16)]:
        session.call("resize", width, height)
        idle(f"02-single-header-{width}x{height}", expect_outline=width >= 110, first_line=True)
    session.call("resize", 160, 45)
    session.key("F9")
    session.key("Home")
    session.key("Down")
    session.key("Enter")
    alpha_line = source.splitlines().index("## Section Alpha") + 1
    session.check(f"Ln {alpha_line}, Col 1" in session.state()["text"],
                  "Outline Home/Down selects the second heading with no document entries ahead of it")
    saved_exact("Keyboard outline navigation preserves every source byte")
    session.key("F9")
    session.call("type", "ignored")
    session.key("Escape")
    saved_exact("Typing while outline has focus cannot modify the document")
    cells = session.cells()
    target = outline_target(session, "Final Section", cells)[1]
    session.call("mouse", "click", target["x"], target["y"])
    session.settle()
    final_line = source.splitlines().index("## Final Section") + 1
    session.check(f"Ln {final_line}, Col 1" in session.state()["text"],
                  "Outline pointer jumps to the exact offscreen source heading")
    session.capture("03-outline-pointer-focus")
    saved_exact("Pointer outline navigation preserves exact source")

    session.key("Ctrl+g")
    session.call("type", "3")
    session.key("Enter")
    before = session.cells()
    session.key("Ctrl+Shift+Right")
    session.check(selection_visible(before, session.cells()), "Document selection is visible before theme changes")
    catalog = ROOT / "third_party/iterm2-themes/palettes.json"
    if not catalog.exists():
        catalog = Path(__file__).with_name("palettes.json")
    palettes = {theme["name"]: theme for theme in json.loads(catalog.read_text())["themes"]}
    for index, theme in enumerate(["Sage Dark", "Sage Light", "Dracula", "Catppuccin Latte", "Nord"]):
        session.key("F2")
        session.call("type", "theme")
        session.key("Enter")
        session.call("type", theme)
        session.key("Enter")
        state, cells = idle(f"04-theme-{index + 1:02d}", expect_outline=True)
        background = next(c["bg"] for c in cells if (c["x"], c["y"]) == (state["cols"] - 1, 3))
        if theme in palettes:
            session.check(background.lower() == palettes[theme]["background"].lower(),
                          f"{theme} retains its upstream document background alongside new chrome")
        session.check(highlighted(cells, "Keep", background), f"{theme} preserves the existing source selection")
    session.paste("REPLACED")
    session.key("Ctrl+s")
    session.check(session.fixture.read_bytes() == source.replace("Keep", "REPLACED", 1).encode(),
                  "Theme selection preserves the exact selected source range")
    session.key("Ctrl+z")
    saved_exact("One undo after theme changes restores the original source bytes")

    for number in range(2, 10):
        session.key("Ctrl+n")
        session.call("type", f"Tab{number}")
    active_name = "Untitled 9.md"
    for width, height in [(160, 45), (120, 36), (80, 24), (42, 16)]:
        session.call("resize", width, height)
        state, cells = session.capture(f"05-tab-overflow-{width}x{height}")
        labels = list(find_cells(cells, active_name))
        session.check(len(labels) == 1 and "Tab9" in state["text"],
                      f"Active document remains visible once during tab overflow at {width} columns")
        active = labels[0]
        dirty = [c for c in cells if c["y"] == active[0]["y"]
                 and active[0]["x"] - 4 <= c["x"] <= active[-1]["x"] + 3 and c["char"] == "*"]
        session.check(bool(dirty), f"Active dirty marker survives tab overflow at {width} columns")
        session.check(not re.search(r"\b(?:DOCUMENTS|Live|F1|F2|F9)\b", state["text"]),
                      f"Tab overflow does not introduce repeated chrome hints at {width} columns")
    session.key("F7")
    session.check("Untitled 8.md" in session.state()["text"] and "Tab8" in session.state()["text"],
                  "Previous tab remains independently editable during overflow")
    session.key("F8")
    session.call("resize", 20, 10)
    state, cells = session.capture("06-compact-dirty-tab-20x10")
    session.check("Tab9" in state["text"] and any(c["char"] == "*" for c in cells),
                  "Compact header preserves the current buffer and its dirty marker")
    session.call("resize", 160, 45)
    for _ in range(8):
        session.key("Ctrl+w")
        session.call("expect", "text", "Unsaved document", "--timeout", 3000)
        session.key("n")
    session.key("Ctrl+Home")
    saved_exact("Closing overflow tabs never changes the original document")
    idle("07-final-single-header", expect_outline=True, first_line=True)
    session.check(not any(c["png_error"] for c in session.captures),
                  "Chrome and optional Nerd Font captures all export as native PNGs")


def reliability(session):
    source = session.original_source
    external = source + "\nFirst external edit\n"
    session.fixture.write_text(external)
    session.call("expect", "text", "Disk file changed", "--timeout", 4000)
    session.check("F5" in session.state()["text"], "External file changes offer a reload action")
    session.capture("01-external-change")
    session.key("F5")
    session.key("Ctrl+End")
    session.paste("local draft")
    local = external + "local draft"
    newer = source + "\nSecond external edit\n"
    session.fixture.write_text(newer)
    session.key("F5")
    session.check("Replace local edits with disk text?" in session.state()["text"], "Dirty reload requires an explicit confirmation")
    session.capture("02-dirty-reload")
    session.call("resize", 20, 4)
    session.key("y")
    session.check("Resize" in session.state()["text"], "Tiny reload prompt cannot discard local edits")
    session.capture("03-tiny-reload")
    session.call("resize", 80, 24)
    session.key("n")
    session.key("F5")
    session.capture("04-reload-80x24")
    session.key("y")
    session.key("Ctrl+z")
    session.key("Ctrl+s")
    session.check(session.fixture.read_text() == local, "Undo after confirmed reload restores the exact local draft against the new baseline")
    # Return the scratch fixture to its initial bytes using an intentional edit.
    session.key("Ctrl+a")
    session.paste(source)
    session.key("Ctrl+s")
    session.check(session.fixture.read_bytes() == session.original_bytes, "Reload checks finish with the original scratch source")
    session.check(not any(c["png_error"] for c in session.captures), "All reload-dialog captures export as native PNG")


def themes(session):
    catalog = ROOT / "third_party/iterm2-themes/palettes.json"
    if not catalog.exists():
        catalog = Path(__file__).with_name("palettes.json")
    data = json.loads(catalog.read_text())
    palettes = {theme["name"]: theme for theme in data["themes"]}
    names = ["Catppuccin Mocha", "Catppuccin Latte", "Dracula", "Nord", "Gruvbox Dark", "TokyoNight", "iTerm2 Solarized Light"]
    def background():
        return next(c["bg"] for c in session.cells() if (c["x"], c["y"]) == (159, 3))
    for index, name in enumerate(names):
        before = background()
        session.key("F2")
        session.call("type", "theme")
        session.key("Enter")
        session.call("type", name)
        session.settle()
        expected = palettes[name]["background"].lower()
        session.check(background().lower() == expected, f"{name} previews the upstream document background")
        session.capture(f"{index+1:02d}-" + palettes[name]["id"] + "-preview")
        session.key("Escape")
        session.check(background() == before, f"Escape from {name} restores the previous theme")
        session.key("F2")
        session.call("type", "theme")
        session.key("Enter")
        session.call("type", name)
        session.settle()
        session.key("Enter")
        session.check(background().lower() == expected, f"{name} remains applied after accepting")
        session.capture(f"{index+1:02d}-" + palettes[name]["id"])
    session.check(session.fixture.read_bytes() == session.original_bytes, "Theme previews and acceptance never change the source")
    session.key("F2")
    session.call("type", "theme")
    session.key("Enter")
    session.call("type", "Catppuccin")
    session.call("resize", 42, 16)
    session.capture("08-picker-42x16")
    session.call("resize", 20, 4)
    session.key("Enter")
    session.check("Resize to choose" in session.state()["text"], "Tiny theme picker cannot accept invisible choices")
    session.capture("09-picker-20x4")
    session.call("resize", 160, 45)
    session.key("Escape")
    session.check(not any(c["png_error"] for c in session.captures), "Imported-theme captures all export as PNG")


def workflows(session):
    """Verify navigation and line editing through the real executable and PTY."""
    source = session.original_bytes
    lines = source.splitlines(keepends=True)
    name = session.fixture.name

    def count(title, expected):
        text = session.state()["text"]
        session.check(re.search(re.escape(title) + r"\s*·\s*" + str(expected) + r"\b", text),
                      f"{title} shows exactly {expected} choices", text=text)

    def chooser(key, query=""):
        session.key(key)
        if query:
            session.call("type", query)
        session.settle()

    def go(line):
        session.key("Ctrl+g")
        session.call("type", str(line))
        session.key("Enter")

    def position(line, column=1):
        text = session.state()["text"]
        session.check(f"Ln {line}, Col {column}" in text,
                      f"Source caret is at line {line}, column {column}", text=text)

    def save(expected, message):
        session.key("Ctrl+s")
        actual = session.fixture.read_bytes()
        session.check(actual == expected, message, expected=expected.decode(), actual=actual.decode())
        session.key("Escape")

    def select_alpha():
        go(3)
        for _ in "Alpha":
            session.key("Shift+Right")
        position(3, 6)

    def selection_survived(message):
        session.paste("REPLACED")
        save(source.replace(b"Alpha", b"REPLACED", 1), message)
        session.key("Ctrl+z")
        save(source, "One undo restores exact source after the selection check")

    # The CLI click emits down and release. A count of two catches double activation.
    state, cells = session.capture("01-workflows-160x45")
    header_y = min(c["y"] for c in cells)
    plus = [c for c in cells if c["y"] == header_y and c["char"] == "+"]
    session.check(len(plus) == 1, "Exactly one plain plus button is visible in the tab row")
    session.call("mouse", "click", plus[0]["x"], plus[0]["y"])
    session.settle()
    chooser("F10")
    count("Open documents", 2)
    session.key("Escape")
    session.check("Untitled 2.md" in session.state()["text"], "Plus activates one new untitled document")
    draft = "Unsaved workflow draft"
    session.paste(draft)
    chooser("F10")
    count("Open documents", 2)
    text = session.state()["text"]
    session.check("● Untitled 2.md" in text and "Unsaved document" in text and name in text,
                  "Document picker includes dirty unsaved and file-backed tabs")
    session.capture("02-documents-dirty-unsaved")
    session.key("Escape")

    # Exercise path filtering using an independently created scratch document.
    other = session.output / "project-beta" / "note.md"
    other.parent.mkdir()
    other_source = b"# Secondary document\n\nSecond file stays exact.\n"
    other.write_bytes(other_source)
    session.key("Ctrl+o")
    session.key("Tab")
    session.paste(str(other))
    session.key("Enter")
    session.check("Secondary document" in session.state()["text"], "Browser opens the scratch path as a third tab")
    chooser("F10", "unt2")
    count("Open documents", 1)
    session.key("Enter")
    session.check(draft in session.state()["text"], "Fuzzy document-name filtering switches to the unsaved draft")
    chooser("F10", "pjbt nmd")
    count("Open documents", 1)
    session.check("project-beta" in session.state()["text"], "Fuzzy path and name tokens expose the matching path")
    session.capture("03-document-path-filter")
    session.key("Escape")
    session.check(draft in session.state()["text"], "Canceling a different document choice preserves the active draft")
    chooser("F10", "pjbt nmd")
    session.key("Enter")
    session.check("Second file stays exact." in session.state()["text"], "Enter commits the filtered document switch")
    chooser("F10")
    count("Open documents", 3)
    session.key("Escape")

    # At each layout, use actual fuzzy choices and check the resulting source caret.
    planning_line = lines.index(b"## Planning checkpoint\n") + 1
    for width, height in [(42, 16), (80, 24), (160, 45)]:
        session.call("resize", width, height)
        chooser("F10", "wfmd")
        count("Open documents", 1)
        session.capture(f"04-document-picker-{width}x{height}")
        session.key("Enter")
        session.check("Alpha line" in session.state()["text"], f"Document acceptance reaches the fixture at {width} columns")
        go(3)
        chooser("F11", "plnchkpt")
        count("Headings", 1)
        session.check(f"H2 · line {planning_line}" in session.state()["text"],
                      f"Heading picker shows source level and line at {width} columns")
        session.capture(f"05-heading-picker-{width}x{height}")
        session.key("Enter")
        position(planning_line)
        save(source, f"Picker navigation preserves every source byte at {width} columns")

    # Cancellation preserves the precise selected range, not just visible text.
    session.call("resize", 80, 24)
    for key, query, title in [("F10", "unt2", "Open documents"), ("F11", "fnlchkpt", "Headings")]:
        select_alpha()
        chooser(key, query)
        count(title, 1)
        session.key("Down")
        session.key("Escape")
        position(3, 6)
        selection_survived(f"Canceling {title} preserves the exact Alpha source selection")

    chooser("F11", "impostor")
    count("Headings", 0)
    session.check("No matches" in session.state()["text"], "Fenced Markdown does not become a heading choice")
    session.key("Enter")
    count("Headings", 0)
    session.key("Escape")
    chooser("F11", "stxtwpt")
    count("Headings", 1)
    setext_line = lines.index(b"Setext waypoint\n") + 1
    session.check(f"H2 · line {setext_line}" in session.state()["text"], "Setext heading reports its original source line")
    session.key("Enter")
    position(setext_line)
    save(source, "Semantic heading navigation leaves source and fenced text unchanged")

    for key, query, title in [("F10", "unt2", "documents"), ("F11", "fnlchkpt", "headings")]:
        go(3)
        chooser(key, query)
        session.call("resize", 20, 4)
        session.call("mouse", "click", 5, 2)
        session.key("Enter")
        text = session.state()["text"]
        session.check("Resize to choose" in text, f"Tiny {title} picker rejects pointer and Enter acceptance")
        session.capture(f"06-tiny-{title}-20x4")
        session.key("Escape")
        session.call("resize", 80, 24)
        position(3)
        save(source, f"Tiny {title} picker cannot switch or edit the source")

    # Source mode gives line selection an unambiguous physical-row boundary.
    session.key("Ctrl+e")
    session.check("SOURCE" in session.state()["text"], "Line workflows run in source mode")

    def edited_lines(operation, selected):
        start, end = 3, 5 if selected else 4  # Bravo; optionally Charlie, excluding Delta.
        block = lines[start:end]
        expected = list(lines)
        if operation == "up":
            expected[start - 1:end] = block + lines[start - 1:start]
        elif operation == "down":
            expected[start:end + 1] = lines[end:end + 1] + block
        elif operation == "above":
            expected[start:start] = block
        else:
            expected[end:end] = block
        return b"".join(expected)

    commands = [
        ("up", "Alt+Up", "Move lines up"),
        ("down", "Alt+Down", "Move lines down"),
        ("above", "Alt+Shift+Up", "Duplicate lines above"),
        ("below", "Alt+Shift+Down", "Duplicate lines below"),
    ]
    for selected in [False, True]:
        for operation, shortcut, command in commands:
            go(4)
            if selected:
                session.key("Shift+Down")
                session.key("Shift+Down")
                session.key("F2")
                session.call("type", command)
                session.key("Enter")
            else:
                session.key(shortcut)
            description = f"{'Selected lines via palette' if selected else 'Current line via ' + shortcut}: {operation}"
            save(edited_lines(operation, selected), description + " saves exact source bytes")
            if selected:
                session.capture(f"07-selected-lines-{operation}-80x24")
            session.key("Ctrl+z")
            save(source, description + " is restored by one Undo")

    # Leave a single clean document so the common runner can verify normal exit.
    chooser("F10", "unt2")
    session.key("Enter")
    session.check(draft in session.state()["text"], "Unsaved draft survives all navigation and source editing")
    session.key("Ctrl+w")
    session.call("expect", "text", "Unsaved document", "--timeout", 3000)
    session.key("n")
    chooser("F10", "pjbt nmd")
    session.key("Enter")
    session.key("Ctrl+w")
    chooser("F10")
    count("Open documents", 1)
    session.key("Escape")
    save(source, "All workflows finish with the exact original fixture bytes")
    session.check(other.read_bytes() == other_source, "Path-filtered secondary scratch document is unchanged")
    session.key("Ctrl+e")
    session.key("Ctrl+Home")
    session.call("resize", 160, 45)
    session.capture("08-finished-workflows-160x45")
    session.check(not any(c["png_error"] for c in session.captures), "All ASCII workflow captures export native PNGs")


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--tool", required=True)
    parser.add_argument("--binary", required=True)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--keyboard", choices=["baseline", "enhanced"], default="enhanced")
    parser.add_argument("--suite", choices=["all", "captures", "protocol", "unicode", "workspace", "themes", "reliability", "chrome", "workflows"], default="all")
    parser.add_argument("--palette", choices=["dark", "light"], default="dark")
    parser.add_argument("--theme", choices=["dark", "light"],
                        help="Editor theme; omitted for compatibility with older binaries")
    parser.add_argument("--icons", choices=["plain", "nerd"], help="Editor icon mode; omitted for older binaries")
    parser.add_argument("--font", default="JetBrains Mono", help="Preferred tui-test recording font family")
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=False)
    shutil.copyfile(__file__, args.output / "scenario.py")
    shutil.copyfile(HELPER, args.output / "session.py")
    for fixture in ["acceptance.md", "screenshots.md", "writing.md", "chrome.md", "workflows.md"]:
        shutil.copyfile(Path(__file__).with_name(fixture), args.output / fixture)
    shutil.copyfile(ROOT / "third_party/iterm2-themes/palettes.json" if (ROOT / "third_party/iterm2-themes/palettes.json").exists() else Path(__file__).with_name("palettes.json"), args.output / "palettes.json")
    write_json(args.output / "invocation.json", vars(args) | {"output": str(args.output.resolve())})
    suites = ["captures", "protocol", "unicode", "workspace", "themes", "reliability", "chrome", "workflows"] if args.suite == "all" else [args.suite]
    for suite in suites:
        fixture = {"unicode": "acceptance.md", "workspace": "writing.md", "themes": "writing.md", "reliability": "writing.md", "chrome": "chrome.md", "workflows": "workflows.md"}.get(suite, "screenshots.md")
        app_args = ["--theme", args.theme] if args.theme else []
        if args.icons:
            app_args.extend(["--icons", args.icons])
        session = Session(args.tool, args.binary, args.output / suite,
                          Path(__file__).with_name(fixture), args.palette,
                          size=(160, 45) if suite in ("workspace", "themes", "reliability", "chrome", "workflows") else (80, 24), app_args=app_args, font=args.font)
        try:
            session.start()
            title = "Workflow acceptance" if suite == "workflows" else "Chrome acceptance" if suite == "chrome" else "A calmer place to write" if suite in ("workspace", "themes", "reliability") else "Terminal acceptance"
            session.call("expect", "text", title, "--timeout", 3000)
            if suite == "captures":
                captures(session)
            elif suite == "protocol":
                protocol(session, args.keyboard)
            elif suite == "workspace":
                workspace(session)
            elif suite == "themes":
                themes(session)
            elif suite == "reliability":
                reliability(session)
            elif suite == "chrome":
                chrome(session)
            elif suite == "workflows":
                workflows(session)
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
