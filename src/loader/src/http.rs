use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};

use injector::http::{MAX_RESPONSE_BODY, Request, Response};
use ureq::http::{self, Uri};

use crate::whitelist::Whitelist;

const MAX_REDIRECTS: usize = 5;
const FORBIDDEN_HEADERS: [&str; 7] = [
    "host",
    "content-length",
    "transfer-encoding",
    "connection",
    "upgrade",
    "te",
    "trailer",
];
const PRIVATE_HEADERS: [&str; 3] = ["authorization", "cookie", "proxy-authorization"];

fn agent(timeout: Duration) -> ureq::Agent {
    user_agent_builder(timeout)
        .http_status_as_error(false)
        .max_redirects(0)
        .build()
        .into()
}

pub fn user_agent_builder(
    timeout: Duration,
) -> ureq::config::ConfigBuilder<ureq::typestate::AgentScope> {
    ureq::Agent::config_builder()
        .timeout_global(Some(timeout))
        .user_agent(crate::paths::USER_AGENT)
}

pub fn host_of(uri: &Uri) -> Result<String, String> {
    if !matches!(uri.scheme_str(), Some("http" | "https")) {
        return Err(format!("{uri} isn't an http or https URL"));
    }
    let host = uri.host().ok_or_else(|| format!("{uri} has no host"))?;
    Ok(host.trim_end_matches('.').to_ascii_lowercase())
}

fn check(allowed: &Whitelist, uri: &Uri) -> Result<(), String> {
    let host = host_of(uri)?;
    if allowed.allows(&host) {
        Ok(())
    } else {
        Err(format!(
            "{host} isn't on the HTTP whitelist; add it under Settings in the loader"
        ))
    }
}

fn resolve(from: &Uri, location: &str) -> Result<Uri, String> {
    let bad = |why: &dyn std::fmt::Display| format!("bad redirect to \"{location}\": {why}");
    let target: Uri = location.parse().map_err(|e| bad(&e))?;
    if target.scheme().is_some() {
        return Ok(target);
    }
    let (scheme, authority) = (
        from.scheme_str().unwrap_or("https"),
        from.authority().ok_or_else(|| bad(&"no host"))?,
    );
    let path = if location.starts_with('/') {
        location.to_owned()
    } else {
        let base = from.path();
        format!("{}{location}", &base[..=base.rfind('/').unwrap_or(0)])
    };
    format!("{scheme}://{authority}{path}")
        .parse()
        .map_err(|e| bad(&e))
}

fn run(
    agent: &ureq::Agent,
    request: http::Request<()>,
    body: Option<&str>,
) -> Result<http::Response<ureq::Body>, String> {
    match body {
        Some(body) => agent.run(request.map(|()| body.as_bytes().to_vec())),
        None => agent.run(request),
    }
    .map_err(|e| e.to_string())
}

pub fn perform(allowed: &Whitelist, request: &Request) -> Result<Response, String> {
    request.validate()?;
    let started = Instant::now();
    let total = Duration::from_millis(request.timeout_ms);
    let mut uri: Uri = request
        .url
        .trim()
        .parse()
        .map_err(|e| format!("\"{}\" isn't a URL: {e}", request.url))?;
    let first_host = host_of(&uri)?;
    let first_https = uri.scheme_str() == Some("https");
    let mut method = request.method.to_ascii_uppercase();
    let mut body = request.body.as_deref();
    let mut headers = Vec::new();
    for (name, value) in &request.headers {
        let lower = name.to_ascii_lowercase();
        if FORBIDDEN_HEADERS.contains(&lower.as_str())
            || lower.starts_with("proxy-") && lower != "proxy-authorization"
        {
            return Err(format!("http_request: the {name} header can't be set"));
        }
        headers.push((name.as_str(), value.as_str()));
    }

    for _ in 0..=MAX_REDIRECTS {
        check(allowed, &uri)?;
        let left = total
            .checked_sub(started.elapsed())
            .filter(|left| !left.is_zero())
            .ok_or("the request timed out")?;
        let same_host =
            host_of(&uri)? == first_host && (!first_https || uri.scheme_str() == Some("https"));
        let mut builder = http::Request::builder()
            .method(method.as_str())
            .uri(uri.clone());
        for &(name, value) in &headers {
            if same_host || !PRIVATE_HEADERS.contains(&name.to_ascii_lowercase().as_str()) {
                builder = builder.header(name, value);
            }
        }
        let built = builder.body(()).map_err(|e| format!("http_request: {e}"))?;
        let mut response = run(&agent(left), built, body)?;
        let status = response.status().as_u16();
        let location = response
            .headers()
            .get(http::header::LOCATION)
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned);
        if let Some(location) = location.filter(|_| matches!(status, 301..=303 | 307 | 308)) {
            uri = resolve(&uri, &location)?;
            if matches!(status, 301..=303) && method != "GET" && method != "HEAD" {
                method = "GET".into();
                body = None;
            }
            continue;
        }

        let mut out = BTreeMap::new();
        for (name, value) in response.headers() {
            let value = String::from_utf8_lossy(value.as_bytes());
            out.entry(name.as_str().to_owned())
                .and_modify(|have: &mut String| {
                    have.push_str(", ");
                    have.push_str(&value);
                })
                .or_insert_with(|| value.into_owned());
        }
        let bytes = response
            .body_mut()
            .with_config()
            .limit(MAX_RESPONSE_BODY as u64)
            .read_to_vec()
            .map_err(|e| format!("couldn't read the response: {e}"))?;
        return Ok(Response {
            status,
            headers: out,
            body: String::from_utf8_lossy(&bytes).into_owned(),
        });
    }
    Err(format!("more than {MAX_REDIRECTS} redirects"))
}
