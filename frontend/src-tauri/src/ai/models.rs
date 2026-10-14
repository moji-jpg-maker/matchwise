//! What MatchWise offers a matchmaker to choose from, and how each matchmaking task is run.
//!
//! This is MatchWise's own list, not Meetily's summary list: a model is only offered here if it can be trusted to
//! return the strict JSON the matchmaking prompts demand. It reuses the summary engine only for what is generic
//! (the downloader, the file on disk and the sidecar), through `summary_model`.

use crate::summary::summary_engine::{self, models::SamplingParams, GenerationOverrides};
use serde::Serialize;
use std::path::Path;

/// One model MatchWise lets the matchmaker pick for the "builtin" provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MmModelDef {
    /// What is stored in the AI settings.
    pub id: &'static str,
    pub label: &'static str,
    /// The summary-engine model that supplies the file, download and chat template.
    pub summary_model: &'static str,
    pub note: &'static str,
    pub recommended: bool,
}

const MODELS: &[MmModelDef] = &[
    MmModelDef {
        id: "qwen3.5:4b",
        label: "Qwen 3.5 4B",
        summary_model: "qwen3.5:4b",
        note: "Best quality of the built-in models for reading free text and explaining matches. Needs about 4 GB of memory.",
        recommended: true,
    },
    MmModelDef {
        id: "qwen3.5:2b",
        label: "Qwen 3.5 2B",
        summary_model: "qwen3.5:2b",
        note: "Lighter and faster, with more missed details. For modest computers.",
        recommended: false,
    },
    MmModelDef {
        id: "gemma3:4b",
        label: "Gemma 3 4B",
        summary_model: "gemma3:4b",
        note: "An alternative of similar size. Compare it with Qwen on your own notes.",
        recommended: false,
    },
];

pub fn models() -> &'static [MmModelDef] {
    MODELS
}

pub fn find_model(id: &str) -> Option<&'static MmModelDef> {
    MODELS.iter().find(|m| m.id == id.trim())
}

pub fn default_model() -> &'static MmModelDef {
    MODELS.iter().find(|m| m.recommended).unwrap_or(&MODELS[0])
}

/// Where a built-in model stands on this computer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelFile {
    Downloaded,
    NotDownloaded,
    /// The registry names a summary model the engine does not know (a build mistake, caught by a test).
    Unknown,
}

pub fn model_file_state(app_data_dir: &Path, def: &MmModelDef) -> ModelFile {
    match summary_engine::get_model_by_name(def.summary_model) {
        None => ModelFile::Unknown,
        Some(m) => {
            let path = summary_engine::models::get_models_directory(&app_data_dir.to_path_buf()).join(&m.gguf_file);
            if path.is_file() { ModelFile::Downloaded } else { ModelFile::NotDownloaded }
        }
    }
}

/// The matchmaking jobs a model is asked to do. Each has its own settings: both want answers that repeat and
/// stay inside the given facts, and neither wants a long reply.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskProfile {
    /// Reading free text into suggested profile fields: longer output, strict JSON.
    Extraction,
    /// Explaining one match from the listed facts: shorter output.
    PairAnalysis,
}

impl TaskProfile {
    /// The `kind` strings the service logs calls under. Anything else (a connection test) gets the pair profile.
    pub fn for_kind(kind: &str) -> TaskProfile {
        match kind {
            "extract" | "intake" | "intake_batch" => TaskProfile::Extraction,
            _ => TaskProfile::PairAnalysis,
        }
    }

    pub fn temperature(self) -> f32 {
        match self {
            TaskProfile::Extraction => 0.1,
            TaskProfile::PairAnalysis => 0.2,
        }
    }

    pub fn max_tokens(self) -> i32 {
        match self {
            TaskProfile::Extraction => 3072,
            TaskProfile::PairAnalysis => 1536,
        }
    }

    pub fn timeout_secs(self) -> u64 {
        match self {
            TaskProfile::Extraction => 600,
            TaskProfile::PairAnalysis => 300,
        }
    }

    /// Settings for the sidecar: near-greedy, keeping the model's own stop tokens.
    pub fn overrides_for(self, def: &MmModelDef) -> Option<GenerationOverrides> {
        let model = summary_engine::get_model_by_name(def.summary_model)?;
        let mut sampling = SamplingParams::tight_structured(model.sampling.stop_tokens.clone());
        sampling.temperature = self.temperature();
        Some(GenerationOverrides { sampling: Some(sampling), max_tokens: Some(self.max_tokens()), timeout_secs: Some(self.timeout_secs()) })
    }
}

/// One provider the AI page can offer. The single source of truth for ids, labels and defaults.
#[derive(Debug, Clone, Serialize)]
pub struct ProviderInfo {
    pub id: &'static str,
    pub label: &'static str,
    pub default_url: &'static str,
    /// Runs on this computer by design (the address can still make others local or cloud).
    pub always_local: bool,
    pub needs_key: bool,
    pub model_from_list: bool,
}

pub fn provider_catalog() -> Vec<ProviderInfo> {
    vec![
        ProviderInfo { id: "none", label: "Off", default_url: "", always_local: true, needs_key: false, model_from_list: false },
        ProviderInfo { id: "builtin", label: "Built-in (on this computer)", default_url: "", always_local: true, needs_key: false, model_from_list: true },
        ProviderInfo { id: "ollama", label: "Ollama (local)", default_url: "http://localhost:11434", always_local: false, needs_key: false, model_from_list: false },
        ProviderInfo { id: "openai_compatible", label: "OpenAI-compatible", default_url: "https://api.openai.com/v1", always_local: false, needs_key: false, model_from_list: false },
        ProviderInfo { id: "anthropic", label: "Anthropic", default_url: "https://api.anthropic.com", always_local: false, needs_key: true, model_from_list: false },
    ]
}

/// Meetily spells the built-in provider "builtin-ai"; MatchWise stores "builtin". Accept both, store one.
pub fn normalize_provider(provider: &str) -> String {
    match provider.trim() {
        "builtin-ai" => "builtin".to_string(),
        other => other.to_string(),
    }
}

/// Why the selected AI cannot be used right now, in a form the page can act on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Readiness {
    Ready,
    Disabled,
    ModelMissing,
    NeedsKey,
    NeedsConsent,
    Invalid,
}

/// Everything the readiness verdict needs, passed in so it is testable without a database or a disk.
pub struct ReadinessInput<'a> {
    pub config: &'a super::settings::AiConfig,
    pub consent: Option<&'a super::settings::CloudConsent>,
    pub has_key: bool,
    pub model_file: Option<ModelFile>,
}

pub fn readiness(i: &ReadinessInput<'_>) -> (Readiness, Option<String>) {
    use super::settings::{check_gate, BUILTIN};
    if i.config.provider == "none" {
        return (Readiness::Disabled, Some("AI is turned off. Choose a provider on the AI page.".into()));
    }
    match check_gate(i.config, i.consent, i.has_key) {
        Err(reason) => {
            let kind = if reason.contains("API key") {
                Readiness::NeedsKey
            } else if reason.contains("cloud notice") {
                Readiness::NeedsConsent
            } else {
                Readiness::Invalid
            };
            (kind, Some(reason))
        }
        Ok(_) if i.config.provider == BUILTIN && i.model_file != Some(ModelFile::Downloaded) => {
            (Readiness::ModelMissing, Some(format!("The model '{}' is not downloaded yet. Download it on the AI page.", i.config.model)))
        }
        Ok(_) => (Readiness::Ready, None),
    }
}
