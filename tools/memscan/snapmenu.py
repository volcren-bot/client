#!/usr/bin/env python3
"""Print a labeled block of the menu-related i32 fields: snapmenu.py <pid> <label>"""
import struct
import sys

pid = int(sys.argv[1])
label = sys.argv[2]
MENU = 0x342BC00
MINI = 0x342BD40

f = open(f"/proc/{pid}/mem", "rb")

def i32s(addr, n):
    f.seek(addr)
    return struct.unpack(f"<{n}i", f.read(4 * n))

def q(addr):
    f.seek(addr)
    return struct.unpack("<Q", f.read(8))[0]

print(f"=== {label}")
print(f"open flag (menu+0x7c0): {q(MENU + 0x7c0):#x}")
print(f"entries: begin={q(MENU+0x228):#x} end={q(MENU+0x230):#x} count={(q(MENU+0x230)-q(MENU+0x228))//0x150}")
print(f"mouse iu (mini+0x168): {i32s(MINI+0x168, 2)}")
print(f"click xy phys (mini+0x3e8): {i32s(MINI+0x3e8, 2)}")
print("menu+0x2a0..0x2f0:")
vals = i32s(MENU + 0x2a0, 21)
for i, v in enumerate(vals):
    print(f"  menu+{0x2a0 + i*4:#05x} (mini+{0x368 + i*4:#05x}): {v}")
