pub mod db;
pub mod licensing;
pub mod module;
pub mod models;

#[cfg(feature = "scanner-module")]
pub mod vision;

#[cfg(feature = "http-server")]
pub mod api;

#[cfg(feature = "http-server")]
mod server;

#[cfg(feature = "http-server")]
pub use server::run_server;
