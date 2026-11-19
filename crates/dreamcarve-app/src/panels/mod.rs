// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The panels and sheets (`docs/specs/phase-4/design.md`, "Module
//! layout"; `docs/specs/phase-4/redesign.md`, "The steps"): the step
//! panel and the Inspector ([`side`]) and what they hold for each step
//! (the Design Tree, the Properties' placement fields and the Arrange
//! group; the stock and the output settings; the tools and the tool
//! editor; the operations and the operation editor; the Export step's
//! readiness card), the start screen, the Warnings drawer, and the
//! sheets: unsaved changes, the Tool Library, Import drawing and export.

pub(crate) mod arrange;
pub(crate) mod clearing;
pub(crate) mod editor;
pub(crate) mod empty;
pub(crate) mod export;
pub(crate) mod export_step;
pub(crate) mod import;
pub(crate) mod library;
pub(crate) mod operations;
pub(crate) mod project;
pub(crate) mod properties;
pub(crate) mod result;
pub(crate) mod side;
pub(crate) mod tools;
pub(crate) mod tree;
pub(crate) mod unsaved;
pub(crate) mod warnings;
