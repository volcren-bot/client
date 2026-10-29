//! The launcher's blocking connection to Volcren: sign-in (PKCE), token refresh and `/api/launcher/*`; run it on a background thread.

pub mod auth;
pub mod client;
pub mod config;
pub mod error;
pub mod models;

pub use client::Api;
pub use config::Config;
pub use error::ApiError;
pub use models::{Heartbeat, Me, Redeemed, Removed, StartedSession, Stats, StoreScript};
