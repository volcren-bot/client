use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
};

const MAX_PAGE_BYTES: usize = 100 * 1024;
const RESULTS: usize = 8;
const LINES_PER_RESULT: usize = 6;
const LINE_CHARS: usize = 160;

fn files(lib: &Path) -> Result<Vec<String>, String> {
    let mut found = Vec::new();
    walk(lib, &lib.join("docs"), "md", &mut found);
    walk(lib, &lib.join("examples"), "luau", &mut found);
    if found.is_empty() {
        return Err(format!(
            "The script library in {} has no docs. A newer library release ships them.",
            lib.display()
        ));
    }
    found.sort();
    Ok(found)
}

fn walk(lib: &Path, dir: &Path, extension: &str, found: &mut Vec<String>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk(lib, &path, extension, found);
        } else if path.extension().is_some_and(|e| e == extension)
            && let Ok(relative) = path.strip_prefix(lib)
        {
            found.push(relative.to_string_lossy().replace('\\', "/"));
        }
    }
}

fn module(page: &str) -> Option<String> {
    let name = page.strip_prefix("docs/")?.strip_suffix(".md")?;
    Some(format!("@lib/{name}"))
}

pub fn page(lib: &Path, name: &str) -> Result<String, String> {
    let relative = crate::libs::safe_entry(name.trim()).ok_or_else(|| {
        format!(
            "{name:?} isn't a library page. Use a path from a search, like docs/main/walker.md."
        )
    })?;
    let text = fs::read_to_string(lib.join(&relative))
        .map_err(|e| format!("Couldn't read {name}: {e}"))?;
    if text.len() <= MAX_PAGE_BYTES {
        return Ok(text);
    }
    let cut = text.floor_char_boundary(MAX_PAGE_BYTES);
    Ok(format!(
        "{}\n\n[cut at {MAX_PAGE_BYTES} bytes of {}]",
        &text[..cut],
        text.len()
    ))
}

pub fn index(lib: &Path) -> Result<String, String> {
    let files = files(lib)?;
    Ok(format!(
        "Library pages (read one with `page`; docs/main/walker.md documents require(\"@lib/main/walker\")):\n{}",
        files.join("\n")
    ))
}

struct Hit {
    path: String,
    terms: usize,
    score: usize,
    lines: Vec<String>,
}

pub fn search(lib: &Path, query: &str) -> Result<String, String> {
    let mut terms: Vec<String> = query
        .split(|c: char| c.is_whitespace() || c == ',')
        .filter(|t| !t.is_empty())
        .map(str::to_lowercase)
        .collect();
    terms.sort();
    terms.dedup();
    if terms.is_empty() {
        return index(lib);
    }
    let mut hits: Vec<Hit> = files(lib)?
        .into_iter()
        .filter_map(|path| {
            let text = fs::read_to_string(lib.join(&path)).ok()?;
            score(&path, &text, &terms)
        })
        .collect();
    if hits.is_empty() {
        return Ok(format!(
            "Nothing matches {query:?}. Try other words, or call without a query for the page list."
        ));
    }
    hits.sort_by_key(|hit| std::cmp::Reverse((hit.terms, hit.score)));
    let mut text = String::new();
    for hit in hits.iter().take(RESULTS) {
        text.push_str(&hit.path);
        if let Some(module) = module(&hit.path) {
            text.push_str(&format!("  (require(\"{module}\"))"));
        }
        text.push('\n');
        for line in &hit.lines {
            text.push_str(&format!("  {line}\n"));
        }
    }
    if hits.len() > RESULTS {
        text.push_str(&format!("{} more pages match.\n", hits.len() - RESULTS));
    }
    Ok(text)
}

fn score(path: &str, text: &str, terms: &[String]) -> Option<Hit> {
    let lower_path = path.to_lowercase();
    let mut matched = vec![false; terms.len()];
    let mut score = 0;
    let mut headings = Vec::new();
    let mut others = Vec::new();
    for line in text.lines() {
        let lower = line.to_lowercase();
        let heading = line.starts_with('#');
        let mut hits = 0;
        for (index, term) in terms.iter().enumerate() {
            let count = lower.matches(term.as_str()).count();
            if count > 0 {
                matched[index] = true;
                hits += count;
            }
        }
        if hits == 0 {
            continue;
        }
        score += if heading { hits * 3 } else { hits };
        let shown: String = line.trim().chars().take(LINE_CHARS).collect();
        if heading {
            headings.push(shown);
        } else {
            others.push(shown);
        }
    }
    for (index, term) in terms.iter().enumerate() {
        let count = lower_path.matches(term.as_str()).count();
        if count > 0 {
            matched[index] = true;
            score += count * 5;
        }
    }
    let terms = matched.iter().filter(|m| **m).count();
    if terms == 0 {
        return None;
    }
    headings.extend(others);
    headings.truncate(LINES_PER_RESULT);
    Some(Hit {
        path: path.to_owned(),
        terms,
        score,
        lines: headings,
    })
}

fn analyzer() -> PathBuf {
    let name = if cfg!(windows) {
        "luau-analyze.exe"
    } else {
        "luau-analyze"
    };
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join(name)))
        .filter(|path| path.is_file())
        .unwrap_or_else(|| PathBuf::from(name))
}

pub enum Draft<'a> {
    Source(&'a str),
    File(&'a Path),
}

pub fn check(workspace: &Path, draft: Draft<'_>) -> Result<String, String> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let (file, temporary) = match draft {
        Draft::Source(source) => {
            let file = workspace.join(format!(
                ".check-{}-{}.luau",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::write(&file, source)
                .map_err(|e| format!("Couldn't write {}: {e}", file.display()))?;
            (file, true)
        }
        Draft::File(path) => (
            std::path::absolute(path).map_err(|e| format!("{}: {e}", path.display()))?,
            false,
        ),
    };
    let output = Command::new(analyzer()).arg(&file).output();
    if temporary {
        let _ = fs::remove_file(&file);
    }
    let output = output.map_err(|e| format!("Couldn't run luau-analyze: {e}"))?;
    let said = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let prefix = format!("{}(", file.display());
    let shown = if temporary {
        "script("
    } else {
        prefix.as_str()
    };
    let diagnostics: Vec<String> = said
        .lines()
        .filter(|line| line.starts_with(&prefix))
        .map(|line| line.replacen(&prefix, shown, 1))
        .collect();
    if !diagnostics.is_empty() {
        return Ok(diagnostics.join("\n"));
    }
    if !output.status.success() && !said.contains(&prefix) {
        let hint = if cfg!(windows) {
            " luau-analyze needs the Microsoft Visual C++ runtime."
        } else {
            ""
        };
        return Err(format!(
            "luau-analyze failed ({}): {}.{hint}",
            output.status,
            said.trim()
        ));
    }
    Ok("No problems found.".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lib(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("volcren-mcp-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("docs/main")).unwrap();
        fs::create_dir_all(dir.join("examples")).unwrap();
        fs::write(
            dir.join("docs/main/walker.md"),
            "# Walker\n\n## walk_to\nWalks to a tile like a person.\n",
        )
        .unwrap();
        fs::write(
            dir.join("docs/main/bank.md"),
            "# Bank\nOpens the bank. You can walk there first.\n",
        )
        .unwrap();
        fs::write(dir.join("examples/walk_to.luau"), "print('walk')\n").unwrap();
        dir
    }

    #[test]
    fn search_ranks_pages_matching_more_terms_and_headings_first() {
        let lib = lib("search");
        let found = search(&lib, "walk tile").unwrap();
        let first = found.lines().next().unwrap();
        assert_eq!(
            first,
            "docs/main/walker.md  (require(\"@lib/main/walker\"))"
        );
        assert!(found.contains("## walk_to"));
        assert!(found.contains("examples/walk_to.luau"));
        assert!(search(&lib, "zzz").unwrap().starts_with("Nothing matches"));
        let _ = fs::remove_dir_all(&lib);
    }

    #[test]
    fn pages_are_read_only_from_inside_the_library() {
        let lib = lib("page");
        assert!(
            page(&lib, "docs/main/bank.md")
                .unwrap()
                .starts_with("# Bank")
        );
        assert!(page(&lib, "../secret.md").is_err());
        assert!(page(&lib, "/etc/passwd").is_err());
        assert!(index(&lib).unwrap().contains("docs/main/bank.md"));
        let _ = fs::remove_dir_all(&lib);
    }
}
