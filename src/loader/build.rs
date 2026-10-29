//! Embeds `API_URL`, `SUPABASE_URL`, `SUPABASE_ANON_KEY` and `SITE_URL` from `client/.env` at build time; the same variables override them at run time.
use std::{fs, path::Path, process::Command};

const KEYS: [&str; 4] = ["API_URL", "SUPABASE_URL", "SUPABASE_ANON_KEY", "SITE_URL"];

/// `YYYY.MM.DD-<7 character commit>`: `VOLCREN_VERSION` from the release workflow, else the checked out commit, else `dev`.
fn version() -> String {
    let git = |args: &[&str]| {
        let output = Command::new("git").args(args).env("TZ", "UTC").output().ok()?;
        output.status.success().then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
    };
    std::env::var("VOLCREN_VERSION").ok().filter(|v| !v.is_empty()).unwrap_or_else(|| {
        let date = git(&["show", "-s", "--format=%cd", "--date=format-local:%Y.%m.%d", "HEAD"]);
        let commit = git(&["rev-parse", "--short=7", "HEAD"]);
        date.zip(commit).map_or_else(|| "dev".to_owned(), |(date, commit)| format!("{date}-{commit}"))
    })
}

fn main() {
    println!("cargo:rerun-if-env-changed=VOLCREN_VERSION");
    println!("cargo:rerun-if-changed=../../.git/HEAD");
    println!("cargo:rustc-env=VOLCREN_VERSION={}", version());
    // Read at run time, not with `env!`, so a cached build script doesn't keep looking at the directory the project used to live in.
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").expect("cargo sets CARGO_MANIFEST_DIR");
    let env_file = Path::new(&manifest_dir).join("../../.env");
    println!("cargo:rerun-if-changed={}", env_file.display());
    let contents = fs::read_to_string(&env_file).unwrap_or_default();
    for key in KEYS {
        println!("cargo:rerun-if-env-changed={key}");
        let from_file = contents.lines().find_map(|line| {
            let (name, value) = line.trim().split_once('=')?;
            (name.trim() == key).then(|| value.trim().trim_matches(['"', '\'']).to_owned())
        });
        if let Some(value) = std::env::var(key).ok().or(from_file).filter(|value| !value.is_empty()) {
            println!("cargo:rustc-env=VOLCREN_{key}={value}");
        }
    }
}
