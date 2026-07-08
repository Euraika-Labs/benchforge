use serde_json::Value;

pub fn target_is_known_zero_cost_when_unpriced(
    kind: &str,
    adapter_id: &str,
    config: &Value,
) -> bool {
    if kind == "mock" {
        return true;
    }
    if !matches!(kind, "direct_model" | "harnessed_model") {
        return false;
    }
    if config
        .get("source")
        .and_then(|value| value.as_str())
        .is_some_and(|source| source == "huggingface-local")
    {
        return true;
    }
    if config_base_url_is_remote(config) {
        return false;
    }
    if config_base_url_is_local(config) {
        return !cloud_model_adapter_id(adapter_id);
    }
    local_model_adapter_id(adapter_id)
}

fn local_model_adapter_id(adapter_id: &str) -> bool {
    matches!(
        adapter_id,
        "generic-openai-compatible"
            | "llama-cpp-openai"
            | "lm-studio-openai"
            | "mlx-lm"
            | "ollama-openai"
            | "omlx-experimental"
            | "openai-compatible"
            | "vllm-openai"
    )
}

fn cloud_model_adapter_id(adapter_id: &str) -> bool {
    matches!(
        adapter_id,
        "anthropic" | "azure-openai" | "gemini" | "mistral" | "openai" | "openrouter"
    )
}

fn config_base_url_is_local(config: &Value) -> bool {
    config
        .get("base_url")
        .and_then(|value| value.as_str())
        .is_some_and(is_local_base_url)
}

fn config_base_url_is_remote(config: &Value) -> bool {
    config
        .get("base_url")
        .and_then(|value| value.as_str())
        .is_some_and(|base_url| base_url.starts_with("http") && !is_local_base_url(base_url))
}

fn is_local_base_url(base_url: &str) -> bool {
    let lower = base_url.to_ascii_lowercase();
    lower.contains("://localhost") || lower.contains("://127.0.0.1") || lower.contains("://0.0.0.0")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_and_mock_targets_are_known_zero_cost_when_unpriced() {
        assert!(target_is_known_zero_cost_when_unpriced(
            "mock",
            "mock",
            &serde_json::json!({})
        ));
        assert!(target_is_known_zero_cost_when_unpriced(
            "direct_model",
            "openai-compatible",
            &serde_json::json!({"base_url": "http://127.0.0.1:8080/v1"})
        ));
        assert!(target_is_known_zero_cost_when_unpriced(
            "direct_model",
            "llama-cpp-openai",
            &serde_json::json!({})
        ));
        assert!(target_is_known_zero_cost_when_unpriced(
            "direct_model",
            "openai-compatible",
            &serde_json::json!({"source": "huggingface-local"})
        ));
    }

    #[test]
    fn remote_and_cloud_targets_are_not_assumed_free() {
        assert!(!target_is_known_zero_cost_when_unpriced(
            "direct_model",
            "openai-compatible",
            &serde_json::json!({"base_url": "https://example.com/v1"})
        ));
        assert!(!target_is_known_zero_cost_when_unpriced(
            "direct_model",
            "openrouter",
            &serde_json::json!({"base_url": "http://127.0.0.1:8080/v1"})
        ));
        assert!(!target_is_known_zero_cost_when_unpriced(
            "direct_model",
            "gemini",
            &serde_json::json!({})
        ));
        assert!(!target_is_known_zero_cost_when_unpriced(
            "benchmark_harness",
            "benchforge-worker",
            &serde_json::json!({})
        ));
    }
}
