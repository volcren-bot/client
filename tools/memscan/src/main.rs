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

const NEAR: [i64; 8] = [-16, -12, -8, -4, 4, 8, 12, 16];

fn aligned<const N: usize>(base: u64, data: &[u8]) -> impl Iterator<Item = (usize, [u8; N])> + '_ {
    let start = (N - base as usize % N) % N;
    data.get(start..).unwrap_or_default().as_chunks::<N>().0.iter().enumerate().map(move |(k, &w)| (start + k * N, w))
}

fn near(data: &[u8], i: usize, d: i64) -> Option<[u8; 4]> {
    let j = usize::try_from(i as i64 + d).ok()?;
    data.get(j..j + 4)?.try_into().ok()
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
                for (i, w) in aligned::<8>(base, data) {
                    let v = u64::from_le_bytes(w);
                    let hit = match range { Some((lo, hi)) => v >= lo && v < hi, None => set.contains(&v) };
                    if hit { report(base + i as u64, format!(" -> {v:#x}")); }
                }
            });
        }
        "i32" => {
            let want = parse(&args[3]) as i64 as i32;
            scan(pid, |base, data| {
                for (i, w) in aligned::<4>(base, data) {
                    if i32::from_le_bytes(w) == want { report(base + i as u64, String::new()); }
                }
            });
        }
        "pair" => {
            let (a, b) = (parse(&args[3]) as i64 as i32, parse(&args[4]) as i64 as i32);
            scan(pid, |base, data| {
                for (i, w) in aligned::<4>(base, data) {
                    if i32::from_le_bytes(w) == a {
                        for d in NEAR {
                            if near(data, i, d).is_some_and(|w| i32::from_le_bytes(w) == b) {
                                report(base + i as u64, format!(" {d:+}"));
                            }
                        }
                    }
                }
            });
        }
        "fpair" => {
            let p = |i: usize| args[i].parse::<f32>().unwrap();
            let ((a0, a1), (b0, b1)) = ((p(3), p(4)), (p(5), p(6)));
            scan(pid, |base, data| {
                for (i, v) in aligned::<4>(base, data) {
                    let v = f32::from_le_bytes(v);
                    if v >= a0 && v <= a1 {
                        for d in NEAR {
                            if let Some(w) = near(data, i, d).map(f32::from_le_bytes).filter(|w| *w >= b0 && *w <= b1) {
                                report(base + i as u64, format!(" {v} {d:+} {w}"));
                            }
                        }
                    }
                }
            });
        }
        other => panic!("unknown mode {other}"),
    }
}
