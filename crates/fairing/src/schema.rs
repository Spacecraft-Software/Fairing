// SPDX-FileCopyrightText: 2026 Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>
// SPDX-License-Identifier: GPL-3.0-or-later
// Rust guideline compliant 2026-05-18

//! `fairing schema [<command>]`: JSON Schema Draft 2020-12 derived from the clap tree.
//!
//! Parameters come straight from the same `clap::Command` that parses argv, so
//! the schema cannot drift from runtime behaviour (CLI Standard
//! `schema-introspection.md` §5). The metadata clap does not model —
//! idempotency, destructiveness, examples, output shape, and the JSON type and
//! bounds of a value parser — lives in [`SPECS`]. The schema document itself
//! is emitted (no envelope) so it drops directly into Anthropic / MCP
//! `input_schema` fields.

use clap::{ArgAction, CommandFactory as _};
use serde_json::{Map, Value, json};

use crate::cli::{Cli, GlobalFlags};
use crate::error::AppError;
use crate::output::mode::Context;
use crate::output::write_line;

/// JSON Schema dialect every emitted document declares.
pub const DIALECT: &str = "https://json-schema.org/draft/2020-12/schema";

/// The JSON type a parameter is reported as.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParamKind {
    /// A finite real number.
    Number,
    /// A whole number.
    Integer,
    /// Text, optionally constrained by a pattern.
    String,
}

impl ParamKind {
    const fn json_type(self) -> &'static str {
        match self {
            Self::Number => "number",
            Self::Integer => "integer",
            Self::String => "string",
        }
    }
}

/// Typing clap does not carry for one parameter: the JSON type a value parser
/// accepts, its bounds and its pattern. Every row names a visible argument of
/// its command (a unit test enforces it).
#[derive(Debug, Clone, Copy)]
pub struct ParamSpec {
    /// The clap argument id.
    pub name: &'static str,
    /// The JSON type.
    pub kind: ParamKind,
    /// Inclusive lower bound, for numeric kinds.
    pub minimum: Option<i64>,
    /// Inclusive upper bound, for numeric kinds.
    pub maximum: Option<i64>,
    /// An ECMA-262 pattern the string must match.
    pub pattern: Option<&'static str>,
}

/// Metadata about one verb that clap does not carry.
#[derive(Debug, Clone, Copy)]
pub struct CommandSpec {
    /// Subcommand name as clap knows it.
    pub name: &'static str,
    /// Same invocation twice, same result.
    pub idempotent: bool,
    /// Changes state outside the process.
    pub destructive: bool,
    /// Honours `--dry-run` meaningfully.
    pub supports_dry_run: bool,
    /// `(command, description)` pairs; at least one shows `--json`.
    pub examples: &'static [(&'static str, &'static str)],
    /// Parameters whose value parser is stricter than "a string".
    pub params: &'static [ParamSpec],
}

/// Every shipped verb. Add a row here when a verb lands, in the same commit as the clap variant.
pub const SPECS: &[CommandSpec] = &[
    CommandSpec {
        name: "describe",
        idempotent: true,
        destructive: false,
        supports_dry_run: false,
        examples: &[
            ("fairing describe", "Print the capability manifest"),
            ("fairing describe --json", "The manifest as a JSON envelope"),
            (
                "fairing describe --json --fields tool,version,commands",
                "Only the named fields",
            ),
        ],
        params: &[],
    },
    CommandSpec {
        name: "schema",
        idempotent: true,
        destructive: false,
        supports_dry_run: false,
        examples: &[
            ("fairing schema", "JSON Schema for the whole tool"),
            (
                "fairing schema describe --json",
                "JSON Schema for the describe command",
            ),
        ],
        params: &[],
    },
    CommandSpec {
        name: "preview",
        idempotent: true,
        destructive: false,
        supports_dry_run: true,
        examples: &[
            (
                "fairing preview --seconds 3",
                "Draw the splash on the current VT for three seconds",
            ),
            (
                "fairing preview --backend memory --snapshot frame.ppm --json",
                "Render off-screen, write a PPM, report as JSON",
            ),
            (
                "fairing preview --dry-run --json",
                "Report what would be drawn without touching a device",
            ),
        ],
        params: &[
            ParamSpec {
                name: "seconds",
                kind: ParamKind::Number,
                minimum: Some(0),
                maximum: Some(3600),
                pattern: None,
            },
            ParamSpec {
                name: "fps",
                kind: ParamKind::Integer,
                minimum: Some(1),
                maximum: Some(240),
                pattern: None,
            },
            ParamSpec {
                name: "size",
                kind: ParamKind::String,
                minimum: None,
                maximum: None,
                // Each side is 1..=16384, exactly as `cli::parse_size` enforces.
                pattern: Some(
                    "^([1-9][0-9]{0,3}|1[0-5][0-9]{3}|16[0-2][0-9]{2}|163[0-7][0-9]|1638[0-4])x([1-9][0-9]{0,3}|1[0-5][0-9]{3}|16[0-2][0-9]{2}|163[0-7][0-9]|1638[0-4])$",
                ),
            },
        ],
    },
];

/// The canonical exit-code map (CLI Standard §4) as a JSON object.
#[must_use]
pub fn exit_codes() -> Value {
    json!({
        "0": "SUCCESS",
        "1": "GENERAL_FAILURE — FEATURE_UNAVAILABLE or INTERNAL_ERROR",
        "2": "USAGE_ERROR — INVALID_ARGUMENT or MISSING_ARGUMENT",
        "3": "NOT_FOUND",
        "4": "PERMISSION_DENIED",
        "5": "CONFLICT",
    })
}

/// Names of the global flags, from the clap tree.
#[must_use]
pub fn global_flags() -> Vec<String> {
    Cli::command()
        .get_arguments()
        .filter(|arg| arg.is_global_set())
        .filter_map(|arg| arg.get_long().map(|long| format!("--{long}")))
        .collect()
}

/// The whole-tool schema document.
#[must_use]
pub fn tool_schema() -> Value {
    let cli = Cli::command();
    let commands: Vec<Value> = SPECS
        .iter()
        .filter_map(|spec| {
            cli.find_subcommand(spec.name)
                .map(|sub| command_schema(sub, spec))
        })
        .collect();
    json!({
        "$schema": DIALECT,
        "tool": env!("CARGO_PKG_NAME"),
        "version": env!("CARGO_PKG_VERSION"),
        "description": cli.get_about().map(ToString::to_string).unwrap_or_default(),
        "global_flags": global_flags(),
        "commands": commands,
        "exit_codes": exit_codes(),
        "error_codes": ["NOT_FOUND", "PERMISSION_DENIED", "CONFLICT", "INVALID_ARGUMENT", "MISSING_ARGUMENT", "FEATURE_UNAVAILABLE", "INTERNAL_ERROR"],
        "diagnostic_codes": ["OUTPUT_MODE", "TUI_FALLBACK", "AGENT_MEMORY_BACKEND", "BACKEND_FALLBACK", "PALETTE_RESOLVED", "SNAPSHOT_WRITTEN", "FIRST_FRAME"],
    })
}

/// The schema document for one verb.
///
/// Hidden arguments are not advertised: an agent reading the schema sees the
/// same surface `--help` shows.
#[must_use]
pub fn command_schema(cmd: &clap::Command, spec: &CommandSpec) -> Value {
    let mut properties = Map::new();
    let mut required = Vec::new();
    for arg in cmd
        .get_arguments()
        .filter(|a| !a.is_global_set() && !a.is_hide_set())
    {
        let name = arg.get_id().to_string();
        if name == "help" {
            continue;
        }
        let description = arg.get_help().map(ToString::to_string).unwrap_or_default();
        let mut property = match arg.get_action() {
            ArgAction::SetTrue | ArgAction::SetFalse => {
                json!({ "type": "boolean", "description": description })
            }
            ArgAction::Count => {
                json!({ "type": "integer", "minimum": 0, "description": description })
            }
            _ if arg.get_num_args().is_some_and(|n| n.max_values() > 1)
                || matches!(arg.get_action(), ArgAction::Append) =>
            {
                json!({ "type": "array", "items": { "type": "string" }, "description": description })
            }
            _ => json!({ "type": "string", "description": description }),
        };
        let possible: Vec<String> = arg
            .get_possible_values()
            .iter()
            .map(|v| v.get_name().to_owned())
            .collect();
        if !possible.is_empty()
            && let Some(object) = property.as_object_mut()
        {
            object.insert("enum".to_owned(), json!(possible));
        }
        if let Some(default) = arg.get_default_values().first().and_then(|d| d.to_str())
            && let Some(object) = property.as_object_mut()
        {
            object.insert("default".to_owned(), json!(default));
        }
        if let Some(param) = spec.params.iter().find(|p| p.name == name)
            && let Some(object) = property.as_object_mut()
        {
            apply_param(object, param);
        }
        if arg.is_required_set() {
            required.push(name.clone());
        }
        properties.insert(name, property);
    }
    let examples: Vec<Value> = spec
        .examples
        .iter()
        .map(|(command, description)| json!({ "command": command, "description": description }))
        .collect();
    json!({
        "$schema": DIALECT,
        "command": format!("fairing {}", spec.name),
        "description": cmd.get_about().map(ToString::to_string).unwrap_or_default(),
        "parameters": {
            "type": "object",
            "properties": Value::Object(properties),
            "required": required,
            "additionalProperties": false,
        },
        "output_schema": output_schema(spec.name),
        "exit_codes": exit_codes(),
        "examples": examples,
        "supports_json": true,
        "supports_dry_run": spec.supports_dry_run,
        "idempotent": spec.idempotent,
        "destructive": spec.destructive,
    })
}

/// Overlays a parameter's declared type, bounds and pattern on its property.
///
/// The default clap reports is text; for a numeric kind it is re-read as the
/// number it parses to, so `"default": 30` is an integer, not `"30"`.
fn apply_param(object: &mut Map<String, Value>, param: &ParamSpec) {
    object.insert("type".to_owned(), json!(param.kind.json_type()));
    let default = object.get("default").and_then(Value::as_str);
    let typed_default = match (param.kind, default) {
        (ParamKind::Integer, Some(text)) => text.parse::<i64>().ok().map(Value::from),
        (ParamKind::Number, Some(text)) => text.parse::<f64>().ok().map(Value::from),
        (ParamKind::String, _) | (_, None) => None,
    };
    if let Some(value) = typed_default {
        object.insert("default".to_owned(), value);
    }
    if let Some(minimum) = param.minimum {
        object.insert("minimum".to_owned(), json!(minimum));
    }
    if let Some(maximum) = param.maximum {
        object.insert("maximum".to_owned(), json!(maximum));
    }
    if let Some(pattern) = param.pattern {
        object.insert("pattern".to_owned(), json!(pattern));
    }
}

fn output_schema(name: &str) -> Value {
    let data = match name {
        "describe" => json!({
            "type": "object",
            "required": ["tool", "version", "commands", "global_flags", "output_formats", "schema_command", "context_files"],
            "properties": {
                "tool": { "type": "string" },
                "version": { "type": "string" },
                "description": { "type": "string" },
                "license": { "type": "string" },
                "homepage": { "type": "string", "format": "uri" },
                "repository": { "type": "string", "format": "uri" },
                "assurance_category": { "type": "string" },
                "commands": { "type": "array", "items": { "type": "object" } },
                "global_flags": { "type": "array", "items": { "type": "string" } },
                "output_formats": { "type": "array", "items": { "type": "string" } },
                "unavailable_formats": { "type": "array", "items": { "type": "string" } },
                "mcp_available": { "type": "boolean" },
                "schema_command": { "type": "string" },
                "context_files": { "type": "array", "items": { "type": "string" } },
                "profile": { "type": "object" }
            }
        }),
        "preview" => json!({
            "type": "object",
            "required": ["backend", "chain", "width", "height", "format", "palette", "seconds", "frames", "fps", "planned"],
            "properties": {
                "backend": {
                    "type": "string",
                    "enum": ["auto", "drm", "fbdev", "memory"],
                    "description": "The backend that drew; `auto` only in a plan, where the chain decides at run time"
                },
                "chain": {
                    "type": "array",
                    "items": { "type": "string", "enum": ["drm", "fbdev", "memory"] },
                    "description": "The backends tried, in order"
                },
                "device": { "type": ["string", "null"] },
                "width": { "type": ["integer", "null"], "description": "Null in a plan for a device backend: the mode is read from the device" },
                "height": { "type": ["integer", "null"] },
                "format": { "type": ["string", "null"] },
                "palette": {
                    "type": "object",
                    "properties": {
                        "slug": { "type": "string" },
                        "base": { "type": "string" },
                        "source": { "type": "string" },
                        "overlay": { "type": "string" },
                        "skipped": { "type": "array", "items": { "type": "object" } }
                    }
                },
                "seconds": { "type": "number" },
                "fps": { "type": "integer" },
                "frames": { "type": "integer" },
                "dropped": { "type": "integer" },
                "first_frame_ms": { "type": ["number", "null"] },
                "measured_fps": { "type": ["number", "null"] },
                "snapshot": { "type": ["string", "null"] },
                "fallbacks": { "type": "array", "items": { "type": "object" } },
                "planned": { "type": "boolean" }
            }
        }),
        _ => json!({ "type": "object", "description": "A JSON Schema Draft 2020-12 document" }),
    };
    json!({
        "type": "object",
        "required": ["metadata", "data"],
        "properties": {
            "metadata": {
                "type": "object",
                "required": ["tool", "version", "command", "timestamp", "maintainer", "website"],
                "properties": {
                    "tool": { "type": "string" },
                    "version": { "type": "string" },
                    "command": { "type": "string" },
                    "timestamp": { "type": "string", "format": "date-time" },
                    "maintainer": { "type": "string" },
                    "website": { "type": "string", "format": "uri" },
                    "tool_agent": { "type": "string" },
                    "dry_run": { "type": "boolean" }
                }
            },
            "data": data
        }
    })
}

/// Runs `fairing schema [<command>]`.
///
/// # Errors
///
/// `NOT_FOUND` when the named command does not exist; `INVALID_ARGUMENT` for
/// a nested path (the tree is one level deep).
pub fn run(
    path: &[String],
    context: &Context,
    invocation: &str,
    _flags: &GlobalFlags,
) -> Result<(), AppError> {
    let document = match path {
        [] => tool_schema(),
        [name] => {
            let cli = Cli::command();
            let spec = SPECS.iter().find(|s| s.name == name);
            match (cli.find_subcommand(name), spec) {
                (Some(sub), Some(spec)) => command_schema(sub, spec),
                _ => {
                    return Err(AppError::not_found(
                        format!("command `{name}` does not exist"),
                        "fairing describe --json",
                        invocation,
                    ));
                }
            }
        }
        _ => {
            return Err(AppError::invalid_argument(
                format!(
                    "command path `{}` is too deep; the tree is one level",
                    path.join(" ")
                ),
                "fairing schema",
                invocation,
            ));
        }
    };
    let rendered = if context.mode.is_machine() {
        serde_json::to_string(&document)
    } else {
        serde_json::to_string_pretty(&document)
    }
    .map_err(|e| AppError::internal(e.to_string(), invocation))?;
    write_line(&rendered, invocation)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_spec_names_a_real_subcommand_with_a_json_example() {
        let cli = Cli::command();
        for spec in SPECS {
            assert!(
                cli.find_subcommand(spec.name).is_some(),
                "{} missing from clap tree",
                spec.name
            );
            assert!(
                spec.examples.iter().any(|(c, _)| c.contains("--json")),
                "{} lacks a --json example",
                spec.name
            );
        }
    }

    #[test]
    fn every_subcommand_has_a_spec() {
        for sub in Cli::command().get_subcommands() {
            assert!(
                SPECS.iter().any(|s| s.name == sub.get_name()),
                "{} lacks a CommandSpec",
                sub.get_name()
            );
        }
    }

    #[test]
    fn every_param_spec_names_a_visible_argument() {
        let cli = Cli::command();
        for spec in SPECS {
            let sub = cli
                .find_subcommand(spec.name)
                .unwrap_or_else(|| panic!("{} missing", spec.name));
            for param in spec.params {
                assert!(
                    sub.get_arguments()
                        .any(|a| a.get_id() == param.name && !a.is_hide_set()),
                    "{}: `{}` is not a visible argument",
                    spec.name,
                    param.name
                );
            }
        }
    }

    #[test]
    fn schema_parameters_follow_the_clap_tree() {
        let document = tool_schema();
        assert_eq!(document["$schema"], DIALECT);
        let schema_cmd = document["commands"]
            .as_array()
            .and_then(|c| c.iter().find(|c| c["command"] == "fairing schema"));
        let schema_cmd = schema_cmd.unwrap_or(&Value::Null);
        assert_eq!(
            schema_cmd["parameters"]["properties"]["command"]["type"],
            "array"
        );
        assert!(global_flags().iter().any(|f| f == "--json"));
        assert!(
            document["error_codes"]
                .as_array()
                .is_some_and(|codes| codes.iter().any(|c| c == "CONFLICT"))
        );
    }

    #[test]
    fn preview_parameters_are_typed_and_hidden_flags_are_omitted() {
        let cli = Cli::command();
        let sub = cli
            .find_subcommand("preview")
            .unwrap_or_else(|| panic!("preview missing"));
        let spec = SPECS
            .iter()
            .find(|s| s.name == "preview")
            .unwrap_or_else(|| panic!("preview spec missing"));
        let document = command_schema(sub, spec);
        let properties = &document["parameters"]["properties"];
        assert_eq!(properties["seconds"]["type"], "number");
        assert_eq!(properties["seconds"]["minimum"], 0);
        assert_eq!(properties["seconds"]["maximum"], 3600);
        assert_eq!(properties["seconds"]["default"], 5.0);
        assert_eq!(properties["fps"]["type"], "integer");
        assert_eq!(properties["fps"]["default"], 30);
        assert_eq!(properties["fps"]["minimum"], 1);
        assert_eq!(properties["fps"]["maximum"], 240);
        assert_eq!(properties["size"]["type"], "string");
        let pattern = properties["size"]["pattern"]
            .as_str()
            .unwrap_or_default()
            .to_owned();
        let side = regex::Regex::new(&format!(
            "^{}$",
            pattern
                .trim_start_matches('^')
                .trim_end_matches('$')
                .split_once('x')
                .map(|(w, _)| w)
                .unwrap_or_default()
        ))
        .unwrap_or_else(|e| panic!("{e}"));
        // The pattern accepts exactly the sides `parse_size` accepts.
        for n in [
            0_u32, 1, 9, 10, 9999, 10_000, 15_999, 16_000, 16_299, 16_383, 16_384, 16_385, 20_000,
            99_999,
        ] {
            assert_eq!(
                side.is_match(&n.to_string()),
                (1..=16_384).contains(&n),
                "{n}"
            );
        }
        assert!(!side.is_match("+8") && !side.is_match("08") && !side.is_match(" 8"));
        assert_eq!(properties["size"]["default"], "1920x1080");
        assert_eq!(
            properties["backend"]["enum"],
            json!(["auto", "drm", "fbdev", "memory"])
        );
        assert!(
            properties.get("theme").is_none(),
            "hidden flags stay hidden"
        );
        assert_eq!(document["exit_codes"]["5"], "CONFLICT");
        let required = document["output_schema"]["properties"]["data"]["required"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        assert!(required.iter().any(|r| r == "chain"));
    }
}
