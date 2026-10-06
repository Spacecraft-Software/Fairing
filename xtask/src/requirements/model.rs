// SPDX-FileCopyrightText: 2026 Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>
// SPDX-License-Identifier: GPL-3.0-or-later
// Rust guideline compliant 2026-05-18

//! Serde model of `doc/requirements.toml`.
//!
//! Attributes follow Steelbore Standard §20.3: identifier, rationale, source,
//! priority, verification method and status. Unknown keys are rejected so a
//! typo cannot silently drop an attribute.

use serde::{Deserialize, Serialize};

/// The whole requirement set.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RequirementSet {
    /// Project-level metadata.
    pub meta: Meta,
    /// Requirement groups, in the order they appear in the manual.
    #[serde(default)]
    pub group: Vec<Group>,
    /// Stakeholder needs (§20.1).
    #[serde(default)]
    pub need: Vec<Need>,
    /// Software requirements (§20.1).
    #[serde(default)]
    pub requirement: Vec<Requirement>,
}

/// Project metadata carried into the generated chapters.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Meta {
    /// Registered project name, e.g. `Fairing`.
    pub project: String,
    /// §20.3 identifier prefix, e.g. `FRN`.
    pub prefix: String,
    /// Steelbore Standard version the set is written against.
    pub standard: String,
    /// Declared assurance category (§19.2).
    pub category: String,
    /// ISO 8601 date of the last change to the set.
    pub updated: String,
    /// Prose that opens the Needs chapter.
    #[serde(default)]
    pub needs_preamble: String,
    /// Prose that opens the Requirements chapter.
    #[serde(default)]
    pub requirements_preamble: String,
}

/// A requirement group: one Texinfo section.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Group {
    /// Short key referenced by requirements, e.g. `display`.
    pub id: String,
    /// Texinfo node name; unique, free of `, : . ( ) @`.
    pub node: String,
    /// Section title.
    pub title: String,
    /// One-paragraph introduction (source need, decision), may be empty.
    #[serde(default)]
    pub intro: String,
}

/// A stakeholder need.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Need {
    /// `<PREFIX>-NEED-<NNN>`.
    pub id: String,
    /// Short title.
    pub title: String,
    /// The need, as the user states it.
    pub text: String,
    /// Who holds the need.
    #[serde(default)]
    pub who: String,
}

/// Priority per §20.3; only `mandatory` and `expected` count toward §17 figures.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "lowercase")]
pub enum Priority {
    /// Required for the release.
    Mandatory,
    /// Expected; its absence is a tailoring entry.
    Expected,
    /// Never counted.
    Optional,
}

/// Verification method per §21.1.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Verification {
    /// Execute and compare.
    Test,
    /// Reason over the artifact.
    Analysis,
    /// Examine a visible property.
    Inspection,
    /// Structured reading of the design.
    Review,
}

/// Lifecycle status per §20.3; the denominator of the §17 figures.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    /// Written, not yet baselined at G1.
    Draft,
    /// Baselined; evidence is owed.
    Baselined,
    /// Code exists; evidence not yet complete.
    Implemented,
    /// Evidence exists and passes.
    Verified,
    /// Withdrawn; keeps its identifier forever.
    Withdrawn,
}

impl Status {
    /// Whether the requirement is part of a baseline (baselined, implemented or verified).
    #[must_use]
    pub const fn is_baselined(self) -> bool {
        matches!(self, Self::Baselined | Self::Implemented | Self::Verified)
    }
}

/// One software requirement.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Requirement {
    /// `<PREFIX>-SRS-<NNN>`; permanent, never reused.
    pub id: String,
    /// The [`Group::id`] this requirement belongs to.
    pub group: String,
    /// The milestone that discharges it, e.g. `M1`.
    pub milestone: String,
    /// Short title (≤ 60 characters).
    pub title: String,
    /// The requirement sentence in the §20.6 pattern.
    pub text: String,
    /// Why this is required; one sentence.
    pub rationale: String,
    /// Need identifiers, decisions or Standard clauses it traces to.
    #[serde(default)]
    pub source: Vec<String>,
    /// §20.3 priority.
    pub priority: Priority,
    /// §21.1 method.
    pub verification: Verification,
    /// §20.3 status.
    pub status: Status,
    /// Assurance category override for a raised subsystem (§19.1), e.g. `A`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    /// Optional free-form note.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

impl Requirement {
    /// Whether the requirement enters §17 denominators.
    #[must_use]
    pub const fn is_counted(&self) -> bool {
        matches!(self.priority, Priority::Mandatory | Priority::Expected)
            && !matches!(self.status, Status::Withdrawn)
    }
}

impl Priority {
    /// Lowercase name as written in the TOML.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Mandatory => "mandatory",
            Self::Expected => "expected",
            Self::Optional => "optional",
        }
    }
}

impl Verification {
    /// Lowercase name as written in the TOML.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Test => "test",
            Self::Analysis => "analysis",
            Self::Inspection => "inspection",
            Self::Review => "review of design",
        }
    }
}

impl Status {
    /// Lowercase name as written in the TOML.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::Baselined => "baselined",
            Self::Implemented => "implemented",
            Self::Verified => "verified",
            Self::Withdrawn => "withdrawn",
        }
    }
}
