//! Signing in and out. The blocking parts run on a background thread (see `app.rs`); the state lives in `Auth`.

use std::sync::{Arc, atomic::AtomicBool};

use crate::api::{
    Api, ApiError, Me,
    auth::{self, Pkce, TokenError},
};

pub enum Auth {
    SignedOut,
    /// Looking for a saved sign-in in the system keyring.
    Restoring,
    /// Waiting for the browser. Set the flag to cancel.
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

/// The whole browser round trip: open the sign-in page, wait for the redirect, trade the code for tokens, load the profile.
pub fn sign_in(api: &Api, cancel: &AtomicBool) -> Result<Me, String> {
    let missing = api.config().missing();
    if !missing.is_empty() {
        return Err(format!(
            "This build isn't configured for sign-in: {} not set.",
            missing.join(", ")
        ));
    }
    let pkce = Pkce::generate();
    // Bind first, so a busy port is reported before a browser tab opens.
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
    api.me().map_err(|e| e.to_string())
}

/// Signs in from the saved refresh token. `Ok(None)` means there was none.
pub fn restore(api: &Api) -> Result<Option<Me>, ApiError> {
    if !api.config().missing().is_empty() || !api.restore()? {
        return Ok(None);
    }
    api.me().map(Some)
}
