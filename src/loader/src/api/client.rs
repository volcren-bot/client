use std::{
    sync::{Arc, Mutex, MutexGuard},
    time::Duration,
};

use serde::{Serialize, de::DeserializeOwned};

use super::{
    auth::{self, Session, TokenError, TokenStore},
    config::Config,
    error::{ApiError, ErrorBody},
    models::{
        Ended, Heartbeat, Me, Quote, RecentSession, RecentSessions, Redeemed, Removed, Report,
        Reported, Scored, ScriptList, ScriptManifest, StartedSession, Stats, StoreScript,
    },
};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);

#[derive(Clone)]
pub struct Api {
    config: Arc<Config>,
    agent: ureq::Agent,
    session: Arc<Mutex<Option<Session>>>,
}

fn persist(session: &Session) {
    if let Err(error) = TokenStore::save(&session.refresh_token) {
        eprintln!("Couldn't save the refresh token to the system keyring: {error}");
    }
}

fn path_id<'a>(id: &'a str, kind: &str) -> Result<&'a str, ApiError> {
    if !id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        Ok(id)
    } else {
        Err(ApiError::NotFound(format!("That {kind} id can't be used.")))
    }
}

impl Api {
    pub fn new(config: Config) -> Self {
        let agent = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .timeout_global(Some(REQUEST_TIMEOUT))
            .user_agent(crate::paths::USER_AGENT)
            .build()
            .into();
        Self {
            config: Arc::new(config),
            agent,
            session: Arc::new(Mutex::new(None)),
        }
    }

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

    fn lock(&self) -> Result<MutexGuard<'_, Option<Session>>, ApiError> {
        self.session
            .lock()
            .map_err(|_| ApiError::Server("the session lock is poisoned".into()))
    }

    pub fn set_tokens(&self, tokens: auth::Tokens) {
        let session = Session::from_tokens(tokens);
        persist(&session);
        if let Ok(mut slot) = self.session.lock() {
            *slot = Some(session);
        }
    }

    pub fn restore(&self) -> Result<bool, ApiError> {
        let Some(token) = TokenStore::load() else {
            return Ok(false);
        };
        let mut slot = self.lock()?;
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

    fn refresh(&self, stale: Option<&str>) -> Result<(), ApiError> {
        let mut slot = self.lock()?;
        let Some(current) = slot.as_ref() else {
            return Err(ApiError::Unauthorized);
        };
        if stale.is_some_and(|stale| current.access_token != stale) {
            return Ok(());
        }
        match auth::refresh(&self.agent, &self.config, &current.refresh_token) {
            Ok(tokens) => {
                let session = Session::from_tokens(tokens);
                persist(&session);
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
        let slot = self.lock()?;
        let session = slot.as_ref().ok_or(ApiError::Unauthorized)?;
        if !session.needs_refresh() {
            return Ok(session.access_token.clone());
        }
        drop(slot);
        self.refresh(None)?;
        let slot = self.lock()?;
        Ok(slot
            .as_ref()
            .ok_or(ApiError::Unauthorized)?
            .access_token
            .clone())
    }

    fn call<T: DeserializeOwned>(
        &self,
        method: &str,
        path: &str,
        query: &[(&str, &str)],
        body: Option<&impl Serialize>,
    ) -> Result<T, ApiError> {
        let token = self.access_token()?;
        match self.send(method, path, query, body, &token) {
            Err(ApiError::Unauthorized) => {
                self.refresh(Some(&token))?;
                self.send(method, path, query, body, &self.access_token()?)
            }
            result => result,
        }
    }

    fn get<T: DeserializeOwned>(&self, path: &str, query: &[(&str, &str)]) -> Result<T, ApiError> {
        self.call("GET", path, query, None::<&()>)
    }

    fn post<T: DeserializeOwned>(&self, path: &str, body: &impl Serialize) -> Result<T, ApiError> {
        self.call("POST", path, &[], Some(body))
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
        self.get::<Me>("/launcher/me", &[])
    }

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
        self.get::<ScriptList>("/launcher/scripts", &query)
            .map(|list| list.scripts)
    }

    pub fn redeem(&self, code: &str) -> Result<Redeemed, ApiError> {
        self.post(
            "/launcher/scripts/redeem",
            &serde_json::json!({ "code": code }),
        )
    }

    pub fn remove_grant(&self, script_id: &str) -> Result<Removed, ApiError> {
        self.call(
            "DELETE",
            &format!("/launcher/scripts/{}/grant", path_id(script_id, "script")?),
            &[],
            None::<&()>,
        )
    }

    pub fn quote(&self, script_id: &str) -> Result<Quote, ApiError> {
        self.get(
            &format!("/launcher/scripts/{}/quote", path_id(script_id, "script")?),
            &[],
        )
    }

    pub fn manifest(&self, script_id: &str) -> Result<ScriptManifest, ApiError> {
        self.get(
            &format!("/launcher/scripts/{}/manifest", path_id(script_id, "script")?),
            &[],
        )
    }

    pub fn start_session(&self, script_id: &str) -> Result<StartedSession, ApiError> {
        self.post(
            "/launcher/sessions",
            &serde_json::json!({ "script_id": script_id }),
        )
    }

    pub fn heartbeat(
        &self,
        session_id: &str,
        stats: Option<&Stats>,
    ) -> Result<Heartbeat, ApiError> {
        let path = format!(
            "/launcher/sessions/{}/heartbeat",
            path_id(session_id, "session")?
        );
        match stats {
            Some(stats) => self.post(&path, &serde_json::json!({ "stats": stats })),
            None => self.post(&path, &serde_json::json!({})),
        }
    }

    pub fn end_session(&self, session_id: &str) -> Result<Ended, ApiError> {
        self.post(
            &format!("/launcher/sessions/{}/end", path_id(session_id, "session")?),
            &serde_json::json!({}),
        )
    }

    pub fn recent_sessions(&self) -> Result<Vec<RecentSession>, ApiError> {
        self.get::<RecentSessions>("/launcher/sessions", &[])
            .map(|list| list.sessions)
    }

    pub fn score_session(&self, session_id: &str, score: u8) -> Result<Scored, ApiError> {
        self.post(
            &format!(
                "/launcher/sessions/{}/score",
                path_id(session_id, "session")?
            ),
            &serde_json::json!({ "score": score }),
        )
    }

    pub fn report_session(&self, session_id: &str, report: &Report) -> Result<Reported, ApiError> {
        self.post(
            &format!(
                "/launcher/sessions/{}/report",
                path_id(session_id, "session")?
            ),
            report,
        )
    }
}
