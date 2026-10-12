//! Matchwise application layer: persistence and Tauri commands around `matchmaking-core`.
pub mod commands;
pub mod export_commands;
pub mod export_repo;
pub mod match_commands;
pub mod match_repo;
pub mod repository;
pub mod rules_commands;

#[cfg(test)]
mod export_tests;
