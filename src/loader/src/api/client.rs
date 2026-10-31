use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use serde::{Serialize, de::DeserializeOwned};

use super::{
    auth::{self, Session, TokenError, TokenStore},
    config::Config,
    error::{ApiError, ErrorBody},
    models::{
        Ended, Heartbeat, Me, Redeemed, Removed, ScriptList, StartedSession, Stats, StoreScript,
    },
};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);

/// A cheap, cloneable handle to the launcher API. Every call blocks, so run them on a background thread.
#[derive(Clone)]
pub struct Api {
    config: Arc<Config>,
    agent: ureq::Agent,
    session: Arc<Mutex<Option<Session>>>,
}

impl Api {
    pub fn new(config: Config) -> Self {
        let agent = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .timeout_global(Some(REQUEST_TIMEOUT))
            .user_agent(concat!("volcren-loader/", env!("VOLCREN_VERSION")))
            .build()
            .into();
        Self {
            config: Arc::new(config),
            agent,
            session: Arc::new(Mutex::new(None)),
        }
    }

    /// An `Api` that is already signed in, without touching the system keyring.
    #[cfg(test)]
    pub fn signed_in(config: Config, access_token: &str) -> Self {
        let api = Self::new(config);
        let session = Session {
            access_token: access_token.into(),
            refresh_token: "refresh".into(),
            expires_at: std::time::Instant::now() + Duration::from_secs(3600),
        };
        *api.session.lock().unwrap() = Some(session);
        api
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    pub fn agent(&self) -> &ureq::Agent {
        &self.agent
    }

    /// Takes the tokens from a finished sign-in (or a refresh) and stores the rotated refresh token.
    pub fn set_tokens(&self, tokens: auth::Tokens) {
        let session = Session::from_tokens(tokens);
        if let Err(error) = TokenStore::save(&session.refresh_token) {
            eprintln!("Couldn't save the refresh token to the system keyring: {error}");
        }
        if let Ok(mut slot) = self.session.lock() {
            *slot = Some(session);
        }
    }

    /// Signs in again from the refresh token in the keyring, if there is one.
    pub fn restore(&self) -> Result<bool, ApiError> {
        let Some(token) = TokenStore::load() else {
            return Ok(false);
        };
        let mut slot = self
            .session
            .lock()
            .map_err(|_| ApiError::Server("the session lock is poisoned".into()))?;
        *slot = Some(Session {
            access_token: String::new(),
            refresh_token: token,
            expires_at: std::time::Instant::now(),
        });
        drop(slot);
        self.refresh(None).map(|()| true)
    }

    pub fn sign_out(&self) {
        TokenStore::clear();
        if let Ok(mut slot) = self.session.lock() {
            *slot = None;
        }
    }

    /// Refreshes the access token. `stale` is the token that just got a 401: if another thread already replaced it, there is nothing to do.
    fn refresh(&self, stale: Option<&str>) -> Result<(), ApiError> {
        let mut slot = self
            .session
            .lock()
            .map_err(|_| ApiError::Server("the session lock is poisoned".into()))?;
        let Some(current) = slot.as_ref() else {
            return Err(ApiError::Unauthorized);
        };
        if stale.is_some_and(|stale| current.access_token != stale) {
            return Ok(());
        }
        match auth::refresh(&self.agent, &self.config, &current.refresh_token) {
            Ok(tokens) => {
                let session = Session::from_tokens(tokens);
                // Refresh token rotation is on: the old one is dead now, so always keep the new one.
                if let Err(error) = TokenStore::save(&session.refresh_token) {
                    eprintln!("Couldn't save the refresh token to the system keyring: {error}");
                }
                *slot = Some(session);
                Ok(())
            }
            Err(TokenError::Rejected(why)) => {
                *slot = None;
                drop(slot);
                TokenStore::clear();
                Err(ApiError::SignedOut(why))
            }
            Err(TokenError::Other(error)) => Err(error),
        }
    }

    fn access_token(&self) -> Result<String, ApiError> {
        let needs_refresh = {
            let slot = self
                .session
                .lock()
                .map_err(|_| ApiError::Server("the session lock is poisoned".into()))?;
            slot.as_ref().ok_or(ApiError::Unauthorized)?.needs_refresh()
        };
        if needs_refresh {
            self.refresh(None)?;
        }
        let slot = self
            .session
            .lock()
            .map_err(|_| ApiError::Server("the session lock is poisoned".into()))?;
        Ok(slot
            .as_ref()
            .ok_or(ApiError::Unauthorized)?
            .access_token
            .clone())
    }

    /// One authenticated call. On a 401 it refreshes once and retries.
    fn call<T: DeserializeOwned>(
        &self,
        method: &str,
        path: &str,
        query: &[(&str, &str)],
        body: Option<&impl Serialize>,
    ) -> Result<T, ApiError> {
        let mut token = self.access_token()?;
        for attempt in 0..2 {
            match self.send(method, path, query, body, &token) {
                Err(ApiError::Unauthorized) if attempt == 0 => {
                    self.refresh(Some(&token))?;
                    token = self.access_token()?;
                }
                result => return result,
            }
        }
        Err(ApiError::Unauthorized)
    }

    fn send<T: DeserializeOwned>(
        &self,
        method: &str,
        path: &str,
        query: &[(&str, &str)],
        body: Option<&impl Serialize>,
        token: &str,
    ) -> Result<T, ApiError> {
        let url = self.config.api(path);
        let authorization = format!("Bearer {token}");
        let result = match method {
            "GET" => {
                let mut request = self.agent.get(&url).header("Authorization", &authorization);
                for (key, value) in query {
                    request = request.query(key, value);
                }
                request.call()
            }
            "DELETE" => self
                .agent
                .delete(&url)
                .header("Authorization", &authorization)
                .header("Origin", &self.config.api_url)
                .call(),
            _ => {
                let request = self
                    .agent
                    .post(&url)
                    .header("Authorization", &authorization)
                    .header("Origin", &self.config.api_url);
                match body {
                    Some(body) => request.send_json(body),
                    None => request.send_empty(),
                }
            }
        };
        let mut response = result.map_err(|e| ApiError::Network(e.to_string()))?;
        let status = response.status().as_u16();
        if (200..300).contains(&status) {
            return response
                .body_mut()
                .read_json()
                .map_err(|e| ApiError::Server(format!("unexpected answer from {path}: {e}")));
        }
        Err(ApiError::from_status(
            status,
            response
                .body_mut()
                .read_json::<ErrorBody>()
                .unwrap_or_default(),
        ))
    }

    pub fn me(&self) -> Result<Me, ApiError> {
        self.call::<Me>("GET", "/launcher/me", &[], None::<&()>)
    }

    /// `community` lists every community script the account can see instead of the default list (testers and above only).
    pub fn scripts(
        &self,
        search: &str,
        category: &str,
        community: bool,
    ) -> Result<Vec<StoreScript>, ApiError> {
        let mut query = Vec::new();
        if !search.is_empty() {
            query.push(("q", search));
        }
        if !category.is_empty() {
            query.push(("category", category));
        }
        if community {
            query.push(("community", "true"));
        }
        self.call::<ScriptList>("GET", "/launcher/scripts", &query, None::<&()>)
            .map(|list| list.scripts)
    }

    /// Redeems a share code (already checked with `normalize_share_code`). The server marks it used and adds the script in one step.
    pub fn redeem(&self, code: &str) -> Result<Redeemed, ApiError> {
        self.call(
            "POST",
            "/launcher/scripts/redeem",
            &[],
            Some(&serde_json::json!({ "code": code })),
        )
    }

    /// Takes a script that came from a share code off the account. Its code stays used.
    pub fn remove_grant(&self, script_id: &str) -> Result<Removed, ApiError> {
        // The id goes into the path, so only a plain id is sent
        if script_id.is_empty()
            || !script_id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-')
        {
            return Err(ApiError::NotFound("That script id can't be used.".into()));
        }
        self.call(
            "DELETE",
            &format!("/launcher/scripts/{script_id}/grant"),
            &[],
            None::<&()>,
        )
    }

    pub fn start_session(&self, script_id: &str) -> Result<StartedSession, ApiError> {
        self.call(
            "POST",
            "/launcher/sessions",
            &[],
            Some(&serde_json::json!({ "script_id": script_id })),
        )
    }

    pub fn heartbeat(
        &self,
        session_id: &str,
        stats: Option<&Stats>,
    ) -> Result<Heartbeat, ApiError> {
        let path = format!("/launcher/sessions/{session_id}/heartbeat");
        match stats {
            Some(stats) => self.call(
                "POST",
                &path,
                &[],
                Some(&serde_json::json!({ "stats": stats })),
            ),
            None => self.call("POST", &path, &[], Some(&serde_json::json!({}))),
        }
    }

    /// Idempotent on the server: ending an ended session is fine.
    pub fn end_session(&self, session_id: &str) -> Result<Ended, ApiError> {
        self.call(
            "POST",
            &format!("/launcher/sessions/{session_id}/end"),
            &[],
            Some(&serde_json::json!({})),
        )
    }
}
