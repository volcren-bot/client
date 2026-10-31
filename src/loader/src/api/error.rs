use std::fmt;

/// What a launcher API call can fail with. The UI maps these to messages; `Unauthorized` is the only one that means "refresh the token and retry".
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApiError {
    Unauthorized,
    /// The refresh token was rejected (banned, deleted, revoked): the stored session is gone and the user must sign in again.
    SignedOut(String),
    Forbidden(String),
    InsufficientCredits {
        required: Option<i64>,
    },
    Conflict(String),
    NotFound(String),
    /// The server can't be reached, or answered something that isn't the contract.
    Network(String),
    Server(String),
}

impl fmt::Display for ApiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unauthorized => write!(f, "You are signed out. Sign in again."),
            Self::SignedOut(why) => write!(f, "Signed out: {why}"),
            Self::Forbidden(message) => write!(f, "{message}"),
            Self::InsufficientCredits {
                required: Some(required),
            } => write!(f, "Not enough credits: {required} needed to start."),
            Self::InsufficientCredits { required: None } => write!(f, "Not enough credits."),
            Self::Conflict(message) | Self::NotFound(message) | Self::Server(message) => {
                write!(f, "{message}")
            }
            Self::Network(message) => write!(f, "Can't reach Volcren: {message}"),
        }
    }
}

impl std::error::Error for ApiError {}

/// The body of every launcher API error: `{ error }`, plus `required` on a 402.
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
            409 => Self::Conflict(message),
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
            ApiError::Conflict("no live revision".into())
        );
        assert_eq!(
            ApiError::from_status(500, body("{}")),
            ApiError::Server("The server answered 500.".into())
        );
    }
}
