"""Small, application-independent adapter for the pinned tui-test CLI.

Runs a real executable under tui-test's PTY and Ghostty backend. The CLI's
native screenshots use its own renderer, not Ghostty desktop pixels. Keep the
owning Python process alive until close: detached daemons may be reaped by CI.
"""

import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile


def write_json(path, value):
    Path(path).write_text(json.dumps(value, ensure_ascii=False, indent=2) + "\n")


def sha256(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def decode_log_bytes(value):
    """Decode tui-test's reversible verbose byte log, not terminal escapes."""
    escapes = {"e": 27, "r": 13, "n": 10, "t": 9, "\\": 92}
    result = bytearray()
    pos = 0
    while pos < len(value):
        if value[pos] != "\\":
            result.extend(value[pos].encode())
            pos += 1
        elif value[pos + 1] == "x":
            result.append(int(value[pos + 2:pos + 4], 16))
            pos += 4
        else:
            result.append(escapes[value[pos + 1]])
            pos += 2
    return bytes(result)


class Session:
    def __init__(self, tool, binary, output, fixture, palette="dark", size=(80, 24), app_args=(), font="JetBrains Mono"):
        self.tool, self.binary = Path(tool).resolve(), Path(binary).resolve()
        self.output = Path(output).resolve()
        self.output.mkdir(parents=True, exist_ok=False)
        self.runtime = Path(tempfile.mkdtemp(prefix="marklane-tui-"))
        self.name = "acceptance"
        self.log = self.runtime / (self.name + ".log")
        self.actions, self.assertions, self.captures = [], [], []
        self.started = False
        self.fixture = self.output / Path(fixture).name
        shutil.copyfile(fixture, self.fixture)
        self.original_bytes = self.fixture.read_bytes()
        self.original_source = self.original_bytes.decode("utf-8")
        self.size = size
        self.app_args = list(app_args)
        self.env = dict(os.environ)
        # The execution shell commonly sets NO_COLOR=1 and TERM=dumb.
        self.env.pop("NO_COLOR", None)
        self.env.update(TUI_TEST_HOME=str(self.runtime), TERM="xterm-256color",
                        XDG_CONFIG_HOME=str(self.runtime / "config"),
                        XDG_STATE_HOME=str(self.runtime / "state"),
                        SSH_CONNECTION="marklane-visual-test",
                        COLORTERM="truecolor", TUI_TEST_RECORDING_FONT_FAMILIES=font)
        colors = {"dark": ("#171a21", "#d6dce8"), "light": ("#f5f3ed", "#242932")}
        background, foreground = colors[palette]
        config = self.output / "tui-test.toml"
        config.write_text('[trace]\nmode = "on"\ndirectory = ' + json.dumps(str(self.output / "traces"))
                          + '\n[recording]\ndirectory = ' + json.dumps(str(self.output / "casts"))
                          + '\n[profiles.default.colors]\nbackground = ' + json.dumps(background)
                          + '\nforeground = ' + json.dumps(foreground) + '\n')
        self.env["TUI_TEST_CONFIG"] = str(config)
        version = subprocess.check_output([str(self.tool), "--version"], text=True).strip()
        if version != "tui-test 0.1.0-beta.5":
            raise RuntimeError("Unreviewed tui-test version: " + version)
        self.metadata = {
            "tool": str(self.tool), "tool_version": version, "tool_sha256": sha256(self.tool),
            "binary": str(self.binary), "binary_sha256": sha256(self.binary),
            "app_arguments": self.app_args,
            "fixture_sha256": sha256(fixture), "backend": "ghostty", "initial_size": size,
            "palette": {"name": palette, "background": background, "foreground": foreground},
            "environment": {"TERM": "xterm-256color", "COLORTERM": "truecolor", "NO_COLOR": None,
                            "TUI_TEST_RECORDING_FONT_FAMILIES": font},
            "font": {"preferred_family": font, "cell_pixels": [10, 21], "size": 17,
                     "fallback": "tui-test bundled styles plus system fallback; not fully pinned"},
            "limitations": ["Embedded Ghostty backend is not native Ghostty 1.3.1.",
                            "PNG rasterizer rejects multi-scalar cell graphemes; SVG and cells retained.",
                            "ZWJ emoji can split in backend cells and overlap app cursor placement; not visually accepted.",
                            "Native clipboard, OS key interception, IME and desktop pixels are not tested."],
        }
        source_root = next((p for p in self.binary.parents if (p / ".git").exists()), None)
        if source_root:
            self.metadata["source_checkout"] = str(source_root)
            self.metadata["source_revision"] = subprocess.check_output(
                ["git", "-C", str(source_root), "rev-parse", "HEAD"], text=True).strip()
            self.metadata["source_worktree_status"] = subprocess.check_output(
                ["git", "-C", str(source_root), "status", "--short"], text=True)
        write_json(self.output / "metadata.json", self.metadata)

    def call(self, *args, allow_failure=False):
        start = self.log.stat().st_size if self.log.exists() else 0
        process = subprocess.run([str(self.tool), "--session", self.name, "--json", *map(str, args)],
                                 env=self.env, capture_output=True, text=True, timeout=20)
        record = {"argv": list(map(str, args)), "returncode": process.returncode,
                  "stdout": process.stdout, "stderr": process.stderr, "log_offset": start}
        self.actions.append(record)
        write_json(self.output / "actions.json", self.actions)
        if process.returncode and not allow_failure:
            raise RuntimeError(f"tui-test {args}: {process.stdout} {process.stderr}")
        data = json.loads(process.stdout) if process.stdout.strip() else {}
        return data if process.returncode else data.get("data")

    def start(self):
        self.started = True
        return self.call("--verbose", "run", "--backend", "ghostty", "--cols", self.size[0],
                         "--rows", self.size[1], "--cwd", self.output,
                         self.binary, *self.app_args, self.fixture)

    def settle(self):
        self.call("wait", "idle", "--timeout", "3000")

    def state(self):
        self.settle()
        return self.call("state")

    def key(self, key):
        self.call("key", "press", key)
        self.settle()

    def raw(self, data):
        self.call("write", data)
        self.settle()

    def paste(self, text):
        modes = self.state()["modes"]
        if "\x1b[201~" in text:
            raise ValueError("Paste must not contain a bracketed-paste terminator")
        self.raw("\x1b[200~" + text + "\x1b[201~" if modes["bracketed_paste"] else text)

    def wheel(self, x, y, direction, count=1):
        state = self.state()
        if state["mouse_mode"] == "none":
            raise RuntimeError("Application has not requested mouse reporting")
        # The state API omits mouse encoding. Track requested encoding from
        # real output, including resets; do not silently assume SGR coordinates.
        modes = {1006: False, 1016: False}
        for match in re.finditer(rb"\x1bc|\x1b\[!p|\x1b\[\?([0-9;]+)([hl])", self.traffic("READ")):
            if match.group(1) is None:
                modes = {1006: False, 1016: False}
            else:
                for mode in map(int, match.group(1).split(b";")):
                    if mode in modes:
                        modes[mode] = match.group(2) == b"h"
        if not modes[1006] or modes[1016]:
            raise RuntimeError("Cell-positioned wheel requires observed SGR 1006 and no pixel mode 1016")
        if not (0 <= x < state["cols"] and 0 <= y < state["rows"]):
            raise ValueError("Wheel point outside terminal")
        button = {"up": 64, "down": 65}[direction]
        self.raw(f"\x1b[<{button};{x + 1};{y + 1}M" * count)

    def cells(self, state=None):
        state = state or self.state()
        return self.call("cells", 0, 0, state["cols"], state["rows"])["cells"]

    def capture(self, name, png=True):
        state = self.state()
        cells = self.cells(state)
        write_json(self.output / (name + ".state.json"), state)
        write_json(self.output / (name + ".cells.json"), cells)
        self.call("screenshot", "-o", self.output / (name + ".svg"))
        result = self.call("screenshot", "-o", self.output / (name + ".png"), allow_failure=True) if png else None
        error = result if isinstance(result, dict) and result.get("ok") is False else None
        capture = {"name": name, "size": [state["cols"], state["rows"]], "png_error": error}
        self.captures.append(capture)
        write_json(self.output / "captures.json", self.captures)
        return state, cells

    def check(self, condition, message, **evidence):
        self.assertions.append({"passed": bool(condition), "message": message, **evidence})
        write_json(self.output / "assertions.json", self.assertions)
        if not condition:
            raise AssertionError(message)

    def traffic(self, direction, offset=0):
        if not self.log.exists():
            return b""
        data = self.log.read_bytes()[offset:].decode()
        return b"".join(decode_log_bytes(m.group(1)) for line in data.splitlines()
                        if (m := re.match(r"^\S+ " + f"{direction:<6}" + r"(.*)$", line)))

    def close(self):
        try:
            if self.started:
                self.call("close", allow_failure=True)
        finally:
            try:
                if self.log.exists():
                    shutil.copyfile(self.log, self.output / "terminal.log")
                    # The unmodified escaped log is authoritative; these aid byte inspection.
                    for direction in ["READ", "WRITE", "REPLY"]:
                        (self.output / (direction.lower() + ".bin")).write_bytes(self.traffic(direction))
            finally:
                shutil.rmtree(self.runtime)
