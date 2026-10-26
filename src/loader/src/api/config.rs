/// Where the loader talks to. `API_URL`, `SUPABASE_URL` and `SUPABASE_ANON_KEY` are embedded at build time from `client/.env`
/// (see `build.rs`) and can be overridden at run time with the same variable names, for example to point a release build at `supabase start` + `bun dev`.
#[derive(Clone, Debug)]
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
        let var = |name: &str, embedded: Option<&str>| {
            std::env::var(name)
                .ok()
                .or_else(|| embedded.map(str::to_owned))
                .map(|value| value.trim().trim_end_matches('/').to_owned())
                .unwrap_or_default()
        };
        Self {
            api_url: var("API_URL", option_env!("VOLCREN_API_URL")),
            supabase_url: var("SUPABASE_URL", option_env!("VOLCREN_SUPABASE_URL")),
            anon_key: var("SUPABASE_ANON_KEY", option_env!("VOLCREN_SUPABASE_ANON_KEY")),
            site_url: var("SITE_URL", option_env!("VOLCREN_SITE_URL")),
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
