//! Matchwise application layer: persistence and Tauri commands around `matchmaking-core`.
pub mod commands;
pub mod export_commands;
pub mod export_repo;
pub mod exposure_commands;
pub mod exposure_repo;
pub mod intake_repo;
pub mod match_commands;
pub mod match_repo;
pub mod ranking_commands;
pub mod ranking_repo;
pub mod repository;
pub mod rules_commands;
pub mod scoring_config_commands;
pub mod scoring_config_repo;

#[cfg(test)]
mod export_tests;
#[cfg(test)]
mod exposure_tests;
#[cfg(test)]
mod intake_tests;
#[cfg(test)]
mod ranking_tests;
