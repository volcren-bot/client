#!/usr/bin/env python3
"""Watch i32 fields in regions of a process memory and report changes with timestamps.

Usage: watch.py <pid> <region_spec>...
region_spec: start,len[,label]   (hex values, e.g. 342bd40,800,minimenu)
"""
import struct
import sys
import time

pid = int(sys.argv[1])
regions = []
for spec in sys.argv[2:]:
    start, length, *label = spec.split(",")
    regions.append((int(start, 16), int(length, 16), label[0] if label else hex(int(start, 16))))

mem = open(f"/proc/{pid}/mem", "rb")
t0 = time.time()

def stamp():
    return f"[{time.time() - t0:7.2f}]"

def read(start, length):
    mem.seek(start)
    return mem.read(length)

def fields(data):
    return struct.unpack(f"<{len(data)//4}i", data[: len(data) // 4 * 4])

snapshots = [(start, length, label, fields(read(start, length))) for start, length, label in regions]

print(f"{stamp()} watching {len(regions)} regions", flush=True)
try:
    while True:
        time.sleep(0.15)
        for i, (start, length, label, old) in enumerate(snapshots):
            new = fields(read(start, length))
            if new != old:
                for j, (a, b) in enumerate(zip(old, new)):
                    if a != b:
                        def fmt(v):
                            if -0x10000 < v < 0x10000:
                                return str(v)
                            return hex(v & 0xFFFFFFFF)
                        print(f"{stamp()} {label}+{j*4:#06x}: {fmt(a)} -> {fmt(b)}", flush=True)
                snapshots[i] = (start, length, label, new)
except KeyboardInterrupt:
    pass
