//! Update check: `latest.json` in the public `client` bucket, written by the release workflow.

use serde::Deserialize;

use crate::api::Api;

pub const VERSION: &str = env!("VOLCREN_VERSION");

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct Release {
    pub version: String,
    #[serde(default)]
    pub files: Files,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
pub struct Files {
    pub windows: Option<File>,
    pub linux: Option<File>,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct File {
    /// Path inside the `client` bucket.
    pub path: String,
    pub sha256: String,
}

pub fn bucket_url(api: &Api, path: &str) -> String {
    format!(
        "{}/storage/v1/object/public/client/{path}",
        api.config().supabase_url
    )
}

/// `2026.10.26-abc1234`: a date and a commit. Anything else, such as `dev`, doesn't parse.
fn parse_version(text: &str) -> Option<([u32; 3], &str)> {
    let (date, commit) = text.trim().split_once('-')?;
    let mut parts = date.split('.');
    let date = [
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
    ];
    (parts.next().is_none() && !commit.is_empty()).then_some((date, commit))
}

/// Commits can't be ordered, so a later date is newer and on the same day any other commit is: the manifest is by definition the latest.
pub fn is_newer(latest: &str, current: &str) -> bool {
    match (parse_version(latest), parse_version(current)) {
        (Some((latest_date, latest_commit)), Some((date, commit))) => {
            latest_date > date || (latest_date == date && latest_commit != commit)
        }
        _ => false,
    }
}

impl Release {
    /// The file for this platform.
    pub fn file(&self) -> Option<&File> {
        if cfg!(windows) {
            self.files.windows.as_ref()
        } else {
            self.files.linux.as_ref()
        }
    }

    pub fn download_url(&self, api: &Api) -> Option<String> {
        self.file().map(|file| bucket_url(api, &file.path))
    }
}

/// The newest release if it is newer than this build, `None` if this build is current.
pub fn check(api: &Api) -> Result<Option<Release>, String> {
    if api.config().supabase_url.is_empty() {
        return Ok(None);
    }
    let mut response = api
        .agent()
        .get(&bucket_url(api, "latest.json"))
        .call()
        .map_err(|e| e.to_string())?;
    if !response.status().is_success() {
        return Err(format!("the server answered {}", response.status()));
    }
    let release: Release = response
        .body_mut()
        .read_json()
        .map_err(|e| format!("unexpected update manifest: {e}"))?;
    Ok(is_newer(&release.version, VERSION).then_some(release))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_compare_by_date_then_commit() {
        assert!(is_newer("2026.10.27-bbbbbbb", "2026.10.26-aaaaaaa"));
        assert!(is_newer("2026.11.01-bbbbbbb", "2026.10.31-aaaaaaa"));
        assert!(is_newer("2027.01.01-bbbbbbb", "2026.12.31-aaaaaaa"));
        assert!(
            is_newer("2026.10.26-bbbbbbb", "2026.10.26-aaaaaaa"),
            "same day, another commit"
        );
        assert!(!is_newer("2026.10.26-aaaaaaa", "2026.10.26-aaaaaaa"));
        assert!(!is_newer("2026.10.25-bbbbbbb", "2026.10.26-aaaaaaa"));
    }

    #[test]
    fn unparseable_versions_never_prompt_an_update() {
        assert!(!is_newer("0.2.0", "2026.10.26-aaaaaaa"));
        assert!(!is_newer("2026.10-aaaaaaa", "2026.10.26-aaaaaaa"));
        assert!(!is_newer("2026.10.26-", "2026.10.25-aaaaaaa"));
        assert!(!is_newer("2026.10.27-bbbbbbb", "dev"));
    }

    #[test]
    fn parses_the_manifest_the_release_workflow_writes() {
        let release: Release = serde_json::from_str(
            r#"{"version":"2026.10.26-abc1234","api_version":1,"files":{"windows":{"name":"a.zip","path":"2026.10.26-abc1234/a.zip","sha256":"ab"},"linux":{"name":"a.tar.gz","path":"2026.10.26-abc1234/a.tar.gz","sha256":"cd"}}}"#,
        )
        .unwrap();
        assert_eq!(release.version, "2026.10.26-abc1234");
        assert_eq!(
            release.files.linux.unwrap().path,
            "2026.10.26-abc1234/a.tar.gz"
        );
    }
}
