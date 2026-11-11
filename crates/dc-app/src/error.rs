// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The crate's error type: why a command could not be applied, a file could
//! not be opened or saved, or a typed value is not a length. The shell
//! shows the message in the status bar (functional spec, capability 58: "a
//! locked instance ... refuses every transform, and the status bar says
//! why").

use dc_geometry::GeometryError;
use dc_job::JobError;
use dc_project::{
    InstanceId, LoadError, OperationId, PostProcessorError, SaveError, TabId, ToolId,
};

/// Why the app refused what it was asked.
#[derive(Debug, thiserror::Error)]
pub enum CommandError {
    /// A transform named a locked instance (functional spec, capability
    /// 58). `names` are the locked instances', in selection order.
    #[error("{}", locked_text(names))]
    Locked {
        /// The locked instances' names.
        names: Vec<String>,
    },
    /// A command that acts on a selection was given none.
    #[error("nothing is selected")]
    NothingSelected,
    /// A command was given nothing to change.
    #[error("nothing to change")]
    NothingToDo,
    /// The project has no instance with this id.
    #[error("the project has no shape `{id}`")]
    UnknownInstance {
        /// The id that was named.
        id: InstanceId,
    },
    /// An instance names a definition the project does not have.
    #[error("shape `{instance}` has no definition `{definition}`")]
    UnknownDefinition {
        /// The instance.
        instance: InstanceId,
        /// The definition it names.
        definition: dc_project::DefinitionId,
    },
    /// The project has no operation with this id.
    #[error("the project has no operation `{id}`")]
    UnknownOperation {
        /// The id that was named.
        id: OperationId,
    },
    /// The project has no tool with this id.
    #[error("the project has no tool `{id}`")]
    UnknownTool {
        /// The id that was named.
        id: ToolId,
    },
    /// The project has no tab with this id.
    #[error("the project has no tab `{id}`")]
    UnknownTab {
        /// The id that was named.
        id: TabId,
    },
    /// Something was added under an id the project already uses.
    #[error("the project already has {kind} `{id}`")]
    DuplicateId {
        /// What was added: "a tool", "an operation", "a tab".
        kind: &'static str,
        /// The id.
        id: String,
    },
    /// A tool an operation uses cannot be removed (functional spec,
    /// capability 63).
    #[error("tool “{tool}” is used by {}", operations_text(operations))]
    ToolInUse {
        /// The tool's name.
        tool: String,
        /// The operations that use it, in project order.
        operations: Vec<String>,
    },
    /// A position in the operation list that does not exist.
    #[error("position {index} is past the end of the {len} operations")]
    IndexOutOfRange {
        /// The position asked for.
        index: usize,
        /// How many operations there are.
        len: usize,
    },
    /// A number that cannot be stored: not finite, or out of the field's
    /// range (a negative margin). `dc_project::load` rejects such values
    /// at its boundary, so the app rejects them at its own.
    #[error("`{field}` cannot be {value}")]
    InvalidValue {
        /// The field, as the file names it.
        field: &'static str,
        /// The value.
        value: f64,
    },
    /// A transform that cannot place a shape: zero scale, or not finite.
    #[error("the transform collapses the shape and cannot be applied")]
    SingularTransform,
    /// A typed value is not a length or a feed rate (functional spec,
    /// capability 53). `reason` is `dc_project::units_input`'s message,
    /// which says what would be accepted.
    #[error("{reason}")]
    NotAQuantity {
        /// What was typed.
        text: String,
        /// Why it was refused.
        reason: String,
    },
    /// An operation's field cannot hold the value an editor row built:
    /// the record has no such path, or the file's reader refuses the
    /// result (capability 66).
    #[error("`{field}` cannot be set: {reason}")]
    FieldRefused {
        /// The key path.
        field: String,
        /// The reader's reason.
        reason: String,
    },
    /// The import sheet's *Import* is disabled: `reason` is what the sheet
    /// shows (the importer's refusal, or nothing to import).
    #[error("{reason}")]
    NotImportable {
        /// Why the drawing cannot be imported with these options.
        reason: String,
    },
    /// A change was asked while an operation's targets are being edited
    /// (functional spec, capability 67: "nothing else can be selected or
    /// moved"): *Done* or *Cancel* comes first.
    #[error("finish editing the targets first: Done or Cancel")]
    EditingTargets,
    /// *Fit stock to shapes* with no visible shape to fit round.
    #[error("there are no visible shapes to fit the stock to")]
    NoShapes,
    /// Placing a shape failed in `dc-geometry`.
    #[error(transparent)]
    Geometry(#[from] GeometryError),
    /// A `dc-job` step refused: placing tabs, usually.
    #[error(transparent)]
    Job(#[from] JobError),
    /// A post-processor id names no bundled definition.
    #[error(transparent)]
    PostProcessor(#[from] PostProcessorError),
    /// The bytes being opened are not a project.
    #[error(transparent)]
    Load(#[from] LoadError),
    /// The project could not be serialized.
    #[error(transparent)]
    Save(#[from] SaveError),
    /// The tool library refused a change: an invalid value, a taken name
    /// or an unknown tool (capability 64a).
    #[error(transparent)]
    Library(#[from] dc_library::LibraryError),
}

/// `shape “A” is locked` or `shapes “A” and “B” are locked`.
fn locked_text(names: &[String]) -> String {
    let quoted: Vec<String> = names.iter().map(|n| format!("“{n}”")).collect();
    match quoted.as_slice() {
        [] => "the selection is locked".to_owned(),
        [one] => format!("shape {one} is locked"),
        _ => format!("shapes {} are locked", join_and(&quoted)),
    }
}

/// `“A”` or `“A” and “B”` or `“A”, “B” and “C”`.
fn operations_text(names: &[String]) -> String {
    let quoted: Vec<String> = names.iter().map(|n| format!("“{n}”")).collect();
    join_and(&quoted)
}

fn join_and(items: &[String]) -> String {
    match items.split_last() {
        None => String::new(),
        Some((last, [])) => last.clone(),
        Some((last, rest)) => format!("{} and {last}", rest.join(", ")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locked_message_names_the_shapes() {
        let one = CommandError::Locked {
            names: vec!["Oval".into()],
        };
        assert_eq!(one.to_string(), "shape “Oval” is locked");
        let two = CommandError::Locked {
            names: vec!["Oval".into(), "Star".into()],
        };
        assert_eq!(two.to_string(), "shapes “Oval” and “Star” are locked");
    }

    #[test]
    fn tool_in_use_message_lists_the_operations() {
        let error = CommandError::ToolInUse {
            tool: "1/4 end mill".into(),
            operations: vec!["Panel".into(), "Square recess".into(), "Cut out".into()],
        };
        assert_eq!(
            error.to_string(),
            "tool “1/4 end mill” is used by “Panel”, “Square recess” and “Cut out”"
        );
    }
}
