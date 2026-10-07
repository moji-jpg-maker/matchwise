//! Telegram adapter. The matching engine does not depend on this module: Telegram is one of several possible
//! channels, reached only through [`api::TelegramApi`].
//!
//! Privacy boundary: profile data stays in the local encrypted database, but everything sent to or received from
//! Telegram passes through Telegram's servers. The bot therefore never displays stored sensitive fields, shares
//! contact details, or reveals scores, rules or other people's data beyond a matchmaker-approved introduction card.
pub mod api;
pub mod commands;
pub mod engine;
pub mod notify;
pub mod render;
pub mod repo;
pub mod types;
pub mod worker;

#[cfg(test)]
mod tests;
