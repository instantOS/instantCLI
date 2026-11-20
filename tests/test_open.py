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
import shutil
import subprocess
import termios
import time

root = Path(os.environ["TEST_ROOT"])
binary = os.environ["INS_BIN"]
real_xdg_mime = shutil.which("xdg-mime")
bin_dir = root / "bin"
bin_dir.mkdir()
# Let the real ins-open.desktop entry resolve TryExec like an installed system.
os.symlink(Path(binary).resolve(), bin_dir / "ins")
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
    if Path(args[2]).name == 'mime-error.txt':
        print('test MIME detection failure', file=sys.stderr)
        sys.exit(1)
    gate = root / 'mime-gate'
    if gate.exists():
        (root / 'mime-waiting').touch()
        with gate.open() as stream:
            stream.read()
    print('image/png' if args[2].endswith('.png') else 'text/plain')
elif args[:2] == ['query', 'default']:
    default = 'viewer.desktop' if args[2] == 'image/png' else 'editor.desktop'
    log = root / 'defaults.jsonl'
    if log.exists():
        for line in log.read_text().splitlines():
            desktop, mime = json.loads(line)
            if mime == args[2]:
                default = desktop
    print(default)
elif args[0] == 'default':
    with (root / 'defaults.jsonl').open('a') as file:
        file.write(json.dumps(args[1:]) + '\\n')
else:
    sys.exit(1)
""")
# Record MIME probes; image previews may separately request descriptive metadata.
executable("file", """
import os, sys
from pathlib import Path
if '--mime-type' in sys.argv:
    with (Path(os.environ['TEST_ROOT']) / 'file-probes').open('a') as stream:
        stream.write('probe\\n')
os.execv('/usr/bin/file', ['file', *sys.argv[1:]])
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
            raise AssertionError(f"Picker did not exit: {self.screen()}")
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


# Existing file previews detect types when needed; supplied types skip detection.
standalone = subprocess.run(
    [binary, "preview", "--id", "file-suggestion", "--key", str(file)],
    check=True, capture_output=True, text=True,
)
assert "File preview contents" in standalone.stdout
probe_log = root / "file-probes"
assert probe_log.read_text().splitlines() == ["probe"]
supplied = subprocess.run(
    [binary, "preview", "--id", "file-suggestion", "--key", str(file),
     "--mime-type", "application/x-ins-test"],
    check=True, capture_output=True, text=True,
)
assert "application/x-ins-test" in supplied.stdout
assert probe_log.read_text().splitlines() == ["probe"]

# The UI and search work while MIME detection cannot finish.
gate = root / "mime-gate"
os.mkfifo(gate)
picker = Picker()
picker.wait_for("Open With")
assert "Current default" not in picker.screen()
picker.send(b"Second")
with gate.open("w") as stream:
    stream.write("continue")
picker.wait_for("Desktop ID: second.desktop")
picker.scroll_preview_to("File preview contents")
picker.finish()
gate.unlink()
assert not history.exists()
assert not launches()
assert probe_log.read_text().splitlines() == ["probe"]

# MIME failures surface inside the menu; confirming reports the actual error.
failed_mime = root / "mime-error.txt"
failed_mime.write_text("contents")
picker = Picker(failed_mime)
picker.wait_for("test MIME detection failure")
picker.finish(b"\r", status=1)
assert not history.exists()
assert not launches()

# A blocked association-cache read must not delay the default, its preview,
# or keyboard focus. Release discovery only after verifying them.
cache = applications / "mimeinfo.cache"
os.mkfifo(cache)
picker = Picker()
picker.wait_for("Desktop ID: editor.desktop")
picker.wait_focused("Default Editor")
with cache.open("w") as stream:
    stream.write("[MIME Cache]\ntext/plain=editor.desktop;second.desktop;terminal.desktop;broken.desktop;\n")
picker.wait_for("Recommended")
picker.send(b"Second")
picker.wait_for("Desktop ID: second.desktop")
picker.finish()
cache.unlink()
cache.write_text("[MIME Cache]\ntext/plain=editor.desktop;second.desktop;terminal.desktop;broken.desktop;\n")


picker = Picker()
picker.wait_for("Current default")
assert picker.screen().count("Ctrl D") == 1, picker.screen()
assert "toggle checkbox" not in picker.screen()
assert "set default" in picker.screen(), picker.screen()
assert "Use as default for this file type" not in picker.screen()
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
selected_row = next(i for i, line in enumerate(picker.screen().splitlines())
                    if "▌" in line and "Default Editor" in line[:picker.width // 2])
picker.send(f"\x1b[<64;12;{selected_row + 1}M".encode())  # Wheel up over the list.
picker.wait_focused("Current default")
picker.send(b"\r")
picker.double_click_separator("Current default")
picker.send(b"Second")
picker.wait_for("Desktop ID: second.desktop")
assert not launches()
assert not history.exists()
assert not (root / "defaults.jsonl").exists()
picker.finish()

# Ctrl+D saves the selected application immediately, without launching or history.
picker = Picker()
picker.wait_for("Current default")
picker.send(b"Second")
picker.wait_for("Desktop ID: second.desktop")
picker.send(b"\x04")
picker.wait_for("Default set to Second Editor")
picker.wait_focused("Second Editor")
assert json.loads((root / "defaults.jsonl").read_text()) == ["second.desktop", "text/plain"]
assert not launches()
assert not history.exists()
picker.finish()

# Enter opens the new default; ordinary launches don't alter defaults.
picker = Picker()
picker.wait_for("Desktop ID: second.desktop")
picker.finish(b"\r")
await_launches(1)
assert launches()[0] == ["second", str(file)]
assert len((root / "defaults.jsonl").read_text().splitlines()) == 1
entries = json.loads(history.read_text())["entries"]
assert "text/plain\nsecond.desktop" in entries

picker = Picker()
picker.wait_for("Current default")
picker.send(b"Default Editor")
picker.wait_for("Desktop ID: editor.desktop")
picker.finish(b"\r")
await_launches(2)
assert launches()[1] == ["editor", str(file)]
assert len((root / "defaults.jsonl").read_text().splitlines()) == 1

# Padded menus use the existing responsive preview layout on narrow terminals.
picker = Picker(width=70)
picker.wait_for("Current default")
assert picker.screen().count("Ctrl D") == 1, picker.screen()
assert "toggle checkbox" not in picker.screen()
assert "set default" in picker.screen(), picker.screen()
assert "Use as default for this file type" not in picker.screen()
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
picker.finish(b"\r", status=1)
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

# Ctrl+E uses the shared Yazi chooser, including cancel and invalid executable paths.
custom = executable("custom executable; $literal", """
import json, os, sys
from pathlib import Path
(Path(os.environ['TEST_ROOT']) / 'custom.json').write_text(json.dumps(sys.argv[1:]))
""")
executable("yazi", """
import os, sys
from pathlib import Path
root = Path(os.environ['TEST_ROOT'])
(root / 'yazi-hint').write_text(os.environ['INS_MENU_PICKER_HINT'])
selection = root / 'yazi-selection'
if selection.exists():
    Path(sys.argv[sys.argv.index('--chooser-file') + 1]).write_text(selection.read_text())
""")
picker = Picker()
picker.wait_for("Current default")
picker.send(b"\x05")
deadline = time.monotonic() + 5
while not (root / 'yazi-hint').exists() and time.monotonic() < deadline:
    picker.read()
assert (root / 'yazi-hint').read_text() == "Choose an executable to open the item"
picker.wait_for("Current default")
picker.finish()

(root / 'yazi-selection').write_text(str(file))
picker = Picker()
picker.wait_for("Current default")
picker.send(b"\x05")
picker.wait_for("Cannot execute")
picker.finish()

old_history = history.read_bytes()
(root / 'yazi-selection').write_text(str(custom))
picker = Picker()
picker.wait_for("Current default")
picker.finish(b"\x05")
deadline = time.monotonic() + 5
while not (root / 'custom.json').exists() and time.monotonic() < deadline:
    time.sleep(0.02)
assert json.loads((root / 'custom.json').read_text()) == [str(file)]
custom_key = f'text/plain\nexec:{custom}'
assert json.loads(history.read_text())['entries'][custom_key]['num_accesses'] >= 1
assert len((root / "defaults.jsonl").read_text().splitlines()) == 1

# --gui starts a graphical terminal without requiring an existing TTY.
subprocess.run([binary, "open", "--gui", str(file)], check=True, capture_output=True)
deadline = time.monotonic() + 5
while not terminal_record.exists() and time.monotonic() < deadline:
    time.sleep(0.02)
terminal_args = json.loads(terminal_record.read_text())
assert terminal_args[-2:] == ["open", str(file)], terminal_args
assert os.access(terminal_args[-3], os.X_OK), terminal_args

# Explicit renderers are mutually exclusive.
conflict = subprocess.run(
    [binary, 'open', '--gui', '--instantmenu', str(file)], capture_output=True,
)
assert conflict.returncode == 2

# Graphical mode uses the same controller without requiring an interactive TTY.
# Mock only the external renderer and hosted terminal protocol.
queue = root / "instantmenu-actions.json"
menu_log = root / "instantmenu.jsonl"
executable("instantmenu", r"""
import json, os, re, sys
from pathlib import Path
root = Path(os.environ['TEST_ROOT'])
# Early cancellation must not wait for a stalled MIME/discovery producer.
if (root / 'cancel-immediately').exists():
    import time
    deadline = time.monotonic() + 3
    while not (root / 'mime-waiting').exists() and time.monotonic() < deadline:
        time.sleep(0.01)
    sys.exit(1)
rows = sys.stdin.read()
with (root / 'instantmenu.jsonl').open('a') as stream:
    stream.write(json.dumps({'args': sys.argv[1:], 'rows': rows}) + '\n')
confirmation = '--bind' not in sys.argv and 'confirmation' in sys.argv
if '--bind' not in sys.argv and not confirmation:
    sys.exit(0)  # Error message dialog.
queue = root / 'instantmenu-actions.json'
actions = json.loads(queue.read_text())
action = actions.pop(0)
queue.write_text(json.dumps(actions))
if confirmation:
    answer = action['confirm']
    if answer == 'cancel':
        sys.exit(1)
    print(answer)
    sys.exit(0)
if action.get('cancel'):
    print('instantmenu: warning: --width auto requires measuring all 300 items; use a fixed width for large lists', file=sys.stderr)
    sys.exit(1)
if action.get('failure'):
    print('instantmenu: could not connect to display', file=sys.stderr)
    sys.exit(1)
if 'raw' in action:
    print(action['raw'])
    sys.exit(0)
print(action.get('key', ''))
if 'custom' in action:
    row = next(line for line in rows.splitlines() if '(custom · ' + action['custom'] + ')' in line)
    print(re.search(r'value=(\d+)', row).group(1))
if 'desktop' in action:
    row = next(line for line in rows.splitlines() if '(' + action['desktop'] + ')' in line)
    print(re.search(r'value=(\d+)', row).group(1))
""")


def graphical(actions, path=file, status=0):
    queue.write_text(json.dumps(actions))
    result = subprocess.run(
        [binary, "open", "--instantmenu", str(path)],
        stdin=subprocess.DEVNULL, capture_output=True, text=True, timeout=10,
    )
    assert result.returncode == status, result.stderr
    assert json.loads(queue.read_text()) == [], queue.read_text()
    return result


def menus():
    return [json.loads(line) for line in menu_log.read_text().splitlines()]


# Install our real desktop entry in the isolated catalog: it is a normal choice.
(applications / 'ins-open.desktop').write_text(
    (Path(__file__).resolve().parent.parent / 'desktop/ins-open.desktop').read_text()
)
old_history = history.read_bytes()
before_cancel = len(menus()) if menu_log.exists() else 0
graphical([{'cancel': True}])
assert len(menus()) == before_cancel + 1, 'Cancellation opened an error dialog'
record = menus()[-1]
assert '--reject-no-match' in record['args']
assert record['args'][record['args'].index('--prompt') + 1] == ''
assert record['args'][record['args'].index('--placeholder') + 1] == f'Open With · {file}'
assert 'ctrl-d:set default' in record['args']
assert 'ctrl-e:custom executable' in record['args']
assert 'ctrl-x:make executable and run' in record['args']
assert '{heading fade} Current default' in record['rows']
assert record['args'][record['args'].index('--width') + 1] == '640'
for row in record['rows'].splitlines():
    if row.startswith('{heading'):
        assert 'icon=' not in row
    else:
        assert 'icon=mdapplication' in row
        assert row.startswith(('{icon=mdapplication ', '{green icon=mdapplication ')), row
assert '↗' not in record['rows']
assert 'Second Editor (second.desktop) (current)' in record['rows']
assert 'INS Open (ins-open.desktop)' in record['rows']
assert 'Hidden Application' not in record['rows']
assert '\x1b' not in record['rows']
assert history.read_bytes() == old_history
assert len(launches()) == 2

# Ctrl+D saves immediately, then both renderers see the new default.
graphical([{'key': 'ctrl-d', 'desktop': 'editor.desktop'}, {'cancel': True}])
assert json.loads((root / 'defaults.jsonl').read_text().splitlines()[-1]) == ['editor.desktop', 'text/plain']
assert 'Default set to Default Editor' in menus()[-1]['rows']
assert 'Default Editor (editor.desktop) (current)' in menus()[-1]['rows']
assert history.read_bytes() == old_history
assert len(launches()) == 2
picker = Picker()
picker.wait_for('Desktop ID: editor.desktop')
picker.wait_focused('Default Editor')
picker.finish()

graphical([{'desktop': 'second.desktop'}])
await_launches(3)
assert launches()[-1] == ['second', str(file)]
assert len((root / 'defaults.jsonl').read_text().splitlines()) == 2

# Unknown/typed output cannot become a launch. Failures show a graphical message.
old_history = history.read_bytes()
graphical([{'raw': '\nunknown'}], status=1)
assert 'invalid selection' in menus()[-1]['rows']
assert len(launches()) == 3
assert history.read_bytes() == old_history

# Genuine renderer errors still surface, including a graphical error message.
graphical([{'failure': True}], status=1)
assert 'could not connect to display' in menus()[-1]['rows']

# INS Open can be set as the default from its own picker, so a file type always
# asks which application to use. It stays visible as the current default.
graphical([{'key': 'ctrl-d', 'desktop': 'ins-open.desktop'}, {'cancel': True}])
assert json.loads((root / 'defaults.jsonl').read_text().splitlines()[-1]) == ['ins-open.desktop', 'text/plain']
assert 'Default set to INS Open' in menus()[-1]['rows']
assert 'INS Open (ins-open.desktop) (current)' in menus()[-1]['rows']

# The GUI hosts Yazi externally using the existing file-picker transport.
# Force dwm so the test uses the transient-terminal fallback on any host.
os.environ.pop('INSTANTWM', None)
os.environ['XDG_SESSION_DESKTOP'] = 'dwm'
executable('foot', r"""
import json, os, sys
from pathlib import Path
root = Path(os.environ['TEST_ROOT'])
args = sys.argv[1:]
request = json.loads(Path(args[args.index('--request-file') + 1]).read_text())
(root / 'hosted-picker.json').write_text(json.dumps(request))
response = json.loads((root / 'hosted-response.json').read_text())
Path(args[args.index('--response-file') + 1]).write_text(json.dumps(response))
""")
(root / 'hosted-response.json').write_text(json.dumps('Cancelled'))
graphical([{'key': 'ctrl-e'}, {'cancel': True}])
request = json.loads((root / 'hosted-picker.json').read_text())
assert request['FilePicker']['scope'] == 'Files'
assert request['FilePicker']['allow_multiple'] is False
(root / 'hosted-response.json').write_text(json.dumps({'FilePickerResult': [str(custom)]}))
(root / 'custom.json').unlink()
graphical([{'key': 'ctrl-e'}])
deadline = time.monotonic() + 5
while not (root / 'custom.json').exists() and time.monotonic() < deadline:
    time.sleep(0.02)
assert json.loads((root / 'custom.json').read_text()) == [str(file)]
custom_key = f'text/plain\nexec:{custom}'
assert json.loads(history.read_text())['entries'][custom_key]['num_accesses'] >= 1

# Remembered executables are shared by both menus and scoped to the file type.
graphical([{'cancel': True}])
assert f'(custom · {custom})' in menus()[-1]['rows']
graphical([{'cancel': True}], path=image)
assert str(custom) not in menus()[-1]['rows']
other_file = root / 'another file.txt'
other_file.write_text('another document')
(root / 'custom.json').unlink()
graphical([{'custom': str(custom)}], path=other_file)
deadline = time.monotonic() + 5
while not (root / 'custom.json').exists() and time.monotonic() < deadline:
    time.sleep(0.02)
assert json.loads((root / 'custom.json').read_text()) == [str(other_file)]
assert json.loads(history.read_text())['entries'][custom_key]['num_accesses'] == 3
(root / 'custom.json').unlink()
picker = Picker(other_file)
picker.wait_for('Open With')
picker.send(b'custom executable')
picker.wait_for('Executable:')
picker.finish(b'\r')
deadline = time.monotonic() + 5
while not (root / 'custom.json').exists() and time.monotonic() < deadline:
    time.sleep(0.02)
assert json.loads((root / 'custom.json').read_text()) == [str(other_file)]
assert json.loads(history.read_text())['entries'][custom_key]['num_accesses'] == 4

# Missing or non-executable handlers are removed on discovery, including on cancel.
custom.chmod(0o644)
graphical([{'cancel': True}])
assert str(custom) not in menus()[-1]['rows']
assert custom_key not in json.loads(history.read_text())['entries']
custom.chmod(0o755)
graphical([{'key': 'ctrl-e'}])
custom.unlink()
graphical([{'cancel': True}])
assert custom_key not in json.loads(history.read_text())['entries']
assert str(custom) not in menus()[-1]['rows']

# Ctrl+X runs the opened file itself, adding only owner execution permission.
program = root / 'run me; $literal.AppImage'
program.write_text("#!/usr/bin/env python3\n" + r"""
import json, sys
from pathlib import Path
result = Path(__file__).parent / 'executed.json'
temp = result.with_suffix('.tmp')
temp.write_text(json.dumps(sys.argv))
temp.replace(result)
""")
program.chmod(0o640)
executed = root / 'executed.json'
old_history = history.read_bytes()
old_defaults = (root / 'defaults.jsonl').read_bytes()
picker = Picker(program)
picker.wait_for('Open With')
picker.finish()
assert program.stat().st_mode & 0o777 == 0o640
assert not executed.exists()


def await_execution():
    deadline = time.monotonic() + 5
    while not executed.exists() and time.monotonic() < deadline:
        time.sleep(0.02)
    assert json.loads(executed.read_text()) == [str(program)]
    assert program.stat().st_mode & 0o777 == 0o740
    assert history.read_bytes() == old_history
    assert (root / 'defaults.jsonl').read_bytes() == old_defaults
    assert len(launches()) == 3
    executed.unlink()


picker = Picker(program)
picker.wait_for('Open With')
picker.finish(b'\x18')
await_execution()
program.chmod(0o640)
graphical([{'key': 'ctrl-x'}], path=program)
await_execution()
# Already executable programs can be launched again without changing their mode.
graphical([{'key': 'ctrl-x'}], path=program)
await_execution()

# A directory never advertises direct execution or changes its permissions.
directory_mode = root.stat().st_mode
graphical([{'cancel': True}], path=root)
assert not any(arg.startswith('ctrl-x:') for arg in menus()[-1]['args'])
assert root.stat().st_mode == directory_mode

# Unrecognized formats require confirmation before changing permissions.
invalid_program = root / 'invalid-program'
invalid_program.write_text('not an executable format\n')
invalid_program.chmod(0o640)
for answer in ['no', 'cancel']:
    start = len(menus())
    graphical([{'key': 'ctrl-x'}, {'confirm': answer}, {'cancel': True}], path=invalid_program)
    assert 'unlikely to execute successfully' in menus()[start + 1]['rows']
    assert 'no recognized executable header' in menus()[start + 1]['rows']
    assert invalid_program.stat().st_mode & 0o777 == 0o640

# Confirmation can also be cancelled in the terminal, returning to Open With.
picker = Picker(invalid_program)
picker.wait_for('Open With')
picker.send(b'\x18')
picker.wait_for('unlikely to execute successfully')
assert invalid_program.stat().st_mode & 0o777 == 0o640
checkpoint = len(picker.output)
picker.send(b'\x1b')
# Wait for the replacement picker to draw; the VT reconstruction can retain
# Open With text from the menu underneath the confirmation.
deadline = time.monotonic() + 10
while b'Open With' not in picker.output[checkpoint:]:
    picker.read()
    assert time.monotonic() < deadline, picker.screen()
picker.finish()
assert invalid_program.stat().st_mode & 0o777 == 0o640

# An explicit Yes still attempts execution and reports the actual launch error.
graphical([{'key': 'ctrl-x'}, {'confirm': 'yes'}], path=invalid_program, status=1)
assert invalid_program.stat().st_mode & 0o777 == 0o740
assert 'Failed to execute' in menus()[-1]['rows']
assert history.read_bytes() == old_history
assert (root / 'defaults.jsonl').read_bytes() == old_defaults
assert len(launches()) == 3

# The GUI opens and cancels even while MIME detection is blocked.
(root / 'cancel-immediately').touch()
(root / 'mime-waiting').unlink(missing_ok=True)
os.mkfifo(gate)
subprocess.run([binary, 'open', '--instantmenu', str(file)],
               stdin=subprocess.DEVNULL, capture_output=True, check=True, timeout=5)
with gate.open('w') as stream:
    stream.write('continue')
gate.unlink()
(root / 'cancel-immediately').unlink()

assert probe_log.read_text().splitlines() == ["probe"], "File preview repeated MIME detection"

# xdg-utils #151: real xdg-mime skips a saved default with a quoted Exec path.
# Exercise saving, refreshing, reopening, and launching without modifying the launcher.
assert real_xdg_mime, "xdg-mime is required for the quoted default regression"
os.environ.update(XDG_CURRENT_DESKTOP="generic", XDG_CONFIG_DIRS=str(root / "empty-config"))
executable("xdg-mime", f"""
import os, sys
if sys.argv[1:3] == ['query', 'filetype']:
    print('text/plain')
else:
    os.execv({real_xdg_mime!r}, [{real_xdg_mime!r}, *sys.argv[1:]])
""")
quoted_launcher = executable("quoted app", launcher.read_text().split("\n", 1)[1])
quoted_desktop = applications / "quoted.desktop"
quoted_content = (
    '[Desktop Entry]\nType=Application\nName=Quoted Editor\n'
    f'Exec="{quoted_launcher}" quoted %F\n'
)
quoted_desktop.write_text(quoted_content)
(Path(os.environ["XDG_CONFIG_HOME"]) / "mimeapps.list").write_text(
    '[Default Applications]\ntext/plain=editor.desktop;\n'
)
picker = Picker()
picker.wait_focused("Default Editor")
picker.send(b"Quoted Editor")
picker.wait_for("Desktop ID: quoted.desktop")
picker.send(b"\x04")
picker.wait_for("Default set to Quoted Editor")
picker.wait_focused("Quoted Editor")
picker.finish()
picker = Picker()
picker.wait_focused("Quoted Editor")
picker.finish(b"\r")
await_launches(4)
assert launches()[-1] == ["quoted", str(file)]
assert quoted_desktop.read_text() == quoted_content

print("Open With terminal and Instant Menu integration tests passed")
