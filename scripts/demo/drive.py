#!/usr/bin/env python3
"""Plays the demo against a real `kb` in a pseudo-terminal and writes an asciinema v2 cast.

    drive.py <kb-binary> <kb-root> <out.cast> [--peek]

No terminal is needed: this script is the terminal. --peek prints the screen at each `mark`
(needs `pip install pyte`) so the storyboard can be checked without rendering a GIF.
"""
import fcntl, json, os, pty, select, struct, sys, termios, time, tempfile

kb, root, out = sys.argv[1:4]
peek = "--peek" in sys.argv
COLS, ROWS = 110, 30

work = tempfile.mkdtemp(prefix="kb-demo-")
bindir = os.path.join(work, "bin")
os.makedirs(bindir)
clip = os.path.join(work, "clip.txt")
for name, body in {
    "pbcopy": f"cat > '{clip}'",
    "pbpaste": f"cat '{work}/paste.txt'",
    "open": "true",
}.items():
    p = os.path.join(bindir, name)
    open(p, "w").write("#!/bin/sh\n" + body + "\n")
    os.chmod(p, 0o755)
open(os.path.join(work, "paste.txt"), "w").write(
    "select pid, usename, state, now() - query_start as running_for\n"
    "from pg_stat_activity\nwhere state <> 'idle'\norder by running_for desc;\n"
)
home = os.path.dirname(os.path.abspath(root))  # so the header reads ~/kb

pid, fd = pty.fork()
if pid == 0:
    os.environ.clear()
    os.environ.update(
        HOME=home, KB_ROOT=root, TERM="xterm-256color", COLORTERM="truecolor",
        PATH=f"{bindir}:{os.path.dirname(os.path.abspath(kb))}:/usr/bin:/bin",
        PS1="\x1b[1;32m❯\x1b[0m ", LANG="en_US.UTF-8", ENV="",
    )
    os.chdir(home)
    os.execv("/bin/sh", ["sh", "-i"])
fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack("HHHH", ROWS, COLS, 0, 0))

events, start = [], time.time()
screen = stream = None
if peek:
    import pyte
    screen = pyte.Screen(COLS, ROWS)
    stream = pyte.Stream(screen)


def pump(seconds):
    end = time.time() + seconds
    while True:
        left = end - time.time()
        if left <= 0:
            return
        if select.select([fd], [], [], left)[0]:
            try:
                data = os.read(fd, 65536)
            except OSError:
                return
            if not data:
                return
            events.append([round(time.time() - start, 4), "o", data.decode("utf-8", "replace")])
            if stream:
                stream.feed(data.decode("utf-8", "replace").replace("\x1b[?1049l", "\x1b[2J\x1b[H").replace("\x1b[?1049h", "\x1b[2J\x1b[H"))


def key(b, wait=0.9):
    os.write(fd, b)
    pump(wait)


def type_(text, delay=0.055, wait=0.8):
    for ch in text:
        os.write(fd, ch.encode())
        pump(delay)
    pump(wait)


def mark(label):
    if peek:
        print(f"=== {label}")
        print("\n".join(l.rstrip() for l in screen.display).rstrip())


ENTER, ESC, TAB, BTAB, BS = b"\r", b"\x1b", b"\t", b"\x1b[Z", b"\x7f"
DOWN, UP = b"\x1b[B", b"\x1b[A"
CTRL = lambda c: bytes([ord(c) - 96])


def cmd(text, wait=1.6):
    type_(text, wait=0.5)
    key(ENTER, wait)


exec(open(os.path.join(os.path.dirname(os.path.abspath(__file__)), "storyboard.py")).read())

pump(1.5)
with open(out, "w") as f:
    f.write(json.dumps({"version": 2, "width": COLS, "height": ROWS,
                        "env": {"TERM": "xterm-256color", "SHELL": "/bin/sh"}}) + "\n")
    for e in events:
        f.write(json.dumps(e, ensure_ascii=False) + "\n")
os.close(fd)
