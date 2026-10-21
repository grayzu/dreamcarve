// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Identifier newtypes for project entities (`docs/specs/phase-1/design.md`,
//! "Data model → dc-project"). Each is a string in the file, so ids stay
//! readable in a hand-edited `.dcv`, and a distinct type in code, so a tool
//! id can never be passed where an instance id is expected.

use serde::{Deserialize, Serialize};
use std::fmt;

macro_rules! id_type {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        ///
        /// Serialized as a plain JSON string. Ordered, so it can key a
        /// `BTreeMap` and sort deterministically.
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            /// Wraps a string as this id type.
            pub fn new(id: impl Into<String>) -> Self {
                Self(id.into())
            }

            /// The id as a string slice.
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl From<&str> for $name {
            fn from(id: &str) -> Self {
                Self::new(id)
            }
        }

        impl From<String> for $name {
            fn from(id: String) -> Self {
                Self(id)
            }
        }
    };
}

id_type!(
    /// Identifies a [`crate::FeatureDefinition`].
    DefinitionId
);
id_type!(
    /// Identifies a [`crate::FeatureInstance`].
    InstanceId
);
id_type!(
    /// Identifies a [`crate::Tool`] embedded in the project.
    ToolId
);
id_type!(
    /// Identifies an [`crate::Operation`].
    OperationId
);
id_type!(
    /// Identifies a [`crate::PlacedTab`] (#105).
    TabId
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn id_serializes_as_plain_string() {
        let id = ToolId::new("t1");
        assert_eq!(serde_json::to_string(&id).unwrap(), "\"t1\"");
        assert_eq!(serde_json::from_str::<ToolId>("\"t1\"").unwrap(), id);
    }
}
