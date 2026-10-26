use std::{
    io::{BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::{config::Config, error::ApiError};

/// The loopback address GoTrue is allowed to redirect to (`ADDITIONAL_REDIRECT_URLS`).
pub const LOOPBACK_PORT: u16 = 5217;
pub const LOGIN_TIMEOUT: Duration = Duration::from_secs(5 * 60);

const KEYRING_SERVICE: &str = "volcren-loader";
const KEYRING_USER: &str = "refresh-token";

pub struct Pkce {
    pub verifier: String,
    pub challenge: String,
}

impl Pkce {
    pub fn generate() -> Self {
        let mut bytes = [0u8; 32];
        rand::fill(&mut bytes);
        Self::from_verifier(URL_SAFE_NO_PAD.encode(bytes))
    }

    pub fn from_verifier(verifier: String) -> Self {
        let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
        Self { verifier, challenge }
    }
}

/// The tokens GoTrue hands out. Refresh tokens rotate, so every response carries the one to keep.
#[derive(Debug, Clone, Deserialize)]
pub struct Tokens {
    pub access_token: String,
    pub refresh_token: String,
    /// Seconds until the access token expires.
    pub expires_in: u64,
}

#[derive(Debug, Clone)]
pub struct Session {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_at: Instant,
}

impl Session {
    pub fn from_tokens(tokens: Tokens) -> Self {
        Self {
            access_token: tokens.access_token,
            refresh_token: tokens.refresh_token,
            expires_at: Instant::now() + Duration::from_secs(tokens.expires_in),
        }
    }

    /// True shortly before the access token expires, so a request doesn't race the expiry.
    pub fn needs_refresh(&self) -> bool {
        self.expires_at.saturating_duration_since(Instant::now()) < Duration::from_secs(60)
    }
}

pub fn authorize_url(config: &Config, pkce: &Pkce) -> String {
    format!(
        "{}?provider=discord&redirect_to={}&code_challenge={}&code_challenge_method=s256",
        config.auth("/authorize"),
        urlencode(&redirect_uri()),
        pkce.challenge
    )
}

pub fn redirect_uri() -> String {
    format!("http://localhost:{LOOPBACK_PORT}/")
}

fn urlencode(text: &str) -> String {
    text.bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (byte as char).to_string(),
            _ => format!("%{byte:02X}"),
        })
        .collect()
}

/// Binds the loopback listener. Do this before opening the browser, so a busy port is reported first.
pub fn bind() -> Result<TcpListener, String> {
    TcpListener::bind(("127.0.0.1", LOOPBACK_PORT)).map_err(|e| {
        format!("Port {LOOPBACK_PORT} is in use ({e}). Close whatever is using it, or a previous sign-in, and try again.")
    })
}

#[derive(Debug, PartialEq, Eq)]
pub enum Callback {
    Code(String),
    Denied(String),
}

/// Reads the request target of the first line of an HTTP request and pulls `code` (or `error_description`) out of its query.
pub fn parse_callback(request_line: &str) -> Option<Callback> {
    let target = request_line.split_whitespace().nth(1)?;
    let query = target.split_once('?')?.1;
    let query = query.split('#').next().unwrap_or(query);
    let mut error = None;
    for pair in query.split('&') {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        match key {
            "code" if !value.is_empty() => return Some(Callback::Code(urldecode(value))),
            "error_description" => error = Some(urldecode(value)),
            "error" if error.is_none() => error = Some(urldecode(value)),
            _ => {}
        }
    }
    error.map(Callback::Denied)
}

fn urldecode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => out.push(b' '),
            b'%' if bytes.get(i + 1..i + 3).is_some_and(|hex| hex.iter().all(u8::is_ascii_hexdigit)) => {
                out.push(u8::from_str_radix(&text[i + 1..i + 3], 16).unwrap_or(b'?'));
                i += 2;
            }
            byte => out.push(byte),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

const CLOSE_PAGE: &str = "<!doctype html><meta charset=utf-8><title>Volcren</title>\
<body style=\"font-family:sans-serif;background:#111;color:#eee;display:grid;place-items:center;height:100vh;margin:0\">\
<p>You can close this tab and go back to the loader.</p></body>";

fn respond(mut stream: TcpStream, status: &str, body: &str) {
    let _ = write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
}

/// Waits for the browser to come back to the loopback address, until `timeout` or `cancel`.
pub fn wait_for_callback(listener: &TcpListener, timeout: Duration, cancel: &AtomicBool) -> Result<String, String> {
    listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    let started = Instant::now();
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err("Sign-in cancelled.".into());
        }
        if started.elapsed() > timeout {
            return Err("Sign-in timed out. Try again.".into());
        }
        match listener.accept() {
            Ok((stream, _)) => {
                stream.set_nonblocking(false).ok();
                stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
                let mut line = String::new();
                if BufReader::new((&stream).take(8192)).read_line(&mut line).is_err() {
                    continue;
                }
                match parse_callback(&line) {
                    Some(Callback::Code(code)) => {
                        respond(stream, "200 OK", CLOSE_PAGE);
                        return Ok(code);
                    }
                    Some(Callback::Denied(reason)) => {
                        respond(stream, "200 OK", CLOSE_PAGE);
                        return Err(format!("Sign-in was refused: {reason}"));
                    }
                    // Favicon and other stray requests.
                    None => respond(stream, "404 Not Found", ""),
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => std::thread::sleep(Duration::from_millis(100)),
            Err(e) => return Err(format!("The sign-in listener failed: {e}")),
        }
    }
}

#[derive(Serialize)]
struct PkceGrant<'a> {
    auth_code: &'a str,
    code_verifier: &'a str,
}

#[derive(Serialize)]
struct RefreshGrant<'a> {
    refresh_token: &'a str,
}

/// How a token request failed: the refresh token is dead (sign out and say why), or something transient.
#[derive(Debug, PartialEq, Eq)]
pub enum TokenError {
    Rejected(String),
    Other(ApiError),
}

fn token_request<T: Serialize>(agent: &ureq::Agent, config: &Config, grant: &str, body: &T) -> Result<Tokens, TokenError> {
    let mut response = agent
        .post(&config.auth("/token"))
        .query("grant_type", grant)
        .header("apikey", &config.anon_key)
        .send_json(body)
        .map_err(|e| TokenError::Other(ApiError::Network(e.to_string())))?;
    let status = response.status().as_u16();
    if (200..300).contains(&status) {
        return response
            .body_mut()
            .read_json::<Tokens>()
            .map_err(|e| TokenError::Other(ApiError::Network(format!("unexpected sign-in answer: {e}"))));
    }
    let detail: serde_json::Value = response.body_mut().read_json().unwrap_or_default();
    let message = ["error_description", "msg", "message", "error"]
        .iter()
        .find_map(|key| detail.get(key).and_then(serde_json::Value::as_str))
        .unwrap_or("the sign-in was refused")
        .to_owned();
    if (400..500).contains(&status) {
        Err(TokenError::Rejected(message))
    } else {
        Err(TokenError::Other(ApiError::Server(message)))
    }
}

pub fn exchange_code(agent: &ureq::Agent, config: &Config, code: &str, pkce: &Pkce) -> Result<Tokens, TokenError> {
    token_request(agent, config, "pkce", &PkceGrant { auth_code: code, code_verifier: &pkce.verifier })
}

pub fn refresh(agent: &ureq::Agent, config: &Config, refresh_token: &str) -> Result<Tokens, TokenError> {
    token_request(agent, config, "refresh_token", &RefreshGrant { refresh_token })
}

/// The refresh token lives in the OS keyring (Credential Manager / Secret Service), never in eframe's settings storage.
pub struct TokenStore;

impl TokenStore {
    fn entry() -> Result<keyring::Entry, String> {
        keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER).map_err(|e| e.to_string())
    }

    pub fn load() -> Option<String> {
        Self::entry().ok()?.get_password().ok().filter(|token| !token.is_empty())
    }

    pub fn save(refresh_token: &str) -> Result<(), String> {
        Self::entry()?.set_password(refresh_token).map_err(|e| e.to_string())
    }

    pub fn clear() {
        if let Ok(entry) = Self::entry() {
            let _ = entry.delete_credential();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn challenge_matches_the_rfc_7636_example() {
        let pkce = Pkce::from_verifier("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk".into());
        assert_eq!(pkce.challenge, "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM");
    }

    #[test]
    fn generated_verifiers_are_valid_and_distinct() {
        let (a, b) = (Pkce::generate(), Pkce::generate());
        assert_ne!(a.verifier, b.verifier);
        assert!((43..=128).contains(&a.verifier.len()));
        assert!(a.verifier.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_'));
    }

    #[test]
    fn callback_code_and_errors_are_parsed() {
        assert_eq!(parse_callback("GET /?code=abc%2Fdef HTTP/1.1"), Some(Callback::Code("abc/def".into())));
        assert_eq!(
            parse_callback("GET /?error=access_denied&error_description=User+said+no HTTP/1.1"),
            Some(Callback::Denied("User said no".into()))
        );
        assert_eq!(parse_callback("GET /favicon.ico HTTP/1.1"), None);
        assert_eq!(parse_callback(""), None);
    }

    #[test]
    fn authorize_url_carries_the_challenge_and_the_loopback_redirect() {
        let config = Config { api_url: String::new(), supabase_url: "http://sb".into(), anon_key: String::new(), site_url: String::new() };
        let url = authorize_url(&config, &Pkce::from_verifier("v".into()));
        assert!(url.starts_with("http://sb/auth/v1/authorize?provider=discord&redirect_to=http%3A%2F%2Flocalhost%3A5217%2F&code_challenge="));
        assert!(url.ends_with("&code_challenge_method=s256"));
    }

    #[test]
    fn callback_listener_answers_the_browser_and_returns_the_code() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let client = std::thread::spawn(move || {
            let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
            stream.write_all(b"GET /?code=xyz HTTP/1.1\r\nHost: localhost\r\n\r\n").unwrap();
            let mut reply = String::new();
            stream.read_to_string(&mut reply).unwrap();
            reply
        });
        let code = wait_for_callback(&listener, Duration::from_secs(5), &AtomicBool::new(false)).unwrap();
        assert_eq!(code, "xyz");
        assert!(client.join().unwrap().contains("You can close this tab"));
    }

    #[test]
    fn waiting_can_be_cancelled_and_times_out() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        assert_eq!(wait_for_callback(&listener, Duration::from_secs(5), &AtomicBool::new(true)), Err("Sign-in cancelled.".into()));
        assert!(wait_for_callback(&listener, Duration::from_millis(150), &AtomicBool::new(false)).unwrap_err().contains("timed out"));
    }
}
