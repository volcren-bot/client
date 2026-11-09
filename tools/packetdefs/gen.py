#!/usr/bin/env python3
import re
import struct
import sys
from pathlib import Path

WIDTH = r"(?:0xFF|255)"


def braced(text, open_at):
    depth, i = 0, open_at
    while i < len(text):
        depth += (text[i] == "{") - (text[i] == "}")
        i += 1
        if depth == 0:
            return text[open_at + 1 : i - 1]
    raise ValueError("unbalanced braces")


def methods(source):
    found = {}
    for m in re.finditer(r"\n   (?:(?:public|static|final|protected|private) )*[\w\[\].<>]+ (\w+)\(([^)]*)\) \{", source):
        found.setdefault(m.group(1), []).append((m.group(2), braced(source, m.end() - 1)))
    return found


def classify(body):
    text = re.sub(r"\s+", " ", body)
    reads = len(re.findall(r"\bah\[", text))
    if reads == 0:
        return None
    if re.search(r"ah\[[^\]]*\] != 0\)", text):
        return "str0" if re.search(r"if \(0? ?!?= ?\w+\) \{ throw|!= 0\) \{ throw|if \(0 != \w+\)", text) else "str"
    if "<< 7" in text and "& 127" in text:
        return "varint"
    if re.search(r"< 128 \? \w+\.cc\(-?\d+\) : ra\(\w+, \(byte\)-?\d+\) - 32768;", text):
        return "smart"
    if reads == 1:
        if re.search(r"\(byte\)\(128 - ", text):
            return "i8sub"
        if re.search(r"\(byte\)\(0 - ", text):
            return "i8neg"
        if re.search(r"\(byte\)\(.*ah\[[^\]]*\] - 128\)", text):
            return "i8add"
        if re.search(r"return \w+\.ah\[[^\]]*\];", text):
            return "i8"
        if re.search(rf"return 128 - [\w.]*ah\[[^\]]*\] & {WIDTH};", text):
            return "u8sub"
        if re.search(rf"return 0 - [\w.]*ah\[[^\]]*\] & {WIDTH};", text):
            return "u8neg"
        if re.search(rf"return [\w.]*ah\[[^\]]*\] - 128 & {WIDTH};", text):
            return "u8add"
        if re.search(rf"return [\w.]*ah\[[^\]]*\] & {WIDTH};", text):
            return "u8"
        return None
    term = re.compile(rf"\(\(?[\w.]*ah\[[^\]]*? - (\d)\]( - 128)? ?& ?{WIDTH}\)(?: ?<< ?(\d+))?")
    terms = term.findall(text)
    if len(terms) != reads or reads not in (2, 3, 4):
        return None
    shifts = [None] * reads
    adjusted = None
    for k, add, shift in terms:
        index = reads - int(k)
        if not 0 <= index < reads or shifts[index] is not None:
            return None
        shifts[index] = int(shift or 0)
        if add:
            adjusted = shifts[index]
    if sorted(shifts) != list(range(0, reads * 8, 8)) or adjusted not in (None, 0):
        return None
    limit = {2: ("32767", "65536"), 3: ("8388607", "16777216"), 4: (None, None)}[reads]
    sign_test = re.findall(r"if \(\w+ > (-?\d+)\)", text)
    signed = False
    if sign_test:
        if limit[0] is None or sign_test[0] != limit[0] or f"-= {limit[1]}" not in text:
            return None
        signed = True
    if reads == 4 and "-=" in text:
        return None
    sign = "i" if signed or reads == 4 else "u"
    order = "be" if shifts == sorted(shifts, reverse=True) else "le" if shifts == sorted(shifts) else ":" + ".".join(str(s // 8) for s in shifts)
    return f"{sign}{reads * 8}{order}{'add' if adjusted == 0 else ''}"


WIDTHS = {"bool": 1, "u8": 1, "i8": 1, "u16": 2, "i16": 2, "u24": 3, "i24": 3, "i32": 4, "u32": 4}


def kind_width(kind):
    m = re.match(r"[ui]\d+|bool", kind)
    return WIDTHS.get(m.group(0)) if m else None


def reads_in(block, buffer, classes):
    out = []
    depth = 0
    for m in re.finditer(r"[{}]|\b(\w+)\.(\w+)\(\s*(\w+)?", block):
        if m.group(0) == "{":
            depth += 1
        elif m.group(0) == "}":
            depth -= 1
        else:
            owner, name, first = m.group(1), m.group(2), m.group(3)
            if owner == buffer and name in classes:
                out.append((classes[name], depth))
            elif first == buffer and owner in ("xw", "PacketBuffer") and name in classes:
                out.append((classes[name], depth))
            elif owner == buffer or first == buffer:
                out.append(("?", depth))
    return out


IGNORED_WORDS = {"return", "boolean", "false", "true", "null", "this", "throw", "IOException", "RuntimeException", "String", "length", "while", "super", "break", "continue", "static", "final", "float", "double", "short", "class", "else", "case", "default", "switch", "catch", "Integer", "Object"}


def hints(block):
    found = []
    for word in re.findall(r"\b[A-Za-z_]\w{4,}\b", block):
        if word not in IGNORED_WORDS and not re.fullmatch(r"var\d+|do_L\w+", word) and word not in found:
            found.append(word)
    return found[:4]


def load_names(path, build):
    by_opcode, by_hint = {}, {}
    if path.exists():
        for line in path.read_text().splitlines():
            if not line.strip() or line.startswith("#"):
                continue
            scope, key, name, *fields = line.split("\t")
            named = (name, fields[0] if fields else "")
            if key.startswith("hint:"):
                by_hint[key[5:]] = named
            elif scope == build:
                by_opcode[key if key.startswith("S") else int(key)] = named
    return by_opcode, by_hint


def main():
    args = sys.argv[1:]
    exe = None
    if "--exe" in args:
        i = args.index("--exe")
        exe = Path(args[i + 1])
        del args[i : i + 2]
    src = Path(args[0])
    build = re.sub(r"^client-|\.exe$", "", exe.name) if exe else "unknown"
    names_by_opcode, names_by_hint = load_names(Path(__file__).with_name("names.tsv"), build)

    xw_methods = methods((src / "xw.java").read_text())
    classes = {}
    for name, overloads in xw_methods.items():
        kinds = {classify(body) for _, body in overloads} - {None}
        if len(kinds) == 1:
            classes[name] = kinds.pop()

    for name, overloads in xw_methods.items():
        for _, body in overloads:
            m = re.search(r"return \((?:\w+\.)?(\w+)\([^()]*\) & 1\) == 1;", body)
            if m and classes.get(m.group(1)) == "u8":
                classes[name] = "bool"

    jc = (src / "jc.java").read_text()
    packets = {}
    for m in re.finditer(r"jc (\w+) = new jc\((\d+), (-?\d+)\)", jc):
        packets[m.group(1)] = (int(m.group(2)), int(m.group(3)))
    by_opcode = {op: (name, size) for name, (op, size) in packets.items()}

    sent_sizes = {int(m.group(1)): int(m.group(2)) for m in re.finditer(r"jo \w+ = new jo\((\d+), (-?\d+)\)", (src / "jo.java").read_text())}

    client = (src / "Client.java").read_text()
    client_methods = methods(client)

    rows = []
    stats = {"layout": 0, "size only": 0}
    for opcode in sorted(by_opcode):
        name, size = by_opcode[opcode]
        layouts = set()
        touched = []
        for m in re.finditer(rf"(?:== jc\.{name}|jc\.{name} ==[^)]*)\) \{{", client):
            block = braced(client, m.end() - 1)
            buffer = "var6"
            call = re.match(r"\s*(?:boolean \w+ = )?(?:return )?(?:\w+\.)?(\w+)\((\w+), ", block)
            if call and call.group(1) in client_methods and "return" in block[:60]:
                for _, body in client_methods[call.group(1)]:
                    b = re.search(r"PacketBuffer (\w+) = \w+\.af", body)
                    if b:
                        block, buffer = body, b.group(1)
            owners = [o for o, n in re.findall(r"\b(var\d+)\.(\w+)\(", block) if n in classes]
            owners += [v for o, n, v in re.findall(r"\b(xw|PacketBuffer)\.(\w+)\((var\d+)", block) if n in classes]
            buffer = max(set(owners), key=owners.count) if owners else buffer
            reads = reads_in(block, buffer, classes)
            ok = all(kind != "?" and depth == 0 for kind, depth in reads)
            if not reads and re.search(r"\bvar\d+\.[a-z]\w*\(-?\d+\)|\b\w+\.\w+\(var\d+, -?\d+\)", block):
                ok = False
            kinds = [kind for kind, _ in reads]
            if ok and size >= 0 and kinds:
                total = [kind_width(kind) for kind in kinds]
                ok = None not in total and sum(total) == size
            layouts.add(tuple(kinds) if ok else None)
            touched += [h for h in hints(block) if h not in touched]
        good = {layout for layout in layouts if layout is not None}
        if size == 0:
            layout = ()
        elif layouts == {()}:
            layout = ("ignored",) if size > 0 else None
        elif len(good) == 1 and None not in layouts:
            layout = next(iter(good))
        else:
            layout = None
        stats["size only" if layout is None else "layout"] += 1
        label, field_names = names_by_opcode.get(opcode) or next((names_by_hint[h] for h in touched if h in names_by_hint), ("", ""))
        rows.append((opcode, size, label, layout, touched[:4], field_names))

    if exe:
        verify_exe(exe, [by_opcode[op][1] if op in by_opcode else None for op in range(max(by_opcode) + 1)])

    print("# Server packets of the native client: opcode, size (-1 a length byte follows, -2 a length short), fields.")
    print("# Generated by tools/packetdefs/gen.py from the decompiled Java client; do not edit.")
    print(f"# {stats['layout']} with a layout, {stats['size only']} with only a size")
    print(f"# build\t{build}")
    print("# R\topcode\tsize\tname\tfields (kind, in order)\tidentifiers the handler touches\tfield names")
    for opcode, size, name, layout, touched, field_names in rows:
        print("R", opcode, size, name, "" if layout is None else ",".join(layout) or "-", ",".join(touched), field_names, sep="\t")
    for key, (name, field_names) in sorted(names_by_opcode.items(), key=lambda kv: str(kv[0])):
        if isinstance(key, str):
            opcode = int(key[1:])
            size = sent_sizes.get(opcode)
            if size is None:
                print(f"gen: no client packet {opcode} for {name}", file=sys.stderr)
                continue
            print("S", opcode, size, name, "-" if size == 0 else "", "", field_names, sep="\t")
    print(f"gen: {stats}", file=sys.stderr)


def verify_exe(exe, sizes):
    data = exe.read_bytes()
    head = sizes[:12]
    if None in head:
        print("exe check: opcodes 0..11 not all known", file=sys.stderr)
        return
    needle = struct.pack(f"<{len(head)}i", *head)
    at = data.find(needle)
    if at < 0 or data.find(needle, at + 1) >= 0:
        print("exe check: the size table was not found exactly once", file=sys.stderr)
        return
    table = struct.unpack_from(f"<{len(sizes)}i", data, at)
    bad = [op for op, (a, b) in enumerate(zip(table, sizes)) if b is not None and a != b]
    print(f"exe check: table at file offset {at:#x}, {len(sizes)} opcodes, {len(bad)} differ {bad[:10]}", file=sys.stderr)


if __name__ == "__main__":
    main()
