use std::path::{Path, PathBuf};

use loader::backend::Settings;

const AGENTS: &str = "# VOLCREN workspace

This folder is for writing and running VOLCREN scripts with an AI agent. The `volcren` MCP server in `.mcp.json` is the loader itself (`loader mcp`): it runs Luau on your game clients and searches the script library's docs. Its instructions explain how to use it.

- Scripts are strict Luau and use the library through `require(\"@lib/...\")`. `.luaurc` points `@lib` at the library the loader uses.
- `runs/<pid>.luau` holds the last snippet the agent ran on each client.
- Every run acts on a real account. Ask before anything that spends coins or risks items.
- Runs started by the agent stop when the agent exits.
";

pub fn dir() -> Result<PathBuf, String> {
    crate::paths::project_dirs()
        .map(|dirs| dirs.data_dir().join("workspace"))
        .ok_or_else(|| "Can't find a data folder on this system.".into())
}

pub fn lib_dir(settings: &Settings) -> Result<PathBuf, String> {
    let custom = settings.lib_dir.trim();
    if !custom.is_empty() {
        return Ok(PathBuf::from(custom));
    }
    crate::session::cache_root()
        .map(|root| root.join("lib"))
        .ok_or_else(|| "Can't find a cache folder on this system.".into())
}

pub fn prepare(settings: &Settings) -> Result<PathBuf, String> {
    let dir = dir()?;
    let exe =
        std::env::current_exe().map_err(|e| format!("Can't find the loader's own path: {e}"))?;
    let mcp = serde_json::json!({
        "mcpServers": { "volcren": { "command": exe, "args": ["mcp"] } }
    });
    let luaurc = serde_json::json!({ "aliases": { "lib": lib_dir(settings)? } });
    write(&dir.join(".mcp.json"), &pretty(&mcp))?;
    write(&dir.join(".luaurc"), &pretty(&luaurc))?;
    write(&dir.join("AGENTS.md"), AGENTS)?;
    write(&dir.join("CLAUDE.md"), "@AGENTS.md\n")?;
    Ok(dir)
}

fn pretty(value: &serde_json::Value) -> String {
    format!(
        "{}\n",
        serde_json::to_string_pretty(value).expect("json serializes")
    )
}

fn write(path: &Path, text: &str) -> Result<(), String> {
    if std::fs::read_to_string(path).is_ok_and(|old| old == text) {
        return Ok(());
    }
    crate::paths::write_atomic(path, text.as_bytes())
        .map_err(|e| format!("Couldn't write {}: {e}", path.display()))
}
