use std::{
    collections::BTreeMap,
    sync::{Mutex, PoisonError},
};

use serde::{Deserialize, Serialize};

pub const CAPABILITY_PREFIX: &str = "\u{1e}HTTP";
pub const REQUEST_PREFIX: &str = "\u{1e}HTTPREQ ";

pub const DEFAULT_TIMEOUT_MS: u64 = 15_000;
pub const MAX_TIMEOUT_MS: u64 = 60_000;
pub const MAX_REQUEST_BODY: usize = 1 << 20;
pub const MAX_RESPONSE_BODY: usize = 8 << 20;

fn default_method() -> String {
    "GET".into()
}

fn default_timeout() -> u64 {
    DEFAULT_TIMEOUT_MS
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Request {
    pub url: String,
    #[serde(default = "default_method")]
    pub method: String,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default = "default_timeout", rename = "timeout")]
    pub timeout_ms: u64,
}

impl Request {
    pub fn validate(&self) -> Result<(), String> {
        if self.url.trim().is_empty() {
            return Err("http_request needs a url".into());
        }
        if self.method.is_empty() || !self.method.bytes().all(|b| b.is_ascii_alphabetic()) {
            return Err(format!(
                "http_request: \"{}\" is not an HTTP method",
                self.method
            ));
        }
        if self
            .body
            .as_ref()
            .is_some_and(|body| body.len() > MAX_REQUEST_BODY)
        {
            return Err(format!(
                "http_request: the body is over {MAX_REQUEST_BODY} bytes"
            ));
        }
        if self.timeout_ms == 0 || self.timeout_ms > MAX_TIMEOUT_MS {
            return Err(format!(
                "http_request: the timeout is 1 to {MAX_TIMEOUT_MS} milliseconds"
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Response {
    pub status: u16,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    #[serde(default)]
    pub body: String,
}

pub fn request_line(request: &Request) -> String {
    format!(
        "{REQUEST_PREFIX}{}",
        serde_json::to_string(request).unwrap_or_default()
    )
}

pub fn parse_request(payload: &str) -> Result<Request, String> {
    serde_json::from_str(payload).map_err(|e| format!("bad HTTP request from the script: {e}"))
}

pub fn reply_line(answer: &Result<Response, String>) -> String {
    match answer {
        Ok(response) => serde_json::json!({ "response": response }),
        Err(error) => serde_json::json!({ "error": error }),
    }
    .to_string()
}

pub fn parse_reply(line: &str) -> Result<Response, String> {
    let reply: serde_json::Value = serde_json::from_str(line.trim())
        .map_err(|e| format!("bad HTTP reply from the loader: {e}"))?;
    if let Some(error) = reply.get("error").and_then(serde_json::Value::as_str) {
        return Err(error.to_owned());
    }
    let response = reply
        .get("response")
        .cloned()
        .ok_or("the loader's HTTP reply has no response")?;
    serde_json::from_value(response).map_err(|e| format!("bad HTTP reply from the loader: {e}"))
}

pub type Handler = Box<dyn FnMut(&Request) -> Result<Response, String> + Send>;

static HANDLER: Mutex<Option<Handler>> = Mutex::new(None);

pub fn set_handler(handler: Handler) {
    *HANDLER.lock().unwrap_or_else(PoisonError::into_inner) = Some(handler);
}

pub fn request(request: &Request) -> Result<Response, String> {
    request.validate()?;
    match HANDLER
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .as_mut()
    {
        Some(handler) => handler(request),
        None => Err(
            "HTTP requests aren't available: this loader can't make them for scripts (update it)"
                .into(),
        ),
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
        assert_eq!(
            (
                request.method.as_str(),
                request.timeout_ms,
                request.body.as_deref()
            ),
            ("GET", DEFAULT_TIMEOUT_MS, None)
        );
        assert_eq!(
            parse_request(&request_line(&request)[REQUEST_PREFIX.len()..]),
            Ok(request)
        );
    }

    #[test]
    fn bad_requests_are_refused_before_the_loader_sees_them() {
        assert!(get("").validate().is_err());
        for method in ["", "GE T", "G3T"] {
            let request = Request {
                method: method.into(),
                ..get("https://volcren.com")
            };
            assert!(request.validate().is_err(), "{method:?}");
        }
        let long = Request {
            body: Some("x".repeat(MAX_REQUEST_BODY + 1)),
            ..get("https://volcren.com")
        };
        assert!(long.validate().is_err());
        for timeout_ms in [0, MAX_TIMEOUT_MS + 1] {
            assert!(
                Request {
                    timeout_ms,
                    ..get("https://volcren.com")
                }
                .validate()
                .is_err()
            );
        }
        assert_eq!(get("https://volcren.com").validate(), Ok(()));
    }

    #[test]
    fn replies_round_trip() {
        let response = Response {
            status: 200,
            headers: BTreeMap::from([("content-type".to_owned(), "text/plain".to_owned())]),
            body: "hi\nthere".into(),
        };
        for answer in [Ok(response), Err("not allowed".to_owned())] {
            assert_eq!(parse_reply(&reply_line(&answer)), answer);
        }
        assert!(parse_reply("nonsense").is_err());
        assert!(parse_reply("{}").is_err());
    }

    #[test]
    fn a_reply_is_one_line() {
        let answer = Ok(Response {
            status: 200,
            headers: BTreeMap::new(),
            body: "a\nb\r\nc\u{2028}".into(),
        });
        assert!(!reply_line(&answer).contains('\n'));
    }
}
