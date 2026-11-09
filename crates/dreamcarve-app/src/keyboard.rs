// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The shortcut table (functional spec, capability 54; slice 19, #548):
//! every keyboard shortcut once, with its menu row and what it does. The
//! menus take their shortcut text from it, the shell's handlers consume
//! keys through it, and the book's Keyboard chapter carries
//! [`chapter_table`], which a test holds to the chapter's text. ⌘ stands
//! for Ctrl on macOS (egui's [`Modifiers::COMMAND`]), and ⌥ for Alt.

use dc_app::{StrKey, Strings};
use eframe::egui::{InputState, Key, Modifiers};

/// What a shortcut asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Command {
    /// File ▸ New.
    New,
    /// File ▸ Open….
    Open,
    /// File ▸ Save.
    Save,
    /// File ▸ Save As….
    SaveAs,
    /// File ▸ Import Drawing….
    Import,
    /// File ▸ Export….
    Export,
    /// File ▸ Quit.
    Quit,
    /// Edit ▸ Undo.
    Undo,
    /// Edit ▸ Redo.
    Redo,
    /// Edit ▸ Select All: every visible shape.
    SelectAll,
    /// Edit ▸ Duplicate: linked copies of the selection.
    Duplicate,
    /// Edit ▸ Delete: the selection.
    Delete,
    /// Edit ▸ Hide: the selection.
    Hide,
    /// Edit ▸ Show: the selection.
    Show,
    /// Edit ▸ Lock, or Unlock when every selected shape is locked.
    Lock,
    /// View ▸ Fit View.
    FitView,
    /// View ▸ Zoom to Selection.
    ZoomToSelection,
    /// Nudge the selection 1 mm.
    Nudge,
    /// Nudge the selection 10 mm.
    NudgeCoarse,
    /// Nudge the selection 0.1 mm.
    NudgeFine,
    /// Clear the selection, disarm a pick, or close a menu.
    Escape,
}

/// The menu a shortcut's row is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Menu {
    /// File.
    File,
    /// Edit.
    Edit,
    /// View.
    View,
}

impl Menu {
    /// Its title in the menu bar.
    #[must_use]
    pub const fn title(self) -> StrKey {
        match self {
            Self::File => StrKey::AppMenuFile,
            Self::Edit => StrKey::AppMenuEdit,
            Self::View => StrKey::AppMenuView,
        }
    }
}

/// Where a shortcut is read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// Anywhere outside a sheet, even while a field is being typed in:
    /// the file and history commands.
    Anywhere,
    /// Only while no text field has the keyboard, so typing a letter, an
    /// arrow or Ctrl A in a field stays the field's.
    OutsideFields,
}

/// One shortcut.
#[derive(Debug, Clone, Copy)]
pub struct Shortcut {
    /// What it asks for.
    pub command: Command,
    /// The modifiers held.
    pub modifiers: Modifiers,
    /// The keys, any of which triggers it (Delete or Backspace; the four
    /// arrows).
    pub keys: &'static [Key],
    /// Its menu row: the menu and the row's label.
    pub menu: Option<(Menu, StrKey)>,
    /// What it does, as the Keyboard chapter says it.
    pub does: &'static str,
    /// Where it is read.
    pub scope: Scope,
    /// Whether this build has the command. Export's dialog arrives with
    /// slice 18 (#547); until then Ctrl/⌘ E is bound to nothing and its
    /// menu row is disabled.
    pub available: bool,
}

/// No modifier.
const NONE: Modifiers = Modifiers::NONE;
/// Ctrl, or ⌘ on macOS.
const COMMAND: Modifiers = Modifiers::COMMAND;
/// Shift.
const SHIFT: Modifiers = Modifiers::SHIFT;
/// Alt, or ⌥ on macOS.
const ALT: Modifiers = Modifiers::ALT;
/// Ctrl and Shift, or ⌘ and ⇧ on macOS.
const COMMAND_SHIFT: Modifiers = Modifiers {
    alt: false,
    ctrl: false,
    shift: true,
    mac_cmd: false,
    command: true,
};

/// The four arrows.
pub const ARROWS: &[Key] = &[
    Key::ArrowLeft,
    Key::ArrowRight,
    Key::ArrowUp,
    Key::ArrowDown,
];

/// A shortcut in `menu` labelled `label`.
const fn menu_row(
    command: Command,
    modifiers: Modifiers,
    keys: &'static [Key],
    menu: Menu,
    label: StrKey,
    does: &'static str,
    scope: Scope,
) -> Shortcut {
    Shortcut {
        command,
        modifiers,
        keys,
        menu: Some((menu, label)),
        does,
        scope,
        available: true,
    }
}

/// A shortcut with no menu row.
const fn key_only(
    command: Command,
    modifiers: Modifiers,
    keys: &'static [Key],
    does: &'static str,
) -> Shortcut {
    Shortcut {
        command,
        modifiers,
        keys,
        menu: None,
        does,
        scope: Scope::OutsideFields,
        available: true,
    }
}

/// Every shortcut, in the Keyboard chapter's order (capability 54).
pub const SHORTCUTS: &[Shortcut] = &[
    menu_row(
        Command::New,
        COMMAND,
        &[Key::N],
        Menu::File,
        StrKey::AppMenuNew,
        "Start a new project",
        Scope::Anywhere,
    ),
    menu_row(
        Command::Open,
        COMMAND,
        &[Key::O],
        Menu::File,
        StrKey::AppMenuOpen,
        "Open a project",
        Scope::Anywhere,
    ),
    menu_row(
        Command::Save,
        COMMAND,
        &[Key::S],
        Menu::File,
        StrKey::AppMenuSave,
        "Save the project",
        Scope::Anywhere,
    ),
    menu_row(
        Command::SaveAs,
        COMMAND_SHIFT,
        &[Key::S],
        Menu::File,
        StrKey::AppMenuSaveAs,
        "Save the project under a new name",
        Scope::Anywhere,
    ),
    menu_row(
        Command::Import,
        COMMAND,
        &[Key::I],
        Menu::File,
        StrKey::AppMenuImportDrawing,
        "Import an SVG or DXF drawing",
        Scope::Anywhere,
    ),
    // Export's dialog is slice 18's (#547): the binding is listed, and
    // the handler skips it until then.
    Shortcut {
        command: Command::Export,
        modifiers: COMMAND,
        keys: &[Key::E],
        menu: Some((Menu::File, StrKey::AppMenuExport)),
        does: "Export the G-code programs",
        scope: Scope::Anywhere,
        available: false,
    },
    menu_row(
        Command::Quit,
        COMMAND,
        &[Key::Q],
        Menu::File,
        StrKey::AppMenuQuit,
        "Quit DreamCarve",
        Scope::Anywhere,
    ),
    menu_row(
        Command::Undo,
        COMMAND,
        &[Key::Z],
        Menu::Edit,
        StrKey::AppMenuUndo,
        "Undo the last change",
        Scope::Anywhere,
    ),
    menu_row(
        Command::Redo,
        COMMAND_SHIFT,
        &[Key::Z],
        Menu::Edit,
        StrKey::AppMenuRedo,
        "Redo the change undone",
        Scope::Anywhere,
    ),
    menu_row(
        Command::SelectAll,
        COMMAND,
        &[Key::A],
        Menu::Edit,
        StrKey::AppMenuSelectAll,
        "Select every visible shape",
        Scope::OutsideFields,
    ),
    menu_row(
        Command::Duplicate,
        COMMAND,
        &[Key::D],
        Menu::Edit,
        StrKey::AppMenuDuplicate,
        "Make linked copies of the selection",
        Scope::OutsideFields,
    ),
    menu_row(
        Command::Delete,
        NONE,
        &[Key::Delete, Key::Backspace],
        Menu::Edit,
        StrKey::AppMenuDelete,
        "Delete the selection",
        Scope::OutsideFields,
    ),
    menu_row(
        Command::Hide,
        NONE,
        &[Key::H],
        Menu::Edit,
        StrKey::AppMenuHide,
        "Hide the selection",
        Scope::OutsideFields,
    ),
    menu_row(
        Command::Show,
        SHIFT,
        &[Key::H],
        Menu::Edit,
        StrKey::AppMenuShow,
        "Show the selection",
        Scope::OutsideFields,
    ),
    menu_row(
        Command::Lock,
        NONE,
        &[Key::L],
        Menu::Edit,
        StrKey::AppMenuLock,
        "Lock the selection, or unlock it when every shape in it is locked",
        Scope::OutsideFields,
    ),
    menu_row(
        Command::FitView,
        NONE,
        &[Key::F],
        Menu::View,
        StrKey::AppViewportFitView,
        "Fit the Viewport to the stock and shapes",
        Scope::OutsideFields,
    ),
    menu_row(
        Command::ZoomToSelection,
        SHIFT,
        &[Key::F],
        Menu::View,
        StrKey::AppViewportZoomSelection,
        "Fit the Viewport to the selection",
        Scope::OutsideFields,
    ),
    key_only(Command::Nudge, NONE, ARROWS, "Nudge the selection 1 mm"),
    key_only(
        Command::NudgeCoarse,
        SHIFT,
        ARROWS,
        "Nudge the selection 10 mm",
    ),
    key_only(
        Command::NudgeFine,
        ALT,
        ARROWS,
        "Nudge the selection 0.1 mm",
    ),
    key_only(
        Command::Escape,
        NONE,
        &[Key::Escape],
        "Clear the selection, stop picking on the Viewport, or close a menu",
    ),
];

/// The shortcut of `command` (every command has one row).
#[must_use]
pub fn of(command: Command) -> Option<&'static Shortcut> {
    SHORTCUTS.iter().find(|s| s.command == command)
}

/// The binding a menu row shows for `command`: its modifiers and first
/// key.
#[must_use]
pub fn binding(command: Command) -> Option<(Modifiers, Key)> {
    let shortcut = of(command)?;
    shortcut.keys.first().map(|key| (shortcut.modifiers, *key))
}

/// Whether `command`'s keys were pressed this frame, consuming them.
/// Extra Shift or Alt is not ignored, so H and Shift H stay apart.
pub fn consume(input: &mut InputState, command: Command) -> bool {
    let _ = (input, command);
    false
}

/// The Keyboard chapter's table, as the book carries it between its
/// `shortcuts` markers.
#[must_use]
pub fn chapter_table() -> String {
    String::new()
}

/// A shortcut's keys as the book writes them for Windows and Linux
/// (`Ctrl+Shift+S`) and for macOS (`⇧⌘S`).
#[must_use]
pub fn keys_text(shortcut: &Shortcut, strings: &Strings) -> (String, String) {
    let _ = (shortcut, strings);
    (String::new(), String::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The marker lines the chapter's generated table sits between.
    const BEGIN: &str =
        "<!-- shortcuts:begin (generated from crates/dreamcarve-app/src/keyboard.rs) -->";
    const END: &str = "<!-- shortcuts:end -->";

    /// Capability 54: the book's Keyboard chapter lists every shortcut,
    /// and its table is [`chapter_table`] exactly, so the menus, the
    /// handlers and the chapter read one table. On a mismatch the message
    /// carries the table to paste.
    #[test]
    fn chapter_table_matches_the_shortcut_table() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../docs/book/src/user-guide/keyboard.md");
        let chapter = std::fs::read_to_string(&path).unwrap_or_default();
        let table = chapter
            .split_once(BEGIN)
            .and_then(|(_, rest)| rest.split_once(END))
            .map(|(table, _)| table.trim().to_owned());
        let expected = chapter_table();
        assert_eq!(
            table.as_deref(),
            Some(expected.trim()),
            "{} carries the shortcut table between its markers:\n{BEGIN}\n{expected}\n{END}",
            path.display()
        );
        for shortcut in SHORTCUTS {
            assert!(
                expected.contains(shortcut.does),
                "the chapter says what {:?} does",
                shortcut.command
            );
        }
    }

    #[test]
    fn every_command_has_one_row() {
        for shortcut in SHORTCUTS {
            let rows = SHORTCUTS
                .iter()
                .filter(|s| s.command == shortcut.command)
                .count();
            assert_eq!(rows, 1, "{:?} has one row", shortcut.command);
            assert!(
                !shortcut.keys.is_empty(),
                "{:?} has a key",
                shortcut.command
            );
        }
    }

    #[test]
    fn no_two_shortcuts_share_keys() {
        for (n, a) in SHORTCUTS.iter().enumerate() {
            for b in SHORTCUTS.iter().skip(n + 1) {
                let shared = a.keys.iter().any(|k| b.keys.contains(k));
                assert!(
                    !(shared && a.modifiers == b.modifiers),
                    "{:?} and {:?} share a key",
                    a.command,
                    b.command
                );
            }
        }
    }
}
