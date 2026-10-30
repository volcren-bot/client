//! `http_request`: HTTP for scripts, done by the loader.
//!
//! A script can't open a socket. `http_request` writes a `REQUEST_PREFIX` line (like `bank_pin_enter` does for the PIN) and
//! waits for the loader's one line answer. The loader checks the URL against the whitelist the user keeps in its settings,
//! follows redirects only to whitelisted hosts, and makes the request, so what a script may reach is decided outside the
//! sandbox.

use std::{
    collections::BTreeMap,
    sync::{Mutex, PoisonError},
};

use serde::{Deserialize, Serialize};

/// The loader's second capability line, after `form::PINS_PREFIX`: it answers `REQUEST_PREFIX` lines.
pub const CAPABILITY_PREFIX: &str = "\u{1e}HTTP";
/// A line the injector writes for `http_request`: a JSON `Request`. The loader answers on the script's stdin with `reply_line`.
pub const REQUEST_PREFIX: &str = "\u{1e}HTTPREQ ";

/// A request waits this long when the script gives no timeout.
pub const DEFAULT_TIMEOUT_MS: u64 = 15_000;
/// The longest a script may make a request wait.
pub const MAX_TIMEOUT_MS: u64 = 60_000;
/// The biggest request body, in bytes.
pub const MAX_REQUEST_BODY: usize = 1 << 20;
/// The biggest response body the loader reads, in bytes; a bigger one is an error.
pub const MAX_RESPONSE_BODY: usize = 8 << 20;

fn default_method() -> String {
    "GET".into()
}

fn default_timeout() -> u64 {
    DEFAULT_TIMEOUT_MS
}

/// What a script passes to `http_request`, and what goes to the loader.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Request {
    pub url: String,
    #[serde(default = "default_method")]
    pub method: String,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    #[serde(default)]
    pub body: Option<String>,
    /// Milliseconds the whole request may take, redirects included.
    #[serde(default = "default_timeout", rename = "timeout")]
    pub timeout_ms: u64,
}

impl Request {
    /// Checks what the injector can without the whitelist, so a bad call fails in the script and not at the loader.
    pub fn validate(&self) -> Result<(), String> {
        if self.url.trim().is_empty() {
            return Err("http_request needs a url".into());
        }
        if self.method.is_empty() || !self.method.bytes().all(|b| b.is_ascii_alphabetic()) {
            return Err(format!("http_request: \"{}\" is not an HTTP method", self.method));
        }
        if self.body.as_ref().is_some_and(|body| body.len() > MAX_REQUEST_BODY) {
            return Err(format!("http_request: the body is over {} bytes", MAX_REQUEST_BODY));
        }
        if self.timeout_ms == 0 || self.timeout_ms > MAX_TIMEOUT_MS {
            return Err(format!("http_request: the timeout is 1 to {MAX_TIMEOUT_MS} milliseconds"));
        }
        Ok(())
    }
}

/// The server's answer. Header names are lowercase; a repeated header is joined with ", ". The body is text (invalid UTF-8
/// becomes U+FFFD).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Response {
    pub status: u16,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    #[serde(default)]
    pub body: String,
}

pub fn request_line(request: &Request) -> String {
    format!("{REQUEST_PREFIX}{}", serde_json::to_string(request).unwrap_or_default())
}

/// The request of a `REQUEST_PREFIX` line, without the prefix.
pub fn parse_request(payload: &str) -> Result<Request, String> {
    serde_json::from_str(payload).map_err(|e| format!("bad HTTP request from the script: {e}"))
}

/// The loader's answer to a request: the response, or why there is none (not whitelisted, no connection, ...).
pub fn reply_line(answer: &Result<Response, String>) -> String {
    match answer {
        Ok(response) => serde_json::json!({ "response": response }),
        Err(error) => serde_json::json!({ "error": error }),
    }
    .to_string()
}

pub fn parse_reply(line: &str) -> Result<Response, String> {
    let reply: serde_json::Value = serde_json::from_str(line.trim()).map_err(|e| format!("bad HTTP reply from the loader: {e}"))?;
    if let Some(error) = reply.get("error").and_then(serde_json::Value::as_str) {
        return Err(error.to_owned());
    }
    let response = reply.get("response").cloned().ok_or("the loader's HTTP reply has no response")?;
    serde_json::from_value(response).map_err(|e| format!("bad HTTP reply from the loader: {e}"))
}

/// Where `http_request` sends requests. Without one every call fails.
pub type Handler = Box<dyn FnMut(&Request) -> Result<Response, String> + Send>;

static HANDLER: Mutex<Option<Handler>> = Mutex::new(None);

pub fn set_handler(handler: Handler) {
    *HANDLER.lock().unwrap_or_else(PoisonError::into_inner) = Some(handler);
}

pub fn request(request: &Request) -> Result<Response, String> {
    request.validate()?;
    match HANDLER.lock().unwrap_or_else(PoisonError::into_inner).as_mut() {
        Some(handler) => handler(request),
        None => Err("HTTP requests aren't available: this loader can't make them for scripts (update it)".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn get(url: &str) -> Request {
        parse_request(&format!("{{\"url\":\"{url}\"}}")).unwrap()
    }

    #[test]
    fn a_request_defaults_to_a_get_with_a_timeout() {
        let request = get("https://volcren.com/x");
        assert_eq!((request.method.as_str(), request.timeout_ms, request.body.as_deref()), ("GET", DEFAULT_TIMEOUT_MS, None));
        assert_eq!(parse_request(&request_line(&request)[REQUEST_PREFIX.len()..]), Ok(request));
    }

    #[test]
    fn bad_requests_are_refused_before_the_loader_sees_them() {
        assert!(get("").validate().is_err());
        for method in ["", "GE T", "G3T"] {
            let request = Request { method: method.into(), ..get("https://volcren.com") };
            assert!(request.validate().is_err(), "{method:?}");
        }
        let long = Request { body: Some("x".repeat(MAX_REQUEST_BODY + 1)), ..get("https://volcren.com") };
        assert!(long.validate().is_err());
        for timeout_ms in [0, MAX_TIMEOUT_MS + 1] {
            assert!(Request { timeout_ms, ..get("https://volcren.com") }.validate().is_err());
        }
        assert_eq!(get("https://volcren.com").validate(), Ok(()));
    }

    #[test]
    fn replies_round_trip() {
        let response = Response { status: 200, headers: BTreeMap::from([("content-type".to_owned(), "text/plain".to_owned())]), body: "hi\nthere".into() };
        for answer in [Ok(response), Err("not allowed".to_owned())] {
            assert_eq!(parse_reply(&reply_line(&answer)), answer);
        }
        assert!(parse_reply("nonsense").is_err());
        assert!(parse_reply("{}").is_err());
    }

    #[test]
    fn a_reply_is_one_line() {
        let answer = Ok(Response { status: 200, headers: BTreeMap::new(), body: "a\nb\r\nc\u{2028}".into() });
        assert!(!reply_line(&answer).contains('\n'));
    }
}
