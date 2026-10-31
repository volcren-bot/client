//! The Luau lib that store scripts `require("../lib/...")`: published to the `client` bucket and kept in the cache beside the script folders.

use std::{
    fs,
    io::{Cursor, Read},
    path::{Component, Path, PathBuf},
};

use serde::Deserialize;

use crate::{api::Api, session::sha256_hex, update::bucket_url};

const VERSION_FILE: &str = ".version";
const MAX_ARCHIVE_BYTES: u64 = 16 * 1024 * 1024;
const MAX_FILE_BYTES: u64 = 4 * 1024 * 1024;
const MAX_FILES: usize = 2000;

/// One published lib, as listed in `lib/index.json` (newest first, the last few releases).
#[derive(Debug, Deserialize, PartialEq)]
struct Release {
    version: String,
    /// The `HOST_API` the loader needs to run this lib.
    requires_host: u32,
    /// Path inside the `client` bucket.
    path: String,
    sha256: String,
}

#[derive(Debug, Deserialize)]
struct Index {
    versions: Vec<Release>,
}

/// The lib to use: the newest the loader can run, and whether a newer one exists that it can't.
fn choose(releases: &[Release], host_api: u32) -> Option<(&Release, bool)> {
    let position = releases
        .iter()
        .position(|release| release.requires_host <= host_api)?;
    Some((&releases[position], position > 0))
}

/// What `ensure` found.
#[derive(Debug, PartialEq, Eq)]
pub struct Lib {
    pub version: String,
    /// Set when a newer lib is published that needs a newer loader.
    pub warning: Option<String>,
}

const OUTDATED: &str = "A newer script library is available but needs a newer loader, so this script runs with an older one and may not work properly. Update the loader.";

/// Makes sure `<root>/lib` is the newest published lib this loader can run; offline, a cached lib is used.
pub fn ensure(api: &Api, root: &Path) -> Result<Lib, String> {
    let dir = root.join("lib");
    let installed = fs::read_to_string(dir.join(VERSION_FILE))
        .ok()
        .map(|v| v.trim().to_owned());
    let cached = |why: String| match &installed {
        Some(version) => {
            eprintln!(
                "lib: using the cached copy ({version}), couldn't check for a newer one: {why}"
            );
            Ok(Lib {
                version: version.clone(),
                warning: None,
            })
        }
        None => Err(format!("Couldn't get the script library: {why}")),
    };
    let index = match fetch_index(api) {
        Ok(index) => index,
        Err(error) => return cached(error),
    };
    let Some((release, outdated)) = choose(&index.versions, injector::HOST_API) else {
        return cached("no published script library works with this loader, update it".into());
    };
    let warning = outdated.then(|| OUTDATED.to_owned());
    if installed.as_deref() != Some(release.version.as_str())
        && let Err(error) = install(api, root, release)
    {
        return cached(error).map(|lib| Lib { warning, ..lib });
    }
    Ok(Lib {
        version: release.version.clone(),
        warning,
    })
}

fn fetch_index(api: &Api) -> Result<Index, String> {
    let mut response = api
        .agent()
        .get(&bucket_url(api, "lib/index.json"))
        .call()
        .map_err(|e| e.to_string())?;
    if !response.status().is_success() {
        return Err(format!("the server answered {}", response.status()));
    }
    response
        .body_mut()
        .read_json()
        .map_err(|e| format!("unexpected lib index: {e}"))
}

fn install(api: &Api, root: &Path, manifest: &Release) -> Result<(), String> {
    if manifest
        .path
        .split('/')
        .any(|part| part.is_empty() || part == "..")
    {
        return Err("the lib manifest has a path that can't be used".into());
    }
    let mut response = api
        .agent()
        .get(&bucket_url(api, &manifest.path))
        .call()
        .map_err(|e| format!("Couldn't download the script library: {e}"))?;
    if !response.status().is_success() {
        return Err(format!(
            "Couldn't download the script library: the server answered {}.",
            response.status()
        ));
    }
    let bytes = response
        .body_mut()
        .with_config()
        .limit(MAX_ARCHIVE_BYTES)
        .read_to_vec()
        .map_err(|e| format!("Couldn't download the script library: {e}"))?;
    if !manifest.sha256.eq_ignore_ascii_case(&sha256_hex(&bytes)) {
        return Err("The script library didn't match its checksum, so it was discarded.".into());
    }

    // Unpack beside the real folder and swap, so a failed or interrupted install never leaves half a lib.
    let staging = root.join("lib.new");
    let _ = fs::remove_dir_all(&staging);
    fs::create_dir_all(&staging).map_err(|e| format!("Couldn't create the cache folder: {e}"))?;
    let result = unpack(&bytes, &staging)
        .and_then(|()| {
            fs::write(staging.join(VERSION_FILE), &manifest.version).map_err(|e| e.to_string())
        })
        .and_then(|()| {
            let dir = root.join("lib");
            let _ = fs::remove_dir_all(&dir);
            fs::rename(&staging, &dir).map_err(|e| e.to_string())
        });
    if result.is_err() {
        let _ = fs::remove_dir_all(&staging);
    }
    result.map_err(|e| format!("Couldn't install the script library: {e}"))
}

/// A zip entry name as a relative path, only if it can't leave the folder it is unpacked into.
fn safe_entry(name: &str) -> Option<PathBuf> {
    if name.contains(['\\', '\0', ':']) {
        return None;
    }
    let path = Path::new(name);
    let plain = path.components().all(
        |part| matches!(part, Component::Normal(part) if !part.to_string_lossy().starts_with('.')),
    );
    (plain
        && matches!(
            path.extension().and_then(|e| e.to_str()),
            Some("luau" | "lua")
        ))
    .then(|| path.to_path_buf())
}

fn unpack(bytes: &[u8], into: &Path) -> Result<(), String> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|e| e.to_string())?;
    if archive.len() > MAX_FILES {
        return Err("the archive has too many files".into());
    }
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(|e| e.to_string())?;
        if entry.is_dir() {
            continue;
        }
        let name = entry.name().map_err(|e| e.to_string())?.into_owned();
        let relative = safe_entry(&name)
            .ok_or_else(|| format!("the archive has a file that can't be used: {name:?}"))?;
        let mut contents = Vec::new();
        entry
            .by_ref()
            .take(MAX_FILE_BYTES + 1)
            .read_to_end(&mut contents)
            .map_err(|e| e.to_string())?;
        if contents.len() as u64 > MAX_FILE_BYTES {
            return Err(format!("{} is too large", relative.display()));
        }
        let target = into.join(&relative);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        fs::write(&target, contents).map_err(|e| e.to_string())?;
    }
    if !into.join("host.luau").is_file() {
        return Err("the archive has no host.luau".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use zip::{CompressionMethod, ZipWriter, write::SimpleFileOptions};

    use super::*;

    fn archive(files: &[(&str, &str)]) -> Vec<u8> {
        let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
        for (name, text) in files {
            writer
                .start_file(
                    *name,
                    SimpleFileOptions::default().compression_method(CompressionMethod::Deflated),
                )
                .unwrap();
            writer.write_all(text.as_bytes()).unwrap();
        }
        writer.finish().unwrap().into_inner()
    }

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("volcren-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn entries_cannot_escape_the_lib_folder() {
        for bad in [
            "../x.luau",
            "/etc/x.luau",
            "a/../../x.luau",
            "a\\b.luau",
            "C:x.luau",
            ".hidden.luau",
            "a/.git/x.luau",
            "x.exe",
            "x",
        ] {
            assert_eq!(safe_entry(bad), None, "{bad:?}");
        }
        assert_eq!(safe_entry("host.luau"), Some(PathBuf::from("host.luau")));
        assert_eq!(
            safe_entry("main/interfaces/login.luau"),
            Some(PathBuf::from("main/interfaces/login.luau"))
        );
    }

    #[test]
    fn unpacks_a_lib_with_folders() {
        let dir = temp("lib-ok");
        unpack(
            &archive(&[
                ("host.luau", "return {}"),
                ("main/game_state.luau", "return 1"),
            ]),
            &dir,
        )
        .unwrap();
        assert_eq!(
            fs::read_to_string(dir.join("main/game_state.luau")).unwrap(),
            "return 1"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn refuses_archives_that_escape_or_lack_a_host() {
        let dir = temp("lib-bad");
        assert!(unpack(&archive(&[("host.luau", ""), ("../evil.luau", "")]), &dir).is_err());
        assert!(!dir.parent().unwrap().join("evil.luau").exists());
        let _ = fs::remove_dir_all(&dir);
        assert!(
            unpack(&archive(&[("main/x.luau", "")]), &temp("lib-bad"))
                .unwrap_err()
                .contains("host.luau")
        );
        assert!(unpack(b"not a zip", &temp("lib-bad")).is_err());
        let _ = fs::remove_dir_all(&dir);
    }

    fn release(version: &str, requires_host: u32) -> Release {
        Release {
            version: version.into(),
            requires_host,
            path: format!("lib/{version}/lib.zip"),
            sha256: String::new(),
        }
    }

    #[test]
    fn picks_the_newest_lib_the_loader_can_run() {
        let releases = [release("c", 3), release("b", 2), release("a", 1)];
        assert_eq!(
            choose(&releases, 3).map(|(r, outdated)| (r.version.as_str(), outdated)),
            Some(("c", false))
        );
        assert_eq!(
            choose(&releases, 2).map(|(r, outdated)| (r.version.as_str(), outdated)),
            Some(("b", true)),
            "c needs a newer loader"
        );
        assert_eq!(
            choose(&releases, 1).map(|(r, outdated)| (r.version.as_str(), outdated)),
            Some(("a", true))
        );
        assert!(choose(&releases, 0).is_none());
        assert!(choose(&[], 1).is_none());
    }

    #[test]
    fn parses_the_index_the_lib_workflow_writes() {
        let index: Index = serde_json::from_str(
            r#"{"versions":[{"version":"2026.10.26-abc1234","requires_host":1,"path":"lib/2026.10.26-abc1234/lib.zip","sha256":"ab"}]}"#,
        )
        .unwrap();
        assert_eq!(index.versions[0].requires_host, 1);
        assert_eq!(index.versions[0].path, "lib/2026.10.26-abc1234/lib.zip");
    }
}
