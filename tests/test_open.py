"""Exercise the actual Open With TUI in an isolated XDG environment and PTY."""
import atexit
import errno
import fcntl
import json
import os
from pathlib import Path
import pty
import re
import unicodedata
import select
import struct
import signal
import subprocess
import termios
import time

root = Path(os.environ["TEST_ROOT"])
binary = os.environ["INS_BIN"]
bin_dir = root / "bin"
bin_dir.mkdir()
applications = Path(os.environ["XDG_DATA_HOME"]) / "applications"
applications.mkdir()
os.environ.update(
    PATH=f"{bin_dir}:{os.environ['PATH']}",
    XDG_DATA_DIRS=str(root / "system"),
    XDG_STATE_HOME=str(root / "state"),
    TERM="xterm-256color",
)


def executable(name, source):
    path = bin_dir / name
    path.write_text("#!/usr/bin/env python3\n" + source)
    path.chmod(0o755)
    return path


launcher = executable("capture", """
import json, os, sys
from pathlib import Path
with (Path(os.environ['TEST_ROOT']) / 'launches.jsonl').open('a') as file:
    file.write(json.dumps(sys.argv[1:]) + '\\n')
""")
executable("xdg-mime", """
import json, os, sys
from pathlib import Path
root = Path(os.environ['TEST_ROOT'])
args = sys.argv[1:]
if args[:2] == ['query', 'filetype']:
    print('image/png' if args[2].endswith('.png') else 'text/plain')
elif args[:2] == ['query', 'default']:
    print('viewer.desktop' if args[2] == 'image/png' else 'editor.desktop')
elif args[0] == 'default':
    with (root / 'defaults.jsonl').open('a') as file:
        file.write(json.dumps(args[1:]) + '\\n')
else:
    sys.exit(1)
""")
terminal = executable("foot", """
import json, os, sys
from pathlib import Path
path = Path(os.environ['TEST_ROOT']) / 'terminal.json'
temp = path.with_suffix('.tmp')
temp.write_text(json.dumps(sys.argv[1:]))
temp.replace(path)
""")
os.environ["TERMINAL"] = str(terminal)
for desktop_id, name, mime, extra in [
    ("editor", "Default Editor", "text/plain", ""),
    ("second", "Second Editor", "text/plain", "NoDisplay=true\n"),
    ("viewer", "Image Viewer", "image/png", ""),
    ("terminal", "Terminal Editor", "text/plain", "Terminal=true\n"),
    ("broken", "Broken Application", "text/plain", ""),
    ("hidden", "Hidden Application", "text/plain", "Hidden=true\n"),
]:
    command = "/nonexistent/ins-open-test" if desktop_id == "broken" else f"{launcher} {desktop_id}"
    (applications / f"{desktop_id}.desktop").write_text(
        f"[Desktop Entry]\nType=Application\nName={name}\nExec={command} %f\n"
        f"MimeType={mime};\nComment=Test application\n{extra}"
    )
file = root / "a file; $literal.txt"
file.write_text("File preview contents\n")
image = root / "image.png"
image.write_bytes(b"\x89PNG\0")
history = Path(os.environ["XDG_STATE_HOME"]) / "ins/open-frecency.json"


class Picker:
    def __init__(self, path=file, width=120):
        self.width = width
        self.master, slave = pty.openpty()
        fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 36, width, 0, 0))

        def controlling_terminal():
            os.setsid()
            fcntl.ioctl(slave, termios.TIOCSCTTY, 0)

        self.process = subprocess.Popen(
            [binary, "open", str(path)], stdin=slave, stdout=slave, stderr=slave,
            preexec_fn=controlling_terminal,
        )
        atexit.register(self.cleanup)
        os.close(slave)
        self.output = bytearray()
        self.cursor_requests = 0

    def cleanup(self):
        if self.process.poll() is None:
            os.killpg(self.process.pid, signal.SIGTERM)
            self.process.wait()

    def screen(self):
        # Minimal VT screen reconstruction: fzf sends cell diffs and cursor
        # moves, so raw output does not contain contiguous displayed strings.
        width, height = self.width, 40
        cells = [[" "] * width for _ in range(height)]
        row = col = 0
        decoded = self.output.decode(errors="replace")
        tokens = re.findall(r"\x1b\[[0-?]*[ -/]*[@-~]|\x1b.|[^\x1b]+", decoded)
        for token in tokens:
            if token.startswith("\x1b["):
                command = token[-1]
                params = token[2:-1]
                if params.startswith("?"):
                    continue
                values = [int(value or 0) for value in params.split(";")]
                amount = values[0] or 1
                if command in "Hf":
                    row = max(0, values[0] - 1)
                    col = max(0, (values[1] if len(values) > 1 else 1) - 1)
                elif command == "J" and values[0] == 2:
                    cells = [[" "] * width for _ in range(height)]
                elif command == "G":
                    col = amount - 1
                elif command == "A":
                    row = max(0, row - amount)
                elif command == "B":
                    row += amount
                elif command == "C":
                    col += amount
                elif command == "D":
                    col = max(0, col - amount)
                continue
            if token.startswith("\x1b"):
                continue
            for char in token:
                if char == "\r":
                    col = 0
                elif char == "\n":
                    row += 1
                elif not char.isprintable():
                    continue
                else:
                    if col >= width:
                        col, row = 0, row + 1
                    if row < height:
                        cells[row][col] = char
                    col += 2 if unicodedata.east_asian_width(char) in "WF" else 1
        return "\n".join("".join(line) for line in cells)

    def read(self):
        if select.select([self.master], [], [], 0.05)[0]:
            try:
                self.output.extend(os.read(self.master, 65536))
                requests = self.output.count(b"\x1b[6n")
                while self.cursor_requests < requests:
                    os.write(self.master, b"\x1b[1;1R")
                    self.cursor_requests += 1
            except OSError as error:
                if error.errno != errno.EIO:
                    raise

    def wait_for(self, text):
        deadline = time.monotonic() + 10
        while text not in self.screen():
            self.read()
            if time.monotonic() >= deadline or self.process.poll() is not None:
                raise AssertionError(f"Missing {text!r}: {self.screen()}")

    def wait_focused(self, text):
        deadline = time.monotonic() + 10
        while not any("▌" in line and text in line for line in self.screen().splitlines()):
            self.read()
            if time.monotonic() >= deadline or self.process.poll() is not None:
                raise AssertionError(f"Not focused on {text!r}: {self.screen()}")

    def double_click_separator(self, text):
        rows = self.screen().splitlines()
        row = next(i for i, line in enumerate(rows) if "╌╌" in line and text in line[:self.width // 2])
        event = f"\x1b[<0;12;{row + 1}M\x1b[<0;12;{row + 1}m".encode()
        self.send(event + event)

    def scroll_preview_to(self, text):
        deadline = time.monotonic() + 10
        while text not in self.screen():
            self.send(b"\x1b[1;3B" * 5)  # Alt+Down
            self.read()
            if time.monotonic() >= deadline or self.process.poll() is not None:
                raise AssertionError(f"Missing preview {text!r}: {self.screen()}")

    def send(self, keys):
        os.write(self.master, keys)

    def finish(self, keys=b"\x1b", status=0):
        self.send(keys)
        deadline = time.monotonic() + 10
        while self.process.poll() is None and time.monotonic() < deadline:
            self.read()
        if self.process.poll() is None:
            self.process.kill()
            raise AssertionError("Picker did not exit")
        while select.select([self.master], [], [], 0)[0]:
            before = len(self.output)
            self.read()
            if len(self.output) == before:
                break
        os.close(self.master)
        assert self.process.returncode == status, self.output.decode(errors="replace")
        assert b"\x1b[?1049l" in self.output, "Alternate screen was not restored"


def launches():
    path = root / "launches.jsonl"
    return [json.loads(line) for line in path.read_text().splitlines()] if path.exists() else []


def await_launches(count):
    deadline = time.monotonic() + 5
    while len(launches()) < count and time.monotonic() < deadline:
        time.sleep(0.02)
    assert len(launches()) == count, (launches(), picker.screen())


# A blocked association-cache read must not delay the default, its preview,
# keyboard focus, or the checkbox. Release discovery only after verifying them.
cache = applications / "mimeinfo.cache"
os.mkfifo(cache)
picker = Picker()
picker.wait_for("Desktop ID: editor.desktop")
picker.wait_focused("Default Editor")
picker.send(b"\x04")
picker.wait_for("[✓] Use as default")
with cache.open("w") as stream:
    stream.write("[MIME Cache]\ntext/plain=editor.desktop;second.desktop;terminal.desktop;broken.desktop;\n")
picker.wait_for("Recommended")
picker.send(b"Second")
picker.wait_for("Desktop ID: second.desktop")
assert "[✓] Use as default" in picker.screen()
picker.finish()
cache.unlink()
cache.write_text("[MIME Cache]\ntext/plain=editor.desktop;second.desktop;terminal.desktop;broken.desktop;\n")


picker = Picker()
picker.wait_for("Current default")
assert picker.screen().count("Ctrl D") == 1, picker.screen()
assert "toggle checkbox" not in picker.screen()
assert "use as default" in picker.screen(), picker.screen()
picker.scroll_preview_to("File preview contents")
assert b"Hidden Application" not in picker.output
picker.finish()
assert not history.exists()
assert not launches()

# A streamed menu remains alive on separator confirmation after discovery ends.
# Wheel navigation may focus a heading; Enter and double-click must not submit it.
picker = Picker()
picker.wait_for("Desktop ID: editor.desktop")
assert b"\xe2\x81\xa0" not in picker.output, "Query contains the old invisible marker"
picker.send(b"\x04")
picker.wait_for("[✓] Use as default")
selected_row = next(i for i, line in enumerate(picker.screen().splitlines())
                    if "▌" in line and "Default Editor" in line[:picker.width // 2])
picker.send(f"\x1b[<64;12;{selected_row + 1}M".encode())  # Wheel up over the list.
picker.wait_focused("Current default")
picker.send(b"\r")
picker.double_click_separator("Current default")
picker.send(b"Second")
picker.wait_for("Desktop ID: second.desktop")
assert "[✓] Use as default" in picker.screen(), "Checkbox state was lost"
assert not launches()
assert not history.exists()
assert not (root / "defaults.jsonl").exists()
picker.finish()

# Native footer checkbox; default app is preselected.
picker = Picker()
picker.wait_for("Current default")
picker.finish(b"\x04\r")
await_launches(1)
assert launches()[0] == ["editor", str(file)]
assert json.loads((root / "defaults.jsonl").read_text()) == ["editor.desktop", "text/plain"]

# NoDisplay handlers are selectable; ordinary launches don't alter defaults.
picker = Picker()
picker.wait_for("Current default")
picker.send(b"Second")
picker.wait_for("Desktop ID: second.desktop")
picker.send(b"\x04")
picker.wait_for("[✓] Use as default")
picker.send(b"\x04")
picker.wait_for("[ ] Use as default")
picker.finish(b"\r")
await_launches(2)
assert launches()[1] == ["second", str(file)]
assert len((root / "defaults.jsonl").read_text().splitlines()) == 1
entries = json.loads(history.read_text())["entries"]
assert "text/plain\nsecond.desktop" in entries

# Padded menus use the existing responsive preview layout on narrow terminals.
picker = Picker(width=70)
picker.wait_for("Current default")
assert picker.screen().count("Ctrl D") == 1, picker.screen()
assert "toggle checkbox" not in picker.screen()
assert "use as default" in picker.screen(), picker.screen()
picker.scroll_preview_to("File preview contents")
picker.finish()

# MIME scopes don't bleed into one another.
picker = Picker(image)
picker.wait_for("Image Viewer")
picker.finish()

# Failed launches neither record usage nor change the default.
old_history = history.read_bytes()
picker = Picker()
picker.wait_for("Current default")
picker.send(b"Broken")
picker.wait_for("Desktop ID: broken.desktop")
picker.finish(b"\x04\r", status=1)
assert history.read_bytes() == old_history
assert len((root / "defaults.jsonl").read_text().splitlines()) == 1
assert len(launches()) == 2
# Terminal desktop handlers preserve the same file argument.
picker = Picker()
picker.wait_for("Current default")
picker.send(b"Terminal Editor")
picker.wait_for("Desktop ID: terminal.desktop")
picker.finish(b"\r")
terminal_record = root / "terminal.json"
deadline = time.monotonic() + 5
while not terminal_record.exists() and time.monotonic() < deadline:
    time.sleep(0.02)
assert json.loads(terminal_record.read_text()) == ["--", str(launcher), "terminal", str(file)]
terminal_record.unlink()

# --gui starts a graphical terminal without requiring an existing TTY.
subprocess.run([binary, "open", "--gui", str(file)], check=True, capture_output=True)
deadline = time.monotonic() + 5
while not terminal_record.exists() and time.monotonic() < deadline:
    time.sleep(0.02)
terminal_args = json.loads(terminal_record.read_text())
assert terminal_args[-2:] == ["open", str(file)], terminal_args
assert os.access(terminal_args[-3], os.X_OK), terminal_args

print("Open With fzf PTY tests passed")
