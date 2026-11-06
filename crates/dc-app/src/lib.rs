// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

#![doc = include_str!("../README.md")]

pub mod app;
pub mod command;
mod error;
pub mod generation;
pub mod history;
pub mod prefs;
pub mod scene;
pub mod selection;
pub mod strings;
pub mod units;
pub mod view;
pub mod viewport;
pub mod warnings;

#[cfg(any(test, feature = "test-support"))]
pub mod test_support;

pub use app::{App, Opened};
pub use command::Command;
pub use error::CommandError;
pub use generation::{
    FinishJob, Generation, Job, JobResult, OperationResult, Progress, SnapshotId, Work,
};
pub use history::LabelledHistory;
pub use prefs::{Panels, Prefs, Theme, WindowGeometry};
pub use scene::Scene;
pub use selection::{ClickMode, Selection};
pub use strings::{StrKey, Strings};
pub use view::{ColourMode, View, Visibility};
pub use viewport::camera::{Camera, Pos};
pub use viewport::pick::Hit;
