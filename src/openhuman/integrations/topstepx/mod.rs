//! TopStepX (ProjectX Gateway) broker integration.
//!
//! Exposes: auth, orders, cancel, flatten, websocket.
//! All HTTP calls go to the ProjectX Gateway REST API.
//! `isAutomated: true` is set on every order per CME Group rules.

pub mod auth;
pub mod cancel;
pub mod flatten;
pub mod orders;
pub mod websocket;

pub use auth::TopStepClient;
pub use orders::{BracketOrder, BrokerResponse};
