/// Where the loader talks to: compiled in from `src/loader/config.json`, overridable at run time by the `API_URL`, `SUPABASE_URL`, `SUPABASE_ANON_KEY` and `SITE_URL` environment variables.
#[derive(Clone, Debug, Default, serde::Deserialize)]
#[serde(default)]
pub struct Config {
    /// The webapp, which serves `/api/launcher/*`.
    pub api_url: String,
    /// GoTrue, for sign-in and token refresh.
    pub supabase_url: String,
    /// The public anon key.
    pub anon_key: String,
    /// The website, for "Buy credits". Optional: the button is hidden without it.
    pub site_url: String,
}

impl Config {
    pub fn from_env() -> Self {
        let compiled: Self = serde_json::from_str(include_str!("../../config.json")).expect("config.json is valid");
        let var = |name: &str, compiled: String| {
            std::env::var(name).ok().unwrap_or(compiled).trim().trim_end_matches('/').to_owned()
        };
        Self {
            api_url: var("API_URL", compiled.api_url),
            supabase_url: var("SUPABASE_URL", compiled.supabase_url),
            anon_key: var("SUPABASE_ANON_KEY", compiled.anon_key),
            site_url: var("SITE_URL", compiled.site_url),
        }
    }

    /// Names of the settings that are empty, so the UI can say what is missing instead of failing on a blank URL.
    pub fn missing(&self) -> Vec<&'static str> {
        [("API_URL", &self.api_url), ("SUPABASE_URL", &self.supabase_url), ("SUPABASE_ANON_KEY", &self.anon_key)]
            .into_iter()
            .filter(|(_, value)| value.is_empty())
            .map(|(name, _)| name)
            .collect()
    }

    pub fn api(&self, path: &str) -> String {
        format!("{}{path}", self.api_url)
    }

    pub fn auth(&self, path: &str) -> String {
        format!("{}/auth/v1{path}", self.supabase_url)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_compiled_config_is_complete() {
        let config: Config = serde_json::from_str(include_str!("../../config.json")).unwrap();
        assert!(config.missing().is_empty());
    }

    #[test]
    fn urls_join_without_double_slashes() {
        let config = Config {
            api_url: "http://localhost:5173".into(),
            supabase_url: "http://localhost:54321".into(),
            anon_key: String::new(),
            site_url: String::new(),
        };
        assert_eq!(config.api("/api/launcher/me"), "http://localhost:5173/api/launcher/me");
        assert_eq!(config.auth("/token"), "http://localhost:54321/auth/v1/token");
        assert_eq!(config.missing(), ["SUPABASE_ANON_KEY"]);
    }
}
