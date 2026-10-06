// SPDX-FileCopyrightText: 2026 Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>
// SPDX-License-Identifier: GPL-3.0-or-later
// Rust guideline compliant 2026-05-18

//! `fairing describe`: the capability manifest (CLI Standard `schema-introspection.md` §2).

use clap::CommandFactory as _;
use serde::Serialize;

use crate::cli::{Cli, GlobalFlags};
use crate::error::AppError;
use crate::output::envelope::Response;
use crate::output::mode::Context;
use crate::output::write_line;
use crate::schema;

#[derive(Debug, Serialize)]
struct CommandSummary {
    name: String,
    description: String,
    supports_json: bool,
    supports_dry_run: bool,
    idempotent: bool,
    destructive: bool,
}

#[derive(Debug, Serialize)]
struct Profile {
    mode: &'static str,
    color: bool,
    interactive: bool,
    reader: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_agent: Option<&'static str>,
}

#[derive(Debug, Serialize)]
struct Manifest {
    tool: &'static str,
    version: &'static str,
    description: String,
    license: &'static str,
    homepage: &'static str,
    repository: &'static str,
    assurance_category: &'static str,
    standard: &'static str,
    cli_standard: &'static str,
    commands: Vec<CommandSummary>,
    global_flags: Vec<String>,
    output_formats: &'static [&'static str],
    unavailable_formats: &'static [&'static str],
    mcp_available: bool,
    schema_command: &'static str,
    context_files: &'static [&'static str],
    profile: Profile,
}

fn manifest(context: &Context) -> Manifest {
    let cli = Cli::command();
    let commands = schema::SPECS
        .iter()
        .filter_map(|spec| {
            cli.find_subcommand(spec.name).map(|sub| CommandSummary {
                name: format!("fairing {}", spec.name),
                description: sub.get_about().map(ToString::to_string).unwrap_or_default(),
                supports_json: true,
                supports_dry_run: spec.supports_dry_run,
                idempotent: spec.idempotent,
                destructive: spec.destructive,
            })
        })
        .collect();
    Manifest {
        tool: env!("CARGO_PKG_NAME"),
        version: env!("CARGO_PKG_VERSION"),
        description: cli.get_about().map(ToString::to_string).unwrap_or_default(),
        license: "GPL-3.0-or-later",
        homepage: crate::cli::WEBSITE,
        repository: "https://github.com/Spacecraft-Software/Fairing",
        assurance_category: "B; fairing-askpass and the initrd lifecycle unit raised to A",
        standard: "The Steelbore Standard v2.12",
        cli_standard: "Spacecraft Software Dual-Mode Self-Documenting CLI Standard v1.1.0",
        commands,
        global_flags: schema::global_flags(),
        output_formats: &["human", "json", "jsonl"],
        unavailable_formats: &["yaml", "csv", "explore"],
        mcp_available: false,
        schema_command: "fairing schema",
        context_files: &["AGENTS.md", "CLAUDE.md", "SKILL.md", "CONTRIBUTING.md"],
        profile: Profile {
            mode: context.mode.name(),
            color: context.color,
            interactive: context.interactive,
            reader: context.profile.name(),
            tool_agent: context.tool_agent,
        },
    }
}

/// Runs `fairing describe`.
///
/// # Errors
///
/// Propagates envelope serialisation and unavailable-format errors.
pub fn run(context: &Context, invocation: &str, flags: &GlobalFlags) -> Result<(), AppError> {
    let manifest = manifest(context);
    if context.mode.is_machine() {
        return Response::new(invocation, manifest)
            .with_context(context, flags.dry_run)
            .emit(context, &flags.fields);
    }
    let header = [
        format!(
            "{} {} — {}",
            manifest.tool, manifest.version, manifest.description
        ),
        format!(
            "profile: {}\tmode: {}\tcolor: {}\tinteractive: {}",
            manifest.profile.reader,
            manifest.profile.mode,
            if manifest.profile.color { "on" } else { "off" },
            yes_no(manifest.profile.interactive)
        ),
        format!("assurance: {}", manifest.assurance_category),
        "command\tdescription\tjson\tdry_run\tidempotent\tdestructive".to_owned(),
    ];
    for line in &header {
        write_line(line, invocation)?;
    }
    for command in &manifest.commands {
        write_line(
            &format!(
                "{}\t{}\t{}\t{}\t{}\t{}",
                command.name,
                command.description,
                yes_no(command.supports_json),
                yes_no(command.supports_dry_run),
                yes_no(command.idempotent),
                yes_no(command.destructive)
            ),
            invocation,
        )?;
    }
    let footer = [
        format!("global_flags\t{}", manifest.global_flags.join(" ")),
        format!("output_formats\t{}", manifest.output_formats.join(" ")),
        format!("schema_command\t{}", manifest.schema_command),
    ];
    for line in &footer {
        write_line(line, invocation)?;
    }
    Ok(())
}

const fn yes_no(value: bool) -> &'static str {
    if value { "yes" } else { "no" }
}
