//! Aule's platform. One crate, two processes: the API (`serve`) and the
//! worker (`worker`). ARCHITECTURE.md at the repository root has the design.

pub mod auth;
pub mod config;
pub mod devices;
pub mod error;
pub mod events;
pub mod exercise;
pub mod families;
pub mod flow;
pub mod google;
pub mod http;
pub mod jobs;
pub mod learning;
pub mod livekit;
pub mod llm;
pub mod mail;
pub mod memory;
pub mod metrics;
pub mod notices;
pub mod practice;
pub mod push;
pub mod sessions;
pub mod signin;
pub mod state;
pub mod supervisor;

pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");
