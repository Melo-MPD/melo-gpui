//! Minimal MPD protocol client, mirroring MeloCore's `MPDClient` design:
//! separate command / idle / cover-art sockets, an idle loop that maps
//! subsystem changes to refreshes, and chunked binary art assembly.

pub mod client;
pub mod connection;
pub mod discovery;
pub mod models;
pub mod parser;
#[allow(dead_code)]
pub mod protocol;

pub use client::{Command, MpdClient};
pub use models::*;
