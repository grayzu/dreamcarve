// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Keyboard focus (`experience.md`, section 8, "Keyboard focus"; section
//! 11; section 12, "Focus ring"; slice 19, #548).

/// A part of the window Tab walks through, in the walk's order: top-left
/// to bottom-right.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Region {
    /// The menu bar.
    Menu,
    /// The toolbar.
    Toolbar,
    /// The Design Tree.
    Tree,
    /// The Properties panel.
    Properties,
    /// The Viewport, its cards included.
    Viewport,
    /// The Warnings drawer, over the Viewport's bottom.
    Drawer,
    /// The CAM panel: the operations or tools list, then the editor.
    Cam,
    /// The status bar.
    Status,
}

impl Region {
    /// Every region in Tab's order.
    pub const ORDER: [Self; 8] = [
        Self::Menu,
        Self::Toolbar,
        Self::Tree,
        Self::Properties,
        Self::Viewport,
        Self::Drawer,
        Self::Cam,
        Self::Status,
    ];
}
