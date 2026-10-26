use std::collections::HashSet;
use std::fs::File;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};

fn regions(pid: u32) -> Vec<(u64, u64, String)> {
    let f = BufReader::new(File::open(format!("/proc/{pid}/maps")).unwrap());
    let mut out = Vec::new();
    for line in f.lines().map_while(Result::ok) {
        let parts: Vec<&str> = line.split_whitespace().collect();
        let (a, b) = parts[0].split_once('-').unwrap();
        let (a, b) = (u64::from_str_radix(a, 16).unwrap(), u64::from_str_radix(b, 16).unwrap());
        if !parts[1].starts_with("rw") || a >= 0x7fff_ffff_0000 {
            continue;
        }
        out.push((a, b, parts.get(5).unwrap_or(&"").to_string()));
    }
    out
}

fn scan(pid: u32, mut visit: impl FnMut(u64, &[u8])) {
    let mut mem = File::open(format!("/proc/{pid}/mem")).unwrap();
    const CHUNK: u64 = 4 << 20;
    let mut buf = vec![0u8; CHUNK as usize + 64];
    for (a, b, _) in regions(pid) {
        let mut pos = a;
        while pos < b {
            let n = (CHUNK + 64).min(b - pos) as usize;
            let whole = mem.seek(SeekFrom::Start(pos)).is_ok() && mem.read_exact(&mut buf[..n]).is_ok();
            if whole {
                visit(pos, &buf[..n]);
            } else {
                let mut page = pos;
                while page < pos + n as u64 {
                    let len = 4096.min((pos + n as u64 - page) as usize);
                    if mem.seek(SeekFrom::Start(page)).is_ok() && mem.read_exact(&mut buf[..len]).is_ok() {
                        visit(page, &buf[..len]);
                    }
                    page += 4096;
                }
            }
            pos += CHUNK;
        }
    }
}

fn parse(s: &str) -> u64 {
    let s = s.trim();
    if let Some(h) = s.strip_prefix("0x") { u64::from_str_radix(h, 16).unwrap() } else { s.parse::<i64>().unwrap() as u64 }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let pid: u32 = args[1].parse().unwrap();
    let mut seen = HashSet::new();
    let mut report = |addr: u64, extra: String| {
        if seen.insert(addr) {
            println!("{addr:#x}{extra}");
        }
    };
    match args[2].as_str() {
        "bytes" => {
            let hex = &args[3];
            let pat: Vec<u8> = (0..hex.len()).step_by(2).map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap()).collect();
            scan(pid, |base, data| {
                if data.len() < pat.len() { return; }
                for i in 0..=data.len() - pat.len() {
                    if data[i] == pat[0] && data[i..i + pat.len()] == pat[..] {
                        report(base + i as u64, String::new());
                    }
                }
            });
        }
        "ptr" | "ptrrange" => {
            let (set, range): (HashSet<u64>, Option<(u64, u64)>) = if args[2] == "ptr" {
                let mut set = HashSet::new();
                for a in &args[3..] {
                    if let Some(path) = a.strip_prefix('@') {
                        for l in BufReader::new(File::open(path).unwrap()).lines().map_while(Result::ok) {
                            if let Some(tok) = l.split_whitespace().next() { set.insert(parse(tok)); }
                        }
                    } else {
                        set.insert(parse(a));
                    }
                }
                (set, None)
            } else {
                (HashSet::new(), Some((parse(&args[3]), parse(&args[4]))))
            };
            scan(pid, |base, data| {
                let start = ((8 - base % 8) % 8) as usize;
                let mut i = start;
                while i + 8 <= data.len() {
                    let v = u64::from_le_bytes(data[i..i + 8].try_into().unwrap());
                    let hit = match range { Some((lo, hi)) => v >= lo && v < hi, None => set.contains(&v) };
                    if hit { report(base + i as u64, format!(" -> {v:#x}")); }
                    i += 8;
                }
            });
        }
        "i32" => {
            let want = parse(&args[3]) as i64 as i32;
            scan(pid, |base, data| {
                let mut i = ((4 - base % 4) % 4) as usize;
                while i + 4 <= data.len() {
                    if i32::from_le_bytes(data[i..i + 4].try_into().unwrap()) == want { report(base + i as u64, String::new()); }
                    i += 4;
                }
            });
        }
        // two i32 values, the second at most 16 bytes after (or before) the first
        "pair" => {
            let (a, b) = (parse(&args[3]) as i64 as i32, parse(&args[4]) as i64 as i32);
            scan(pid, |base, data| {
                let mut i = ((4 - base % 4) % 4) as usize;
                while i + 4 <= data.len() {
                    if i32::from_le_bytes(data[i..i + 4].try_into().unwrap()) == a {
                        for d in [-16i64, -12, -8, -4, 4, 8, 12, 16] {
                            let j = i as i64 + d;
                            if j >= 0 && (j as usize) + 4 <= data.len() && i32::from_le_bytes(data[j as usize..j as usize + 4].try_into().unwrap()) == b {
                                report(base + i as u64, format!(" {d:+}"));
                            }
                        }
                    }
                    i += 4;
                }
            });
        }
        // two f32 ranges, the second at most 16 bytes after (or before) the first
        "fpair" => {
            let p = |i: usize| args[i].parse::<f32>().unwrap();
            let ((a0, a1), (b0, b1)) = ((p(3), p(4)), (p(5), p(6)));
            let f = |data: &[u8], i: usize| f32::from_le_bytes(data[i..i + 4].try_into().unwrap());
            scan(pid, |base, data| {
                let mut i = ((4 - base % 4) % 4) as usize;
                while i + 4 <= data.len() {
                    let v = f(data, i);
                    if v >= a0 && v <= a1 {
                        for d in [-16i64, -12, -8, -4, 4, 8, 12, 16] {
                            let j = i as i64 + d;
                            if j >= 0 && (j as usize) + 4 <= data.len() {
                                let w = f(data, j as usize);
                                if w >= b0 && w <= b1 {
                                    report(base + i as u64, format!(" {v} {d:+} {w}"));
                                }
                            }
                        }
                    }
                    i += 4;
                }
            });
        }
        other => panic!("unknown mode {other}"),
    }
}
