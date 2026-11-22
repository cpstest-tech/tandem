use super::{Improvement, Job, Provider};
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

pub trait Adapter {
    fn complete(
        &self,
        job: &Job,
        prompt: &str,
        schema: &serde_json::Value,
        directory: &Path,
        cancel: &AtomicBool,
    ) -> Result<Reply, String>;

    fn generate(
        &self,
        job: &Job,
        prompt: &str,
        directory: &Path,
        cancel: &AtomicBool,
    ) -> Result<Output, String> {
        let auto = job.request.auto_duration;
        let mut properties =
            serde_json::json!({"source":{"type":"STRING"},"transparent":{"type":"BOOLEAN"}});
        properties["suggestions"] = serde_json::json!({
            "type": "ARRAY",
            "items": {
                "type": "OBJECT",
                "properties": {
                    "id": {"type": "STRING"},
                    "title": {"type": "STRING"},
                    "description": {"type": "STRING"},
                    "instruction": {"type": "STRING"}
                },
                "required": ["id", "title", "description", "instruction"]
            }
        });
        let mut required = vec!["source", "transparent", "suggestions"];
        let ask = job.request.ask_material;
        if ask {
            properties["material"] = super::material::schema();
            required.push("material");
        }
        if auto {
            properties["duration_seconds"] = serde_json::json!({"type":"NUMBER"});
            required.push("duration_seconds");
        }
        let schema =
            serde_json::json!({"type":"OBJECT", "properties":properties, "required":required});
        let prompt = format!(
            "{prompt}\nReturn the required JSON fields source and transparent. The current render transparency is {}. Preserve it unless the user asks for a transparent/opaque background or overlay. The transparent flag controls the actual rendered alpha channel, so update the component background consistently. Also return suggestions: exactly three different, concrete edits the user could ask for next to improve THIS composition, based on what you just wrote. Each has an id (short slug), a title of at most 4 words, a description of one short plain sentence a non-technical person understands (no jargon, no code terms), and an instruction: a precise edit request addressed to you, specific to elements that exist in the source. Write them in the language of the user request. Do not repeat edits listed in the earlier requests.{}{}",
            job.request.composition.transparent,
            if ask {
                super::material::SCHEMA_HINT
            } else {
                ""
            },
            if auto {
                " Also return duration_seconds: the length in seconds that best fits the content, between 1 and the maximum in Configuration. Design the animation to fill exactly that length."
            } else {
                ""
            }
        );
        let reply = self.complete(job, &prompt, &schema, directory, cancel)?;
        let transparent = parse_transparency(&reply.text)?;
        Ok(Output {
            source: parse_source(&reply.text)?,
            transparent,
            duration_seconds: parse_duration(&reply.text),
            improvements: parse_improvements(&reply.text),
            requested: if ask {
                serde_json::from_str(&reply.text)
                    .map(|value| super::material::parse_requested(&value))
                    .unwrap_or_default()
            } else {
                Vec::new()
            },
            usage: reply.usage,
            session: reply.session,
            events: reply.events,
        })
    }
}

#[derive(Debug)]
pub struct Reply {
    pub text: String,
    pub usage: Option<serde_json::Value>,
    pub session: Option<String>,
    pub events: Vec<String>,
}

pub struct Output {
    pub source: String,
    pub transparent: Option<bool>,
    pub duration_seconds: Option<f64>,
    pub improvements: Vec<Improvement>,
    pub requested: Vec<super::material::Slot>,
    pub usage: Option<serde_json::Value>,
    pub session: Option<String>,
    pub events: Vec<String>,
}

struct Gemini;
struct Cli(Provider);

pub fn adapter(provider: Provider) -> Box<dyn Adapter + Send> {
    match provider {
        Provider::Gemini => Box::new(Gemini),
        other => Box::new(Cli(other)),
    }
}

pub fn parse_source(text: &str) -> Result<String, String> {
    let value: serde_json::Value = serde_json::from_str(text)
        .map_err(|e| format!("Provider did not return the required JSON source: {e}"))?;
    let source = value["source"]
        .as_str()
        .ok_or("Provider response is missing source")?;
    if source.len() > 512_000 || !source.contains("export default") {
        return Err(
            "Source must contain a default React component export and be smaller than 512 KB"
                .into(),
        );
    }
    Ok(source.to_owned())
}

impl Adapter for Gemini {
    fn complete(
        &self,
        job: &Job,
        prompt: &str,
        schema: &serde_json::Value,
        directory: &Path,
        cancel: &AtomicBool,
    ) -> Result<Reply, String> {
        let key = crate::gemini::configured_key()?;
        job.request.api_access.check(&key, &job.request.model)?;
        let mut parts = vec![serde_json::json!({"text":prompt})];
        parts.extend(super::context::inline_parts(job, directory)?);
        let request = serde_json::json!({
            "contents": [{"role": "user", "parts": parts}],
            "generationConfig": {"responseMimeType": "application/json", "responseSchema": schema}
        });
        let body = crate::gemini::generate_with_timeout(
            &request,
            &key,
            cancel,
            &[&job.request.model],
            false,
            Duration::from_secs(job.request.limits.provider_timeout_secs),
        )?;
        if body
            .pointer("/candidates/0/finishReason")
            .and_then(|v| v.as_str())
            != Some("STOP")
        {
            return Err("Gemini returned an incomplete or blocked composition; choose another model or shorten the request".into());
        }
        let parts = body
            .pointer("/candidates/0/content/parts")
            .and_then(|v| v.as_array())
            .ok_or("Gemini returned no source")?;
        let text: String = parts
            .iter()
            .filter(|p| p["thought"] != true)
            .filter_map(|p| p["text"].as_str())
            .collect();
        Ok(Reply {
            text,
            usage: body.get("usageMetadata").cloned(),
            session: None,
            events: vec!["generateContent: STOP".into()],
        })
    }
}

impl Adapter for Cli {
    fn complete(
        &self,
        job: &Job,
        prompt: &str,
        schema: &serde_json::Value,
        directory: &Path,
        cancel: &AtomicBool,
    ) -> Result<Reply, String> {
        let mut command =
            super::process::program(self.0.executable().ok_or("Invalid CLI provider")?);
        // The Windows command line is capped at 32K characters, the prompt
        // with the existing source is not: stdin has no limit.
        let prompt_on_stdin = cfg!(windows);
        command.current_dir(directory);
        let schema = json_schema(schema);
        let images = super::context::image_paths(job, directory)?;
        let mut input = None;
        let session = job.session.as_deref().filter(|id| valid_session(id));
        match self.0 {
            Provider::Codex => {
                let path = directory.join("response-schema.json");
                std::fs::write(
                    &path,
                    serde_json::to_vec(&schema).map_err(|e| e.to_string())?,
                )
                .map_err(|e| e.to_string())?;
                command.args(["exec", "--sandbox", "read-only"]);
                if let Some(session) = session {
                    command.args(["fork", session]);
                }
                command.args([
                    "--skip-git-repo-check",
                    "--json",
                    "--ignore-user-config",
                    "--disable",
                    "shell_tool",
                    "--disable",
                    "unified_exec",
                ]);
                command.arg("--output-schema").arg(&path);
                if !job.request.model.is_empty() {
                    command.args(["--model", &job.request.model]);
                }
                for image in &images {
                    command.arg("--image").arg(image);
                }
                if prompt_on_stdin {
                    command.arg("-");
                    input = Some(prompt.as_bytes().to_vec());
                } else {
                    command.arg("--").arg(prompt);
                }
            }
            Provider::Claude => {
                command.args([
                    "-p",
                    "--output-format",
                    if images.is_empty() {
                        "json"
                    } else {
                        "stream-json"
                    },
                    "--tools",
                    "",
                    "--strict-mcp-config",
                    "--mcp-config",
                    "{\"mcpServers\":{}}",
                ]);
                if !job.request.model.is_empty() {
                    command.args(["--model", &job.request.model]);
                }
                command.arg("--json-schema").arg(schema.to_string());
                command.arg("--disable-slash-commands");
                if let Some(session) = session {
                    command.args(["--resume", session, "--fork-session"]);
                }
                if images.is_empty() && prompt_on_stdin {
                    input = Some(prompt.as_bytes().to_vec());
                } else if images.is_empty() {
                    command.arg("--").arg(prompt);
                } else {
                    use base64::Engine;
                    command.args(["--input-format", "stream-json", "--verbose"]);
                    let mut content = vec![serde_json::json!({"type":"text","text":prompt})];
                    for image in &images {
                        let bytes = std::fs::read(image).map_err(|e| e.to_string())?;
                        content.push(serde_json::json!({"type":"image","source":{"type":"base64","media_type":"image/png","data":base64::engine::general_purpose::STANDARD.encode(bytes)}}));
                    }
                    input=Some(format!("{}\n",serde_json::json!({"type":"user","message":{"role":"user","content":content}})).into_bytes());
                }
            }
            Provider::OpenCode => {
                command.env("OPENCODE_CONFIG_CONTENT", r#"{"permission":{"*":"deny"}}"#);
                command.args(["run", "--format", "json"]);
                if !job.request.model.is_empty() {
                    command.args(["--model", &job.request.model]);
                }
                if let Some(session) = session {
                    command.args(["--session", session, "--fork"]);
                }
                for image in &images {
                    command.arg("--file").arg(image);
                }
                command.arg("--").arg(prompt);
            }
            _ => return Err("Invalid CLI provider".into()),
        }
        let events = super::process::run_with_input(
            &mut command,
            cancel,
            Duration::from_secs(job.request.limits.provider_timeout_secs),
            input,
        )?;
        parse_events(&events)
    }
}

pub fn valid_session(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && !id.starts_with('-')
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-_".contains(&byte))
}

pub fn json_schema(schema: &serde_json::Value) -> serde_json::Value {
    let mut value = schema.clone();
    if let serde_json::Value::Object(map) = &mut value {
        if let Some(serde_json::Value::String(kind)) = map.get_mut("type") {
            *kind = kind.to_ascii_lowercase();
        }
        if map.get("type").is_some_and(|kind| kind == "object") {
            map.insert("additionalProperties".into(), false.into());
        }
        for (key, field) in map.iter_mut() {
            if key == "properties" {
                if let Some(properties) = field.as_object_mut() {
                    for property in properties.values_mut() {
                        *property = json_schema(property);
                    }
                }
            } else if key == "items" {
                *field = json_schema(field);
            }
        }
    }
    value
}

pub fn parse_events(events: &str) -> Result<Reply, String> {
    let values = match serde_json::from_str::<serde_json::Value>(events) {
        Ok(value) => vec![value],
        Err(_) => events
            .lines()
            .filter_map(|line| serde_json::from_str(line).ok())
            .collect(),
    };
    let mut reply = Reply {
        text: String::new(),
        usage: None,
        session: None,
        events: Vec::new(),
    };
    for value in values {
        if value["is_error"] == true
            || matches!(value["type"].as_str(), Some("error" | "turn.failed"))
        {
            let message = value
                .pointer("/error/message")
                .and_then(|v| v.as_str())
                .or_else(|| value["message"].as_str())
                .unwrap_or("Check provider authentication, selected model and usage limits");
            return Err(format!(
                "CLI provider request failed: {}",
                message.chars().take(500).collect::<String>()
            ));
        }
        if let Some(event) = value["type"].as_str().filter(|event| {
            event.len() <= 80
                && event
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
        }) && reply.events.len() < 64
        {
            reply.events.push(event.to_owned());
        }
        for field in [
            value.get("thread_id"),
            value.get("session_id"),
            value.get("sessionID"),
            value.pointer("/part/sessionID"),
        ] {
            if let Some(id) = field
                .and_then(|v| v.as_str())
                .filter(|id| valid_session(id))
            {
                reply.session = Some(id.to_owned());
            }
        }
        if let Some(usage) = value.get("usage").or_else(|| value.pointer("/part/tokens")) {
            reply.usage = Some(usage.clone());
        }
        if let Some(output) = value.get("structured_output").filter(|v| v.is_object()) {
            reply.text = output.to_string();
        } else if let Some(result) = value["result"].as_str().filter(|text| !text.is_empty()) {
            reply.text = result.to_owned();
        }
        if value["type"] == "item.completed"
            && value["item"]["type"] == "agent_message"
            && let Some(message) = value["item"]["text"].as_str()
        {
            reply.text = message.to_owned();
        }
        if value["type"] == "text"
            && let Some(message) = value["part"]["text"].as_str()
        {
            reply.text.push_str(message);
        }
    }
    if reply.text.trim().is_empty() {
        return Err(
            "CLI completed without structured output; update the CLI or choose a compatible model"
                .into(),
        );
    }
    Ok(reply)
}

pub fn parse_transparency(text: &str) -> Result<Option<bool>, String> {
    let value: serde_json::Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
    match value.get("transparent") {
        None => Ok(None),
        Some(serde_json::Value::Bool(value)) => Ok(Some(*value)),
        Some(_) => Err("Provider returned an invalid transparency flag".into()),
    }
}

pub fn parse_duration(text: &str) -> Option<f64> {
    let value: serde_json::Value = serde_json::from_str(text).ok()?;
    value
        .get("duration_seconds")?
        .as_f64()
        .filter(|seconds| seconds.is_finite() && *seconds > 0.0)
}

fn text_field<'a>(item: &'a serde_json::Value, name: &str) -> &'a str {
    item.get(name)
        .and_then(|v| v.as_str())
        .map(str::trim)
        .unwrap_or("")
}

fn limited(text: &str, max: usize) -> String {
    text.chars().take(max).collect()
}

/// Lenient on purpose: a missing or malformed list only means no suggestions.
pub fn parse_improvements(text: &str) -> Vec<Improvement> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(text) else {
        return Vec::new();
    };
    let Some(items) = value.get("suggestions").and_then(|v| v.as_array()) else {
        return Vec::new();
    };
    let mut result: Vec<Improvement> = Vec::new();
    for item in items {
        let title = text_field(item, "title");
        let description = text_field(item, "description");
        let instruction = text_field(item, "instruction");
        if title.is_empty() || description.is_empty() || instruction.is_empty() {
            continue;
        }
        if result.iter().any(|known| {
            known.title.eq_ignore_ascii_case(title)
                || known.instruction.eq_ignore_ascii_case(instruction)
        }) {
            continue;
        }
        let mut id: String = text_field(item, "id")
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
            .take(32)
            .collect();
        if id.is_empty() || result.iter().any(|known| known.id == id) {
            id = format!("idea-{}", result.len() + 1);
        }
        result.push(Improvement {
            id,
            title: limited(title, 60),
            description: limited(description, 240),
            instruction: limited(instruction, 1000),
        });
        if result.len() == 3 {
            break;
        }
    }
    result
}
