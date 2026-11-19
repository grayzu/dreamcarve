// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Identifier newtypes for project entities (`docs/specs/phase-1/design.md`,
//! "Data model → dc-project"). Each is a string in the file, so ids stay
//! readable in a hand-edited `.dcv`, and a distinct type in code, so a tool
//! id can never be passed where an instance id is expected.

/// Declares a string id newtype `$name`, with the doc comment given: a
/// plain JSON string in a file, ordered, shown as its text, and made from
/// a `&str` or a `String`. `dc-library` declares its tool id with it. The
/// calling crate needs `serde`.
#[macro_export]
macro_rules! id_type {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        ///
        /// Serialized as a plain JSON string. Ordered, so it can key a
        /// `BTreeMap` and sort deterministically.
        #[derive(
            Debug,
            Clone,
            PartialEq,
            Eq,
            PartialOrd,
            Ord,
            Hash,
            ::serde::Serialize,
            ::serde::Deserialize,
        )]
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

        impl ::std::fmt::Display for $name {
            fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
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
    /// Identifies a [`crate::PlacedTab`].
    TabId
);
id_type!(
    /// Identifies a [`crate::Bitmap`] (format 27): `b1`, `b2`, … A
    /// bitmap is not an instance, and its id is not an [`InstanceId`].
    BitmapId
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
