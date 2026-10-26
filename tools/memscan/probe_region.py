#!/usr/bin/env python3
"""Probe the minimenu's stay-open region: open at an anchor, teleport to probe
points, and watch the client's menu-open flag to see which points keep it open.

The stay-open region is (per static analysis) the menu rect grown by a margin on
all sides; its boundary is found by bisection along the axes through the anchor.

Usage: WINEPREFIX=<prefix> [INJECTOR=<injector.exe>] probe_region.py <linux_pid> <wine_pid> <anchor_x> <anchor_y> [probe_x probe_y]
       (coordinates are physical canvas pixels; without a probe, runs bisection)
"""
import json
import os
import struct
import subprocess
import sys
import time

PID = int(sys.argv[1])
WPID = sys.argv[2]
AX, AY = int(sys.argv[3]), int(sys.argv[4])
MENU = 0x342BC00

WINEPREFIX = os.environ["WINEPREFIX"]
INJECTOR = os.environ.get("INJECTOR", "target/x86_64-pc-windows-gnu/release/injector.exe")

proc = subprocess.Popen(
    ["wine", INJECTOR, "serve"],
    stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True, bufsize=1,
    env={"WINEPREFIX": WINEPREFIX, "PATH": "/usr/bin:/bin", "HOME": os.environ["HOME"]},
)

def cmd(*args):
    proc.stdin.write(json.dumps(list(args)) + "\n")
    proc.stdin.flush()
    line = proc.stdout.readline()
    return json.loads(line)

mem = open(f"/proc/{PID}/mem", "rb")
def open_flag():
    mem.seek(MENU + 0x7c0)
    return struct.unpack("<Q", mem.read(8))[0] != 0

def mouse_iu():
    mem.seek(0x342BD40 + 0x168)
    return struct.unpack("<2i", mem.read(8))

def wait_flag(want, timeout=1.5):
    t0 = time.time()
    while time.time() - t0 < timeout:
        if open_flag() == want:
            return True
        time.sleep(0.02)
    return False

FAR = (60, 1750)

def ensure_closed():
    if open_flag():
        cmd("move", WPID, str(FAR[0]), str(FAR[1]))
        wait_flag(False)

def open_at_anchor():
    ensure_closed()
    cmd("move", WPID, str(AX), str(AY))
    time.sleep(0.15)
    cmd("click", WPID, "right", str(AX), str(AY))
    if not wait_flag(True):
        raise RuntimeError("menu did not open")

def probe(px, py, settle=0.45):
    """True when the menu is still open `settle` s after moving to (px, py)."""
    open_at_anchor()
    cmd("move", WPID, str(px), str(py))
    t0 = time.time()
    while time.time() - t0 < settle:
        if not open_flag():
            return False
        time.sleep(0.02)
    return open_flag()

def bisect(lo, hi, fixed_x, horizontal, inside_low):
    """Bisect along an axis. (lo, hi) physical range; inside is at the low end
    (inside_low) or high end. Returns the boundary (last inside value)."""
    assert probe(*((lo, fixed_x) if horizontal else (fixed_x, lo))) == inside_low
    assert probe(*((hi, fixed_x) if horizontal else (fixed_x, hi))) != inside_low
    while abs(hi - lo) > 2:
        mid = (lo + hi) // 2
        inside = probe(*((mid, fixed_x) if horizontal else (fixed_x, mid)))
        if inside == inside_low:
            lo = mid
        else:
            hi = mid
    return lo if inside_low else hi

if __name__ == "__main__":
    if len(sys.argv) > 5:
        px, py = int(sys.argv[5]), int(sys.argv[6])
        print("single probe", px, py, "->", "INSIDE" if probe(px, py) else "outside")
        sys.exit(0)
    print("anchor", AX, AY, "mouse iu now:", mouse_iu())
    # horizontal edges at anchor y
    left = bisect(150, AX, AY, horizontal=True, inside_low=False)   # inside at high end
    right = bisect(AX, 2700, AY, horizontal=True, inside_low=True)  # inside at low end
    # vertical edges at anchor x
    top = bisect(150, AY, AX, horizontal=False, inside_low=False)
    bottom = bisect(AY, 1750, AX, horizontal=False, inside_low=True)
    print(f"stay-open region (physical): x [{left}, {right}] y [{top}, {bottom}]")
    s = 1.7506113052368164
    print(f"stay-open region (iu): x [{left/s:.1f}, {right/s:.1f}] y [{top/s:.1f}, {bottom/s:.1f}]")
    print(f"anchor iu: {AX/s:.1f}, {AY/s:.1f}")
