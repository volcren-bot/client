use std::{
    panic,
    sync::{Arc, atomic::AtomicBool},
    thread,
};

use crate::api::{
    Api, ApiError, Me, StoreScript,
    auth::{self, Pkce, TokenError},
};

pub struct Account {
    pub me: Me,
    pub scripts: Result<Vec<StoreScript>, ApiError>,
}

pub enum Auth {
    SignedOut,
    Restoring,
    SigningIn(Arc<AtomicBool>),
    SignedIn(Me),
}

impl Auth {
    pub fn me(&self) -> Option<&Me> {
        match self {
            Self::SignedIn(me) => Some(me),
            _ => None,
        }
    }

    pub fn me_mut(&mut self) -> Option<&mut Me> {
        match self {
            Self::SignedIn(me) => Some(me),
            _ => None,
        }
    }
}

pub fn load(api: &Api) -> Result<Account, ApiError> {
    thread::scope(|scope| {
        let scripts = scope.spawn(|| api.scripts("", "", false));
        let me = api.me();
        let scripts = scripts
            .join()
            .unwrap_or_else(|payload| panic::resume_unwind(payload));
        me.map(|me| Account { me, scripts })
    })
}

pub fn sign_in(api: &Api, cancel: &AtomicBool) -> Result<Account, String> {
    let missing = api.config().missing();
    if !missing.is_empty() {
        return Err(format!(
            "This build isn't configured for sign-in: {} not set.",
            missing.join(", ")
        ));
    }
    let pkce = Pkce::generate();
    let listener = auth::bind()?;
    let url = auth::authorize_url(api.config(), &pkce);
    webbrowser::open(&url).map_err(|e| {
        format!("Couldn't open the browser ({e}). Open this address yourself: {url}")
    })?;
    let code = auth::wait_for_callback(&listener, auth::LOGIN_TIMEOUT, cancel)?;
    drop(listener);
    let tokens =
        auth::exchange_code(api.agent(), api.config(), &code, &pkce).map_err(|e| match e {
            TokenError::Rejected(why) => format!("Sign-in was refused: {why}"),
            TokenError::Other(error) => error.to_string(),
        })?;
    api.set_tokens(tokens);
    load(api).map_err(|e| e.to_string())
}

pub fn restore(api: &Api) -> Result<Option<Account>, ApiError> {
    if !api.config().missing().is_empty() || !api.restore()? {
        return Ok(None);
    }
    load(api).map(Some)
}
