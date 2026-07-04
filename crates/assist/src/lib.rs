//! Command Mode AI backends (PRD 12 "Command Mode").
//!
//! The user highlights text, speaks a question about it, and one of these
//! providers answers. Three wire formats are supported; all requests are
//! blocking (callers run them off the UI/engine threads).
//!
//! Every provider gets a free `web_search` tool (DuckDuckGo HTML — no key,
//! no quota) and may call it a few times before answering. Endpoints or
//! models that reject tool calls fall back to a plain, tool-less request.

mod search;

use std::time::Duration;

use anyhow::{bail, Result};
use serde::Serialize;
use serde_json::{json, Value};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Provider {
    /// Ollama's native chat API (local).
    Ollama,
    /// Anthropic Messages API.
    Anthropic,
    /// Any OpenAI-compatible chat completions endpoint.
    OpenAiCompatible,
}

impl Provider {
    pub fn from_str_lossy(s: &str) -> Self {
        match s.to_ascii_lowercase().as_str() {
            "anthropic" | "claude" => Self::Anthropic,
            "openai" | "openai-compatible" => Self::OpenAiCompatible,
            _ => Self::Ollama,
        }
    }
}

#[derive(Debug, Clone)]
pub struct AssistConfig {
    pub provider: Provider,
    /// Empty string = use the provider's default model.
    pub model: String,
    /// Base URL for Ollama / OpenAI-compatible endpoints.
    pub base_url: String,
    pub api_key: String,
    /// Allow the model to call the free web_search tool.
    pub web_search: bool,
}

impl AssistConfig {
    fn model(&self) -> &str {
        if !self.model.is_empty() {
            return &self.model;
        }
        match self.provider {
            Provider::Ollama => "gemma3:4b",
            Provider::Anthropic => "claude-3-5-sonnet-latest",
            Provider::OpenAiCompatible => "",
        }
    }
}

/// A page the assistant consulted while answering.
#[derive(Debug, Clone, Serialize)]
pub struct Source {
    pub title: String,
    pub url: String,
}

pub struct Answer {
    pub text: String,
    pub sources: Vec<Source>,
}

/// Live status line for the overlay ("Searching the web · rust editions").
pub type Progress<'a> = &'a (dyn Fn(&str) + Sync);

const BASE_PROMPT: &str = "You are FlowOSS Assist. The user highlighted text on their \
screen and asked a spoken question about it (transcribed, so wording may be imperfect). \
Answer the question about the highlighted text directly and concisely - a few sentences \
unless more detail is clearly needed. Plain text only, no markdown headings.";

const SEARCH_PROMPT: &str = "\n\nYou have a web_search tool. Use it when the question \
needs current events, prices, versions, or facts beyond the highlighted text; answer \
directly from the text when it suffices. At most a couple of searches, then answer.";

/// The model may search, read, and search again — but not forever.
const MAX_TOOL_ROUNDS: usize = 4;

fn system_prompt(config: &AssistConfig) -> String {
    if config.web_search {
        format!("{BASE_PROMPT}{SEARCH_PROMPT}")
    } else {
        BASE_PROMPT.to_string()
    }
}

/// Exercise the web_search tool directly (used by `examples/probe.rs`).
pub fn debug_search(query: &str) -> Result<Vec<(String, String, String)>> {
    Ok(search::web_search(query)?
        .into_iter()
        .map(|h| (h.title, h.url, h.snippet))
        .collect())
}

/// Ask the configured provider about `context` (the highlighted text).
pub fn ask(
    config: &AssistConfig,
    context: &str,
    query: &str,
    progress: Progress,
) -> Result<Answer> {
    let user_prompt = format!("Highlighted text:\n---\n{context}\n---\n\nQuestion: {query}");
    match config.provider {
        Provider::Ollama => ask_ollama(config, &user_prompt, progress),
        Provider::Anthropic => ask_anthropic(config, &user_prompt, progress),
        Provider::OpenAiCompatible => ask_openai(config, &user_prompt, progress),
    }
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(120))
        .build()
}

/// Run one search call and append newly-seen pages to `sources`.
fn run_search(query: &str, sources: &mut Vec<Source>, progress: Progress) -> String {
    let query = query.trim();
    if query.is_empty() {
        return "Search failed: empty query.".into();
    }
    progress(&format!("Searching the web · {query}"));
    match search::web_search(query) {
        Ok(hits) if !hits.is_empty() => {
            let mut out = String::new();
            for (i, hit) in hits.iter().enumerate() {
                out.push_str(&format!(
                    "{}. {}\n   {}\n   {}\n",
                    i + 1,
                    hit.title,
                    hit.url,
                    hit.snippet
                ));
                if sources.len() < 8 && !sources.iter().any(|s| s.url == hit.url) {
                    sources.push(Source {
                        title: hit.title.clone(),
                        url: hit.url.clone(),
                    });
                }
            }
            out
        }
        Ok(_) => "No results found.".into(),
        Err(e) => format!("Search failed: {e}"),
    }
}

/// OpenAI-style function schema (Ollama uses the same shape).
fn openai_tools() -> Value {
    json!([{
        "type": "function",
        "function": {
            "name": "web_search",
            "description": "Search the web (DuckDuckGo). Returns the top results as title, URL and snippet.",
            "parameters": {
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "The search query" }
                },
                "required": ["query"]
            }
        }
    }])
}

fn anthropic_tools() -> Value {
    json!([{
        "name": "web_search",
        "description": "Search the web (DuckDuckGo). Returns the top results as title, URL and snippet.",
        "input_schema": {
            "type": "object",
            "properties": {
                "query": { "type": "string", "description": "The search query" }
            },
            "required": ["query"]
        }
    }])
}

enum Reply {
    Json(Value),
    /// 4xx complaining about tools — the model/endpoint can't do tool calls.
    ToolsUnsupported,
}

fn post_json(request: ureq::Request, payload: &Value, who: &str, with_tools: bool) -> Result<Reply> {
    match request.send_json(payload) {
        Ok(response) => Ok(Reply::Json(response.into_json()?)),
        Err(ureq::Error::Status(code, response)) => {
            let body = response.into_string().unwrap_or_default();
            if with_tools && (400..500).contains(&code) && body.to_lowercase().contains("tool") {
                return Ok(Reply::ToolsUnsupported);
            }
            let snippet: String = body.chars().take(300).collect();
            bail!("{who} error {code}: {snippet}")
        }
        Err(ureq::Error::Transport(t)) => bail!("{who} unreachable: {t}"),
    }
}

fn ask_ollama(config: &AssistConfig, prompt: &str, progress: Progress) -> Result<Answer> {
    let base = if config.base_url.is_empty() {
        "http://localhost:11434"
    } else {
        config.base_url.trim_end_matches('/')
    };
    let url = format!("{base}/api/chat");
    let mut messages = vec![
        json!({"role": "system", "content": system_prompt(config)}),
        json!({"role": "user", "content": prompt}),
    ];
    let mut sources = Vec::new();
    let mut use_tools = config.web_search;

    for _ in 0..=MAX_TOOL_ROUNDS {
        let mut payload = json!({
            "model": config.model(),
            "stream": false,
            "messages": messages,
        });
        if use_tools {
            payload["tools"] = openai_tools();
        }
        let response = match post_json(agent().post(&url), &payload, "Ollama", use_tools)? {
            Reply::Json(v) => v,
            Reply::ToolsUnsupported => {
                // e.g. gemma3 — retry without tools, and stop advertising
                // web_search in the system prompt or the model fakes calls.
                use_tools = false;
                messages[0] = json!({"role": "system", "content": BASE_PROMPT});
                continue;
            }
        };
        let message = response["message"].clone();
        let calls = message["tool_calls"].as_array().cloned().unwrap_or_default();
        if calls.is_empty() {
            let text = message["content"]
                .as_str()
                .map(|s| s.trim().to_string())
                .unwrap_or_default();
            if text.is_empty() {
                bail!("Ollama returned no content: {response}");
            }
            return Ok(Answer { text, sources });
        }
        messages.push(message);
        for call in &calls {
            let query = call["function"]["arguments"]["query"].as_str().unwrap_or("");
            let result = run_search(query, &mut sources, progress);
            messages.push(json!({"role": "tool", "content": result}));
        }
        progress("Reading results…");
    }
    bail!("assistant kept searching without answering")
}

fn ask_anthropic(config: &AssistConfig, prompt: &str, progress: Progress) -> Result<Answer> {
    if config.api_key.is_empty() {
        bail!("Anthropic API key is not set (Settings → Assistant)");
    }
    let mut messages = vec![json!({"role": "user", "content": prompt})];
    let mut sources = Vec::new();

    for _ in 0..=MAX_TOOL_ROUNDS {
        let mut payload = json!({
            "model": config.model(),
            "max_tokens": 1024,
            "system": system_prompt(config),
            "messages": messages,
        });
        if config.web_search {
            payload["tools"] = anthropic_tools();
        }
        let response = match post_json(
            agent()
                .post("https://api.anthropic.com/v1/messages")
                .set("x-api-key", &config.api_key)
                .set("anthropic-version", "2023-06-01"),
            &payload,
            "Anthropic",
            config.web_search,
        )? {
            Reply::Json(v) => v,
            Reply::ToolsUnsupported => bail!("Anthropic rejected the web_search tool"),
        };

        if response["stop_reason"] == "tool_use" {
            let blocks = response["content"].as_array().cloned().unwrap_or_default();
            messages.push(json!({"role": "assistant", "content": blocks}));
            let mut results = Vec::new();
            for block in messages.last().unwrap()["content"].as_array().unwrap() {
                if block["type"] == "tool_use" && block["name"] == "web_search" {
                    let query = block["input"]["query"].as_str().unwrap_or("");
                    results.push(json!({
                        "type": "tool_result",
                        "tool_use_id": block["id"],
                        "content": run_search(query, &mut sources, progress),
                    }));
                }
            }
            messages.push(json!({"role": "user", "content": results}));
            progress("Reading results…");
            continue;
        }

        let text: String = response["content"]
            .as_array()
            .map(|blocks| {
                blocks
                    .iter()
                    .filter(|b| b["type"] == "text")
                    .filter_map(|b| b["text"].as_str())
                    .collect::<Vec<_>>()
                    .join("")
            })
            .unwrap_or_default();
        if text.is_empty() {
            bail!("Anthropic returned no text (stop_reason: {})", response["stop_reason"]);
        }
        return Ok(Answer {
            text: text.trim().to_string(),
            sources,
        });
    }
    bail!("assistant kept searching without answering")
}

fn ask_openai(config: &AssistConfig, prompt: &str, progress: Progress) -> Result<Answer> {
    if config.base_url.is_empty() {
        bail!("Endpoint URL is not set (Settings → Assistant)");
    }
    let base = config.base_url.trim_end_matches('/');
    let url = format!("{base}/chat/completions");
    let mut messages = vec![
        json!({"role": "system", "content": system_prompt(config)}),
        json!({"role": "user", "content": prompt}),
    ];
    let mut sources = Vec::new();
    let mut use_tools = config.web_search;

    for _ in 0..=MAX_TOOL_ROUNDS {
        let mut payload = json!({
            "model": config.model(),
            "messages": messages,
        });
        if use_tools {
            payload["tools"] = openai_tools();
        }
        let mut request = agent().post(&url);
        if !config.api_key.is_empty() {
            request = request.set("Authorization", &format!("Bearer {}", config.api_key));
        }
        let response = match post_json(request, &payload, "endpoint", use_tools)? {
            Reply::Json(v) => v,
            Reply::ToolsUnsupported => {
                use_tools = false;
                messages[0] = json!({"role": "system", "content": BASE_PROMPT});
                continue;
            }
        };
        let message = response["choices"][0]["message"].clone();
        let calls = message["tool_calls"].as_array().cloned().unwrap_or_default();
        if calls.is_empty() {
            let text = message["content"]
                .as_str()
                .map(|s| s.trim().to_string())
                .unwrap_or_default();
            if text.is_empty() {
                bail!("endpoint returned no content: {response}");
            }
            return Ok(Answer { text, sources });
        }
        messages.push(message);
        for call in &calls {
            // OpenAI encodes arguments as a JSON string.
            let args: Value = call["function"]["arguments"]
                .as_str()
                .and_then(|s| serde_json::from_str(s).ok())
                .unwrap_or(Value::Null);
            let query = args["query"].as_str().unwrap_or("");
            messages.push(json!({
                "role": "tool",
                "tool_call_id": call["id"],
                "content": run_search(query, &mut sources, progress),
            }));
        }
        progress("Reading results…");
    }
    bail!("assistant kept searching without answering")
}
