//! The AI layer. A model suggests and explains; it never decides. See `matchmaking-core/src/ai.rs` for what a model
//! may be shown and how its answers are checked, and `settings.rs` for the local-versus-cloud gate.
pub mod backend;
pub mod commands;
pub mod repo;
pub mod service;
pub mod settings;

#[cfg(test)]
mod tests;
