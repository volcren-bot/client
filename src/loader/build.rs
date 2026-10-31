//! Embeds the loader's version (`VOLCREN_VERSION`).
use std::process::Command;

/// `YYYY.MM.DD-<7 character commit>`: `VOLCREN_VERSION` from the release workflow, else the checked out commit, else `dev`.
fn version() -> String {
    let git = |args: &[&str]| {
        let output = Command::new("git")
            .args(args)
            .env("TZ", "UTC")
            .output()
            .ok()?;
        output
            .status
            .success()
            .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
    };
    std::env::var("VOLCREN_VERSION")
        .ok()
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| {
            let date = git(&[
                "show",
                "-s",
                "--format=%cd",
                "--date=format-local:%Y.%m.%d",
                "HEAD",
            ]);
            let commit = git(&["rev-parse", "--short=7", "HEAD"]);
            date.zip(commit).map_or_else(
                || "dev".to_owned(),
                |(date, commit)| format!("{date}-{commit}"),
            )
        })
}

fn main() {
    println!("cargo:rerun-if-env-changed=VOLCREN_VERSION");
    println!("cargo:rerun-if-changed=../../.git/HEAD");
    println!("cargo:rustc-env=VOLCREN_VERSION={}", version());
}
