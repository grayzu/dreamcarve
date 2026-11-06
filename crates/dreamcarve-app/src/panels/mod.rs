// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The panels and sheets (`docs/specs/phase-4/design.md`, "Module
//! layout"): the side panels, the Design Tree, the Properties panel's
//! placement fields, the empty state, the unsaved-changes sheet, the CAM
//! panel's Tools tab, the Tool Library sheet and the Import drawing
//! sheet.

pub(crate) mod empty;
pub(crate) mod import;
pub(crate) mod library;
pub(crate) mod project;
pub(crate) mod properties;
pub(crate) mod side;
pub(crate) mod tools;
pub(crate) mod tree;
pub(crate) mod unsaved;
pub(crate) mod warnings;
