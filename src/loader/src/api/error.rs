use std::fmt;

use super::models::credits_label;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApiError {
    Unauthorized,
    SignedOut(String),
    Forbidden(String),
    InsufficientCredits { required: Option<i64> },
    NotFound(String),
    Network(String),
    Server(String),
}

impl fmt::Display for ApiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unauthorized => write!(f, "You are signed out. Sign in again."),
            Self::SignedOut(why) => write!(f, "Signed out: {why}"),
            Self::InsufficientCredits {
                required: Some(required),
            } => write!(
                f,
                "Not enough CREDIT: {} needed to start.",
                credits_label(*required as f64)
            ),
            Self::InsufficientCredits { required: None } => write!(f, "Not enough CREDIT."),
            Self::Forbidden(message) | Self::NotFound(message) | Self::Server(message) => {
                write!(f, "{message}")
            }
            Self::Network(message) => write!(f, "Can't reach Volcren: {message}"),
        }
    }
}

impl std::error::Error for ApiError {}

#[derive(serde::Deserialize, Default)]
pub struct ErrorBody {
    #[serde(default)]
    pub error: String,
    #[serde(default)]
    pub required: Option<i64>,
}

impl ApiError {
    pub fn from_status(status: u16, body: ErrorBody) -> Self {
        let message = if body.error.is_empty() {
            format!("The server answered {status}.")
        } else {
            body.error
        };
        match status {
            401 => Self::Unauthorized,
            402 => Self::InsufficientCredits {
                required: body.required,
            },
            403 => Self::Forbidden(message),
            404 => Self::NotFound(message),
            _ => Self::Server(message),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body(json: &str) -> ErrorBody {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn statuses_map_to_variants() {
        assert_eq!(
            ApiError::from_status(401, body("{}")),
            ApiError::Unauthorized
        );
        assert_eq!(
            ApiError::from_status(402, body(r#"{"error":"no","required":300}"#)),
            ApiError::InsufficientCredits {
                required: Some(300)
            }
        );
        assert_eq!(
            ApiError::from_status(403, body(r#"{"error":"banned"}"#)),
            ApiError::Forbidden("banned".into())
        );
        assert_eq!(
            ApiError::from_status(409, body(r#"{"error":"no live revision"}"#)),
            ApiError::Server("no live revision".into())
        );
        assert_eq!(
            ApiError::from_status(500, body("{}")),
            ApiError::Server("The server answered 500.".into())
        );
    }
}
