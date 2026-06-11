use regex::Regex;
use std::sync::OnceLock;

/// True when the model should use the OpenAI Responses API.
///
/// The Responses API is backwards-compatible with all OpenAI reasoning
/// models, so every `o`-series (`o1`, `o3`, `o4`, ...) and `gpt-5` variant
/// routes here. The matcher intentionally scans the full model identifier so
/// hosted aliases like `databricks-gpt-5.4`, `goose-o3-mini`, or
/// `headless-goose-o3-mini` work without provider-specific normalization.
pub fn is_openai_responses_model(model_name: &str) -> bool {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re =
        RE.get_or_init(|| Regex::new(r"(?i)(?:^|[-/])(?:o\d+(?:$|-)|gpt-5(?:$|[-.]))").unwrap());
    re.is_match(model_name)
}

/// Extract an explicit reasoning-effort suffix from a model name.
///
/// Returns `(base_model_name, Some(effort))` when the user appended a
/// recognised suffix like `-high` or `-xhigh`, e.g. `gpt-5.4-high` becomes
/// `("gpt-5.4", Some("high"))`.
///
/// When no suffix is present the effort is `None`; callers should omit the
/// reasoning field so the API applies its own per-model default. This avoids
/// hard-coding a default that may be invalid for certain models.
pub fn extract_reasoning_effort(model_name: &str) -> (String, Option<String>) {
    if !is_openai_responses_model(model_name) {
        return (model_name.to_string(), None);
    }

    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        Regex::new(r"(?i)^(?P<base>.+)-(?P<effort>none|low|medium|high|xhigh)$").unwrap()
    });

    if let Some(captures) = re.captures(model_name) {
        let base = captures["base"].to_string();
        let effort = captures["effort"].to_ascii_lowercase();
        return (base, Some(effort));
    }

    (model_name.to_string(), None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifies_openai_responses_models_and_hosted_aliases() {
        for model in [
            "o3",
            "o3-mini",
            "gpt-5",
            "gpt-5.4",
            "databricks-gpt-5.4-high",
            "goose-o3-mini",
            "headless-goose-o3-mini",
        ] {
            assert!(is_openai_responses_model(model), "{model} should match");
        }

        for model in [
            "gpt-4o",
            "claude-sonnet-4",
            "gemini-2.5-pro",
            "moonshotai/kimi-k2.6",
        ] {
            assert!(
                !is_openai_responses_model(model),
                "{model} should not match"
            );
        }
    }

    #[test]
    fn extracts_reasoning_effort_for_responses_models() {
        for (model, expected_name, expected_effort) in [
            ("o3-none", "o3", Some("none")),
            ("o3-xhigh", "o3", Some("xhigh")),
            ("gpt-5-low", "gpt-5", Some("low")),
            ("gpt-5.4", "gpt-5.4", None),
            (
                "databricks-gpt-5.4-high",
                "databricks-gpt-5.4",
                Some("high"),
            ),
            ("databricks-o3-low", "databricks-o3", Some("low")),
            ("goose-gpt-5-high", "goose-gpt-5", Some("high")),
            ("gpt-4o", "gpt-4o", None),
        ] {
            let (name, effort) = extract_reasoning_effort(model);
            assert_eq!(name, expected_name, "unexpected base model for {model}");
            assert_eq!(
                effort.as_deref(),
                expected_effort,
                "unexpected effort for {model}"
            );
        }
    }
}
