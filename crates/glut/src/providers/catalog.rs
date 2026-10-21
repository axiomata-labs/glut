//! The providers Glut knows by name (`docs/plans/glut-providers.md`, P1): where their API is, how a run signs in and
//! where a key may wait in the environment. The owner's own endpoints sit beside them as `openai-compatible`.

/// The wire protocol a provider speaks. PV1 has one; the Anthropic Messages and OpenAI Responses clients follow
/// (P2, PV3/PV4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Protocol {
    /// `POST {root}/chat/completions` — OpenRouter, Ollama and most others.
    ChatCompletions,
}

/// How a run signs in to a provider (P3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignIn {
    /// An API key or a coding plan's key, sent as a bearer token.
    Key,
    /// Nothing: a server on this machine.
    None,
}

/// One provider of the catalog.
#[derive(Debug, Clone, Copy)]
pub struct Known {
    /// The id the owner names it by (`axh providers add openrouter`).
    pub id: &'static str,
    pub name: &'static str,
    pub protocol: Protocol,
    /// The API's root, its version included: requests go to `{root}/chat/completions`.
    pub root: &'static str,
    pub sign_in: SignIn,
    /// The environment variable a key may come from instead of the secret store.
    pub key_env: Option<&'static str>,
    /// One line for `axh providers catalog`.
    pub note: &'static str,
}

/// The kind of an endpoint the owner names by its root, not by the catalog.
pub const CUSTOM: &str = "openai-compatible";

/// Every provider Glut knows. Anthropic and Gemini go through their OpenAI-compatible endpoints until their own
/// clients exist (P2); a coding plan has its own root and a key that is not the pay-as-you-go one.
pub const CATALOG: &[Known] = &[
    key(
        "openrouter",
        "OpenRouter",
        "https://openrouter.ai/api/v1",
        "OPENROUTER_API_KEY",
        "hundreds of models, one key",
    ),
    key(
        "openai",
        "OpenAI",
        "https://api.openai.com/v1",
        "OPENAI_API_KEY",
        "API key (the ChatGPT sign-in comes in PV4)",
    ),
    key(
        "anthropic",
        "Anthropic",
        "https://api.anthropic.com/v1",
        "ANTHROPIC_API_KEY",
        "API key; Claude Pro/Max may not be used here",
    ),
    key(
        "gemini",
        "Google Gemini",
        "https://generativelanguage.googleapis.com/v1beta/openai",
        "GEMINI_API_KEY",
        "Gemini API key",
    ),
    key(
        "mistral",
        "Mistral",
        "https://api.mistral.ai/v1",
        "MISTRAL_API_KEY",
        "API key",
    ),
    key(
        "xai",
        "xAI",
        "https://api.x.ai/v1",
        "XAI_API_KEY",
        "API key",
    ),
    key(
        "deepseek",
        "DeepSeek",
        "https://api.deepseek.com/v1",
        "DEEPSEEK_API_KEY",
        "API key",
    ),
    key(
        "groq",
        "Groq",
        "https://api.groq.com/openai/v1",
        "GROQ_API_KEY",
        "API key",
    ),
    key(
        "together",
        "Together AI",
        "https://api.together.xyz/v1",
        "TOGETHER_API_KEY",
        "API key",
    ),
    key(
        "fireworks",
        "Fireworks AI",
        "https://api.fireworks.ai/inference/v1",
        "FIREWORKS_API_KEY",
        "API key",
    ),
    key(
        "cerebras",
        "Cerebras",
        "https://api.cerebras.ai/v1",
        "CEREBRAS_API_KEY",
        "API key",
    ),
    key(
        "moonshot",
        "Moonshot (Kimi API)",
        "https://api.moonshot.ai/v1",
        "MOONSHOT_API_KEY",
        "pay-as-you-go key",
    ),
    key(
        "kimi-code",
        "Kimi Code",
        "https://api.kimi.com/coding/v1",
        "KIMI_API_KEY",
        "coding plan key (Kimi membership)",
    ),
    key(
        "zai",
        "Z.ai (GLM API)",
        "https://api.z.ai/api/paas/v4",
        "ZAI_API_KEY",
        "pay-as-you-go key",
    ),
    key(
        "zai-coding",
        "Z.ai GLM Coding Plan",
        "https://api.z.ai/api/coding/paas/v4",
        "ZAI_CODING_API_KEY",
        "coding plan key",
    ),
    key(
        "minimax",
        "MiniMax",
        "https://api.minimax.io/v1",
        "MINIMAX_API_KEY",
        "API or token plan key",
    ),
    key(
        "qwen",
        "Alibaba Qwen (DashScope)",
        "https://dashscope-intl.aliyuncs.com/compatible-mode/v1",
        "DASHSCOPE_API_KEY",
        "API key",
    ),
    key(
        "ollama-cloud",
        "Ollama Cloud",
        "https://ollama.com/v1",
        "OLLAMA_API_KEY",
        "Ollama account key",
    ),
    local(
        "ollama",
        "Ollama (this Mac)",
        "http://localhost:11434/v1",
        "the models installed with `ollama pull`",
    ),
    local(
        "lmstudio",
        "LM Studio (this Mac)",
        "http://localhost:1234/v1",
        "LM Studio's local server",
    ),
];

const fn key(
    id: &'static str,
    name: &'static str,
    root: &'static str,
    key_env: &'static str,
    note: &'static str,
) -> Known {
    Known {
        id,
        name,
        protocol: Protocol::ChatCompletions,
        root,
        sign_in: SignIn::Key,
        key_env: Some(key_env),
        note,
    }
}

const fn local(
    id: &'static str,
    name: &'static str,
    root: &'static str,
    note: &'static str,
) -> Known {
    Known {
        id,
        name,
        protocol: Protocol::ChatCompletions,
        root,
        sign_in: SignIn::None,
        key_env: None,
        note,
    }
}

/// The catalog's provider `id`.
pub fn known(id: &str) -> Option<&'static Known> {
    CATALOG.iter().find(|known| known.id == id)
}

/// Every environment variable that holds a provider's key: never a server's to have (an MCP header's `${…}`).
pub fn key_envs() -> impl Iterator<Item = &'static str> {
    CATALOG.iter().filter_map(|known| known.key_env)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_unique_and_roots_are_https_unless_local() {
        for (at, known) in CATALOG.iter().enumerate() {
            assert!(
                CATALOG[at + 1..].iter().all(|other| other.id != known.id),
                "{} twice",
                known.id
            );
            assert!(!known.root.ends_with('/'), "{}", known.id);
            match known.sign_in {
                SignIn::Key => assert!(known.root.starts_with("https://"), "{}", known.id),
                SignIn::None => {
                    assert!(known.root.starts_with("http://localhost:"), "{}", known.id)
                }
            }
        }
        assert!(
            known(CUSTOM).is_none(),
            "the custom kind is no catalog entry"
        );
        // A coding plan's key is not its pay-as-you-go key: one variable for both would send it to the wrong API.
        let names: Vec<_> = key_envs().collect();
        for (at, name) in names.iter().enumerate() {
            assert!(!names[at + 1..].contains(name), "{name} twice");
        }
    }
}
