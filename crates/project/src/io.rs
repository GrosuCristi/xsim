//! Reading and writing `.xsim_proj` files.
//!
//! * Pretty-printed JSON, struct-field order, two-space indent, LF line
//!   endings and a trailing newline: saving an unchanged project produces
//!   byte-identical output (Q42).
//! * Floats round-trip exactly (`serde_json` `float_roundtrip`).
//! * Saves are atomic: write a temporary file in the same directory, flush,
//!   then rename over the target.
//! * Older schemas are migrated on load (Q34). Schema 1 is the format of the
//!   first prototype (see `tests/fixtures/schema1_rc_lowpass.xsim_proj`).
//! * Newer schemas are opened best-effort (Q45): unknown fields are ignored
//!   and [`Compatibility::Newer`] tells the UI to warn and open read-only.
//!
//! The schema is detected from the raw `schema_version` field before any
//! typed parsing: every document type has defaults for missing fields, so
//! without this check an empty object would pass for a project. A missing,
//! zero or non-integer version is an error.
//!
//! Every loaded project and every project about to be saved passes the same
//! structural validation (see `validate`), so a file this version writes can
//! always be read back and loaded data cannot make later code panic.
//!
//! A project loaded from a newer schema keeps that `schema_version`, and
//! [`Project::save`] refuses it: writing it back would silently drop
//! whatever the newer version stored. To keep a copy in the current format
//! the caller sets `schema_version` to [`SCHEMA_VERSION`] deliberately.

mod header;
mod newer;
mod schema1;
mod validate;

use crate::document::Project;
use crate::error::ProjectError;
use serde::Deserialize;
use serde_json::Value;
use std::path::Path;

pub(crate) use validate::validate;

/// Current native schema version.
pub const SCHEMA_VERSION: u32 = 2;

/// Project file extension (without dot).
pub const PROJECT_EXTENSION: &str = "xsim_proj";

/// Reserved symbol file extension (Q32); symbol files are not yet written.
pub const SYMBOL_EXTENSION: &str = "xsim_sym";

/// How a loaded file relates to [`SCHEMA_VERSION`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Compatibility {
    Current,
    /// Upgraded from an older schema; saving writes the current schema.
    Migrated {
        from: u32,
    },
    /// Written by a newer xsim. Some data may be missing; open read-only.
    Newer {
        version: u32,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct LoadedProject {
    pub project: Project,
    pub compatibility: Compatibility,
}

impl Project {
    /// Parses project JSON of any known or newer schema version.
    pub fn from_json(text: &str) -> Result<LoadedProject, ProjectError> {
        let version = schema_version(header::schema_version(text)?.as_ref())?;
        if version == SCHEMA_VERSION {
            // The common case decodes straight from the text, without an
            // intermediate JSON tree.
            return checked(serde_json::from_str(text)?, Compatibility::Current);
        }
        convert(&serde_json::from_str(text)?, version)
    }

    /// Canonical JSON text (see module docs).
    pub fn to_json(&self) -> String {
        // Project types contain no maps with non-string keys and no fallible
        // `Serialize` impls, so serialisation into memory cannot fail.
        let mut text =
            serde_json::to_string_pretty(self).expect("project types always serialise to JSON");
        text.push('\n');
        text
    }

    pub fn load(path: &Path) -> Result<LoadedProject, ProjectError> {
        let text = std::fs::read_to_string(path)?;
        Self::from_json(&text)
    }

    /// Atomically writes the project to `path`.
    ///
    /// Fails without touching `path` when the project is invalid (for
    /// example a non-finite value, which JSON cannot represent) or was
    /// loaded from a newer schema (see the module docs).
    pub fn save(&self, path: &Path) -> Result<(), ProjectError> {
        match self.schema_version {
            SCHEMA_VERSION => {}
            v if v > SCHEMA_VERSION => {
                return Err(ProjectError::Invalid(format!(
                    "This project was created by a newer version of xsim (file format {v}). \
                     Saving it would discard data this version cannot read."
                )));
            }
            v => {
                return Err(ProjectError::Invalid(format!(
                    "Cannot save a project with file format {v}; this version writes format {SCHEMA_VERSION}."
                )));
            }
        }
        validate(self)?;
        crate::durable::replace_file(path, self.to_json().as_bytes())?;
        Ok(())
    }
}

/// [`Project::from_json`] for an already parsed document. Recovery
/// snapshots use it for the project they embed.
pub(crate) fn project_from_value(value: &Value) -> Result<LoadedProject, ProjectError> {
    let fields = value.as_object().ok_or_else(not_a_project)?;
    let version = schema_version(fields.get("schema_version"))?;
    if version == SCHEMA_VERSION {
        return checked(Project::deserialize(value)?, Compatibility::Current);
    }
    convert(value, version)
}

/// Migrates an older document, or reads a newer one best-effort.
fn convert(value: &Value, version: u32) -> Result<LoadedProject, ProjectError> {
    if version > SCHEMA_VERSION {
        checked(
            newer::parse(value, version)?,
            Compatibility::Newer { version },
        )
    } else {
        // Schema 1 is the only older schema.
        checked(
            schema1::migrate(value)?,
            Compatibility::Migrated { from: version },
        )
    }
}

fn checked(project: Project, compatibility: Compatibility) -> Result<LoadedProject, ProjectError> {
    validate(&project)?;
    Ok(LoadedProject {
        project,
        compatibility,
    })
}

/// A raw `schema_version` as a positive integer that fits `u32`. Every
/// version from 1 to [`SCHEMA_VERSION`] is known.
fn schema_version(version: Option<&Value>) -> Result<u32, ProjectError> {
    let version = version
        .ok_or_else(|| ProjectError::Invalid("The project file has no schema_version.".into()))?;
    version
        .as_u64()
        .and_then(|v| u32::try_from(v).ok())
        .filter(|&v| v >= 1)
        .ok_or_else(|| {
            ProjectError::Invalid(format!(
                "The project file has an invalid schema_version ({version})."
            ))
        })
}

fn not_a_project() -> ProjectError {
    ProjectError::Invalid("This file is not an xsim project.".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn invalid(text: &str) -> String {
        match Project::from_json(text) {
            Err(ProjectError::Invalid(message)) => message,
            other => panic!("expected an Invalid error, got {other:?}"),
        }
    }

    #[test]
    fn schema_version_must_be_a_positive_integer() {
        assert!(invalid("[]").contains("not an xsim project"));
        assert!(invalid("{}").contains("no schema_version"));
        for bad in ["0", "-1", "1.5", "\"2\"", "null", "4294967296"] {
            let text = format!("{{\"schema_version\": {bad}}}");
            assert!(invalid(&text).contains("invalid schema_version"), "{bad}");
        }
    }

    #[test]
    fn malformed_json_is_a_format_error() {
        assert!(matches!(
            Project::from_json("{\"schema_version\": 2,"),
            Err(ProjectError::Format(_))
        ));
        assert!(matches!(
            Project::from_json(""),
            Err(ProjectError::Format(_))
        ));
    }

    #[test]
    fn wrongly_typed_current_schema_is_a_format_error() {
        let text = r#"{"schema_version": 2, "circuits": "none"}"#;
        assert!(matches!(
            Project::from_json(text),
            Err(ProjectError::Format(_))
        ));
    }

    #[test]
    fn to_json_ends_with_a_single_newline_and_uses_lf() {
        let text = Project::new("P").to_json();
        assert!(text.ends_with("}\n"));
        assert!(!text.ends_with("\n\n"));
        assert!(!text.contains('\r'));
        assert!(text.starts_with("{\n  \"schema_version\": 2,\n  \"name\": \"P\","));
    }

    #[test]
    fn save_refuses_other_schema_versions() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("p.xsim_proj");
        let mut project = Project::new("P");
        project.schema_version = SCHEMA_VERSION + 1;
        assert!(project.save(&path).is_err());
        project.schema_version = 0;
        assert!(project.save(&path).is_err());
        assert!(!path.exists());
    }
}
