// SPDX-FileCopyrightText: 2026 Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>
// SPDX-License-Identifier: GPL-3.0-or-later
// Rust guideline compliant 2026-05-18

//! The `metadata` + `data` envelope (CLI Standard §6) and `--fields` narrowing.

use serde::Serialize;

use crate::cli::{MAINTAINER, WEBSITE};
use crate::error::AppError;
use crate::output::mode::{Context, Mode};
use crate::time::now_iso8601;

/// Response metadata.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Metadata {
    /// Binary name.
    pub tool: &'static str,
    /// Semantic version.
    pub version: &'static str,
    /// The invocation as parsed.
    pub command: String,
    /// ISO 8601 UTC with `Z`.
    pub timestamp: String,
    /// Steelbore Standard §15.2.
    pub maintainer: &'static str,
    /// Steelbore Standard §15.2.
    pub website: &'static str,
    /// Detected informational harness, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_agent: Option<&'static str>,
    /// Present (true) only under `--dry-run`.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub dry_run: bool,
}

/// A complete response.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Response<T: Serialize> {
    /// Metadata.
    pub metadata: Metadata,
    /// Payload: an object for get/describe commands, an array for lists.
    pub data: T,
}

impl<T: Serialize> Response<T> {
    /// Wraps `data` for `command`.
    pub fn new(command: &str, data: T) -> Self {
        Self {
            metadata: Metadata {
                tool: env!("CARGO_PKG_NAME"),
                version: env!("CARGO_PKG_VERSION"),
                command: command.to_owned(),
                timestamp: now_iso8601(),
                maintainer: MAINTAINER,
                website: WEBSITE,
                tool_agent: None,
                dry_run: false,
            },
            data,
        }
    }

    /// Records the harness label and dry-run flag from the context and flags.
    #[must_use]
    pub fn with_context(mut self, context: &Context, dry_run: bool) -> Self {
        self.metadata.tool_agent = context.tool_agent;
        self.metadata.dry_run = dry_run;
        self
    }

    /// Writes the envelope to stdout in the context's machine mode.
    ///
    /// `fields`, when non-empty, keeps only those top-level keys of `data`
    /// (CLI Standard §3 `--fields`); unknown names are ignored.
    ///
    /// # Errors
    ///
    /// Returns `FEATURE_UNAVAILABLE` for `yaml`/`csv` (not shipped yet) and an
    /// internal error if serialisation fails.
    pub fn emit(&self, context: &Context, fields: &[String]) -> Result<(), AppError> {
        let mut value = serde_json::to_value(self)
            .map_err(|e| AppError::internal(e.to_string(), &self.metadata.command))?;
        if !fields.is_empty()
            && let Some(data) = value
                .get_mut("data")
                .and_then(serde_json::Value::as_object_mut)
        {
            data.retain(|key, _| fields.iter().any(|f| f == key));
        }
        match context.mode {
            Mode::Json | Mode::Human | Mode::Jsonl => println!("{value}"),
            Mode::Yaml | Mode::Csv => {
                return Err(AppError::feature_unavailable(
                    format!(
                        "`--format {}` is not available in this release",
                        context.mode.name()
                    ),
                    &self.metadata.command,
                ));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_carries_attribution_and_omits_empty_optionals() {
        let response = Response::new("fairing describe", serde_json::json!({ "a": 1 }));
        let value = serde_json::to_value(&response).unwrap_or_default();
        let metadata = &value["metadata"];
        assert_eq!(metadata["tool"], "fairing");
        assert_eq!(metadata["maintainer"], MAINTAINER);
        assert_eq!(metadata["website"], WEBSITE);
        assert!(metadata.get("tool_agent").is_none());
        assert!(metadata.get("dry_run").is_none());
        assert_eq!(metadata["timestamp"].as_str().map(str::len), Some(20));
    }
}
