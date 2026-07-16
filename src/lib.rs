//! Core library for the tidalbar terminal application.
//!
//! Service access, playback policy, and rendering state are kept behind small
//! interfaces so the TUI can be tested without network access or an mpv process.

pub mod app;
pub mod artwork;
pub mod auth;
pub mod config;
pub mod models;
pub mod playback;
pub mod tidal;
pub mod ui;
