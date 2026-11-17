//! Capricorn's platform. One crate, two processes: the API (`serve`) and the
//! worker (`worker`). ARCHITECTURE.md at the repository root has the design.

pub mod auth;
pub mod config;
pub mod error;
pub mod families;
pub mod http;
pub mod jobs;
pub mod learning;
pub mod livekit;
pub mod llm;
pub mod memory;
pub mod sessions;
pub mod state;
pub mod supervisor;

pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");
