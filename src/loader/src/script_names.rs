//! The names of the client's scripts, for the Script Inspector, fetched on demand.
//!
//! The cache has no script names (only a hash of each), so the names are the ones the community gave the scripts:
//! RuneLite's `cs2-scripts` repository has a file per decompiled script, named `[clientscript,name].cs2` or
//! `[proc,name].cs2`, whose first line is `// <script id>`. Nothing is bundled: the Script Inspector's Load names button
//! downloads the repository as one zip and keeps the names in memory. The ids are the cache's, so the names are right for
//! the build the repository was decompiled from, which is usually the current one.

use std::collections::HashMap;
use std::io::{Cursor, Read};
use std::time::Duration;

const URL: &str = "https://codeload.github.com/runelite/cs2-scripts/zip/refs/heads/master";
/// The repository is about 10 MB zipped.
const MAX_DOWNLOAD: u64 = 128 * 1024 * 1024;
const TIMEOUT: Duration = Duration::from_secs(120);
/// How much of a file is read to find its id: the first line is `// 1234`, or a decompiler error naming the script.
const HEAD: u64 = 256;

#[derive(Default)]
pub struct Names {
    by_id: HashMap<i32, String>,
    by_name: HashMap<String, Vec<i32>>,
    /// Which commit the names are from, for the tooltip.
    pub source: String,
}

/// `name` from `[clientscript,name].cs2` or `[proc,name].cs2`; scripts the decompiler couldn't name (`script4730`) have none.
fn script_name(file: &str) -> Option<&str> {
    let name = file
        .strip_suffix(".cs2")?
        .strip_prefix('[')?
        .strip_suffix(']')?;
    let (kind, name) = name.split_once(',')?;
    let unnamed = name
        .strip_prefix("script")
        .is_some_and(|rest| !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_digit()));
    (matches!(kind, "clientscript" | "proc") && !name.is_empty() && !unnamed).then_some(name)
}

/// The script id in the first line of a file: `// 6697`, or an error from the decompiler like `... in script 5474 not implemented`.
fn script_id(head: &str) -> Option<i32> {
    let line = head.lines().next()?.trim();
    let digits = line
        .strip_prefix("// ")
        .or_else(|| line.split_once("in script ").map(|(_, rest)| rest))?;
    let end = digits
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(digits.len());
    digits[..end].parse().ok()
}

impl Names {
    /// Names from (file name, start of the file) pairs; files that aren't named scripts, or whose id can't be read, are left out.
    fn from_files<'a>(files: impl Iterator<Item = (&'a str, String)>, source: String) -> Self {
        let mut names = Self {
            source,
            ..Self::default()
        };
        for (file, head) in files {
            let (Some(name), Some(id)) = (script_name(file), script_id(&head)) else {
                continue;
            };
            names.by_id.insert(id, name.to_owned());
            names
                .by_name
                .entry(name.to_ascii_lowercase())
                .or_default()
                .push(id);
        }
        names
    }

    /// Names from the repository's zip: the commit it is of is in the archive's comment.
    pub fn from_zip(zip: &[u8]) -> Result<Self, String> {
        let mut archive = zip::ZipArchive::new(Cursor::new(zip))
            .map_err(|e| format!("the download isn't a zip: {e}"))?;
        let source = String::from_utf8_lossy(archive.comment())
            .chars()
            .take(7)
            .collect();
        let mut files = Vec::new();
        for index in 0..archive.len() {
            let file = archive.by_index(index).map_err(|e| e.to_string())?;
            let Ok(path) = file.name() else { continue };
            let Some(base) = path
                .rsplit('/')
                .next()
                .filter(|base| script_name(base).is_some())
                .map(str::to_owned)
            else {
                continue;
            };
            let mut head = Vec::new();
            file.take(HEAD)
                .read_to_end(&mut head)
                .map_err(|e| format!("{base}: {e}"))?;
            files.push((base, String::from_utf8_lossy(&head).into_owned()));
        }
        let names = Self::from_files(
            files
                .iter()
                .map(|(file, head)| (file.as_str(), head.clone())),
            source,
        );
        if names.is_empty() {
            return Err("the download has no script names".into());
        }
        Ok(names)
    }

    /// Downloads the names; slow (the whole repository), so not on the UI thread.
    pub fn download() -> Result<Self, String> {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(TIMEOUT))
            .user_agent(concat!("volcren-loader/", env!("VOLCREN_VERSION")))
            .build()
            .into();
        let mut response = agent
            .get(URL)
            .call()
            .map_err(|e| format!("couldn't download the script names: {e}"))?;
        let zip = response
            .body_mut()
            .with_config()
            .limit(MAX_DOWNLOAD)
            .read_to_vec()
            .map_err(|e| format!("couldn't download the script names: {e}"))?;
        Self::from_zip(&zip)
    }

    pub fn len(&self) -> usize {
        self.by_id.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_id.is_empty()
    }

    /// The name of script `id`, if known.
    pub fn name(&self, id: i32) -> Option<&str> {
        self.by_id.get(&id).map(String::as_str)
    }

    /// The ids named `name`, in any case; a client script and a proc can share a name.
    pub fn ids(&self, name: &str) -> &[i32] {
        self.by_name
            .get(&name.to_ascii_lowercase())
            .map_or(&[], Vec::as_slice)
    }
}

#[cfg(test)]
pub fn names_for_test(pairs: &[(i32, &str)]) -> Names {
    let files: Vec<(String, String)> = pairs
        .iter()
        .map(|(id, name)| (format!("[clientscript,{name}].cs2"), format!("// {id}\n")))
        .collect();
    Names::from_files(
        files
            .iter()
            .map(|(file, head)| (file.as_str(), head.clone())),
        "test".into(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn only_named_scripts_have_names() {
        assert_eq!(
            script_name("[clientscript,npc_name_timer].cs2"),
            Some("npc_name_timer")
        );
        assert_eq!(script_name("[proc,hex_to_rgb].cs2"), Some("hex_to_rgb"));
        assert_eq!(script_name("[clientscript,script4730].cs2"), None);
        assert_eq!(
            script_name("[proc,script_helper].cs2"),
            Some("script_helper")
        );
        assert_eq!(script_name("README.md"), None);
        assert_eq!(script_name("[other,x].cs2"), None);
    }

    #[test]
    fn the_id_is_in_the_first_line_or_in_a_decompiler_error() {
        assert_eq!(script_id("// 3604\n[clientscript,x]()\n"), Some(3604));
        assert_eq!(
            script_id(
                "java.lang.IllegalStateException: Opcode 7204 in script 5474 not implemented"
            ),
            Some(5474)
        );
        assert_eq!(
            script_id("java.lang.IllegalStateException: Check failed."),
            None
        );
        assert_eq!(script_id(""), None);
    }

    #[test]
    fn ids_and_names_find_each_other() {
        let names = names_for_test(&[(6697, "npc_name_timer"), (2, "hex_to_rgb")]);
        assert_eq!(names.name(6697), Some("npc_name_timer"));
        assert_eq!(names.ids("NPC_name_timer"), [6697]);
        assert_eq!(
            (names.name(5), names.ids("nope").len(), names.len()),
            (None, 0, 2)
        );
    }

    #[test]
    fn a_zip_of_the_repository_gives_its_names_and_commit() {
        let mut bytes = Vec::new();
        {
            let mut zip = zip::ZipWriter::new(Cursor::new(&mut bytes));
            let options = zip::write::SimpleFileOptions::default();
            zip.set_comment("404eb7bdeadbeef");
            for (path, body) in [
                (
                    "cs2-scripts-master/scripts/[clientscript,npc_name_timer].cs2",
                    "// 6697\nbody\n",
                ),
                (
                    "cs2-scripts-master/scripts/[proc,broken].cs2",
                    "java.lang.IllegalStateException: x in script 12 y\n",
                ),
                (
                    "cs2-scripts-master/scripts/[clientscript,script4730].cs2",
                    "// 4730\n",
                ),
                ("cs2-scripts-master/README.md", "hello"),
            ] {
                zip.start_file(path, options).unwrap();
                zip.write_all(body.as_bytes()).unwrap();
            }
            zip.finish().unwrap();
        }
        let names = Names::from_zip(&bytes).unwrap();
        assert_eq!(
            (
                names.name(6697),
                names.name(12),
                names.name(4730),
                names.source.as_str()
            ),
            (Some("npc_name_timer"), Some("broken"), None, "404eb7b")
        );
        assert!(Names::from_zip(b"not a zip").is_err());
    }

    /// Needs the network: `cargo test -p loader -- --ignored downloads_the_real_repository`.
    #[test]
    #[ignore]
    fn downloads_the_real_repository() {
        let names = Names::download().unwrap();
        assert!(names.len() > 5000, "{} names", names.len());
        assert_eq!(names.name(6697), Some("npc_name_timer"));
        eprintln!("{} names at {}", names.len(), names.source);
    }
}
