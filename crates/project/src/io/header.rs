//! Reads a document's `schema_version` without building the rest of it.
//!
//! Current-schema files are then decoded straight into the typed project,
//! so loading a project with a large stored result never holds a second,
//! untyped copy of it in memory.

use crate::error::ProjectError;
use serde::de::{Deserializer, IgnoredAny, MapAccess, Visitor};
use serde_json::Value;
use std::fmt;

/// The raw `schema_version` member of the JSON object in `text`, if any.
///
/// Malformed JSON is a [`ProjectError::Format`]; valid JSON that is not an
/// object is [`ProjectError::Invalid`].
pub(super) fn schema_version(text: &str) -> Result<Option<Value>, ProjectError> {
    let mut deserializer = serde_json::Deserializer::from_str(text);
    let version = deserializer
        .deserialize_map(VersionVisitor)
        .and_then(|version| deserializer.end().map(|()| version));
    version.map_err(|e| match e.classify() {
        serde_json::error::Category::Data => super::not_a_project(),
        _ => ProjectError::Format(e),
    })
}

struct VersionVisitor;

impl<'de> Visitor<'de> for VersionVisitor {
    type Value = Option<Value>;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("an xsim project object")
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
        let mut version = None;
        while let Some(key) = map.next_key::<std::borrow::Cow<'de, str>>()? {
            if key == "schema_version" {
                version = Some(map.next_value::<Value>()?);
            } else {
                map.next_value::<IgnoredAny>()?;
            }
        }
        Ok(version)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_version_among_other_members() {
        let text =
            r#"{"name": "x", "circuits": [{"a": [1, 2, {"b": null}]}], "schema_version": 7}"#;
        assert_eq!(schema_version(text).unwrap(), Some(Value::from(7)));
        assert_eq!(schema_version("{}").unwrap(), None);
        assert_eq!(
            schema_version(r#"{"schema_version": "two"}"#).unwrap(),
            Some(Value::from("two"))
        );
    }

    #[test]
    fn distinguishes_malformed_json_from_non_objects() {
        for text in ["", "{", "{} trailing", "{\"schema_version\": 2,}"] {
            assert!(
                matches!(schema_version(text), Err(ProjectError::Format(_))),
                "{text}"
            );
        }
        for text in ["[]", "2", "\"project\"", "null"] {
            assert!(
                matches!(schema_version(text), Err(ProjectError::Invalid(_))),
                "{text}"
            );
        }
    }
}
