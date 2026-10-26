//! The launcher's connection to Volcren: sign-in (PKCE over a loopback redirect), token refresh, and the `/api/launcher/*` calls.
//! Everything here blocks, so callers run it on a background thread and report back through the app's `Event` channel.

pub mod auth;
pub mod client;
pub mod config;
pub mod error;
pub mod models;

pub use client::Api;
pub use config::Config;
pub use error::ApiError;
pub use models::{Heartbeat, Me, StartedSession, Stats, StoreScript};
