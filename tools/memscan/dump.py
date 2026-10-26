#!/usr/bin/env python3
"""Dump i32 fields of memory regions to a file for diffing.

Usage: dump.py <pid> <out.npz-less text> <start:hex> <len:hex> [<start> <len> ...]
Writes lines "addr_offset_from_first_start value" — actually writes one file per
region with lines "offsethex value" plus a header with the region start.
"""
import struct
import sys

pid = int(sys.argv[1])
out = sys.argv[2]
regions = []
for i in range(3, len(sys.argv), 2):
    if i + 1 >= len(sys.argv):
        break
    regions.append((int(sys.argv[i], 16), int(sys.argv[i + 1], 16)))

mem = open(f"/proc/{pid}/mem", "rb")
with open(out, "w") as f:
    for start, length in regions:
        mem.seek(start)
        data = mem.read(length)
        f.write(f"#region {start:#x} {length:#x}\n")
        for off in range(0, length - 3, 4):
            v = struct.unpack_from("<i", data, off)[0]
            f.write(f"{off:06x} {v}\n")
print(f"dumped {len(regions)} regions to {out}")
