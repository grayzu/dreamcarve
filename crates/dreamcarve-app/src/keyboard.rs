// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The shortcut table (functional spec, capability 54; slice 19, #548):
//! every keyboard shortcut once, with its menu row and what it does. The
//! menus take their shortcut text from it, the shell's handlers consume
//! keys through it, and the book's Keyboard chapter carries
//! [`chapter_table`], which a test holds to the chapter's text. ⌘ stands
//! for Ctrl on macOS (egui's [`Modifiers::COMMAND`]), and ⌥ for Alt.

use dc_app::{StrKey, Strings};
use eframe::egui::{self, InputState, Key, Modifiers};

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
    /// Edit ▸ Rename: the selected shape's name in place (#617).
    Rename,
    /// Arrange ▸ Rotate 90° Clockwise (#617).
    RotateClockwise,
    /// Arrange ▸ Rotate 90° Anticlockwise.
    RotateAnticlockwise,
    /// Arrange ▸ Flip Horizontal.
    FlipHorizontal,
    /// Arrange ▸ Flip Vertical.
    FlipVertical,
    /// View ▸ Fit View.
    FitView,
    /// View ▸ Zoom to Selection.
    ZoomToSelection,
    /// The toolbar's Tab tool, on or off (#546).
    TabTool,
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
    /// Arrange (#617).
    Arrange,
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
            Self::Arrange => StrKey::AppMenuArrange,
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
    /// Whether this build has the command; the chapter notes one that
    /// does not. Every command is in this build since the export sheet
    /// (#547).
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
    // Opens the export sheet (#547).
    Shortcut {
        command: Command::Export,
        modifiers: COMMAND,
        keys: &[Key::E],
        menu: Some((Menu::File, StrKey::AppMenuExport)),
        does: "Export the G-code programs",
        scope: Scope::Anywhere,
        available: true,
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
        Command::Rename,
        NONE,
        &[Key::F2],
        Menu::Edit,
        StrKey::AppMenuRename,
        "Rename the selected shape in place",
        Scope::OutsideFields,
    ),
    menu_row(
        Command::RotateClockwise,
        NONE,
        &[Key::R],
        Menu::Arrange,
        StrKey::ArrangeMenuRotateClockwise,
        "Rotate the selection 90° clockwise",
        Scope::OutsideFields,
    ),
    menu_row(
        Command::RotateAnticlockwise,
        SHIFT,
        &[Key::R],
        Menu::Arrange,
        StrKey::ArrangeMenuRotateAnticlockwise,
        "Rotate the selection 90° anticlockwise",
        Scope::OutsideFields,
    ),
    menu_row(
        Command::FlipHorizontal,
        COMMAND_SHIFT,
        &[Key::H],
        Menu::Arrange,
        StrKey::ArrangeMenuFlipHorizontal,
        "Flip the selection left to right",
        Scope::OutsideFields,
    ),
    menu_row(
        Command::FlipVertical,
        COMMAND_SHIFT,
        &[Key::V],
        Menu::Arrange,
        StrKey::ArrangeMenuFlipVertical,
        "Flip the selection top to bottom",
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
    key_only(
        Command::TabTool,
        NONE,
        &[Key::T],
        "Turn the Tab tool on or off: a click on a shape's outline adds a tab",
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
        "Clear the selection, turn off the Tab tool, stop picking on the Viewport, or close a menu",
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

/// How many times `command`'s keys were pressed this frame, consuming
/// them (a held arrow repeats). The modifiers must match exactly, unlike
/// egui's `consume_key`, so H and Shift H, or an arrow and Shift arrow,
/// never take each other's presses.
pub fn consume(input: &mut InputState, command: Command) -> usize {
    of(command).map_or(0, |shortcut| {
        shortcut
            .keys
            .iter()
            .map(|key| consume_key(input, command, *key))
            .sum()
    })
}

/// [`consume`] for one of `command`'s keys: an arrow, for a nudge's
/// direction.
pub fn consume_key(input: &mut InputState, command: Command, key: Key) -> usize {
    let Some(shortcut) = of(command).filter(|s| s.keys.contains(&key)) else {
        return 0;
    };
    let mut count = 0;
    input.events.retain(|event| {
        let hit = matches!(
            event,
            egui::Event::Key { key: pressed_key, modifiers, pressed: true, .. }
                if *pressed_key == key && modifiers.matches_exact(shortcut.modifiers)
        );
        count += usize::from(hit);
        !hit
    });
    count
}

/// A key's name in the book, for Windows and Linux, then for macOS.
fn key_names(key: Key) -> (&'static str, &'static str) {
    match key {
        Key::Delete => ("Delete", "⌦"),
        Key::Backspace => ("Backspace", "⌫"),
        Key::Escape => ("Esc", "Esc"),
        other => (other.name(), other.name()),
    }
}

/// A shortcut's keys as the book writes them for Windows and Linux
/// (`Ctrl+Shift+S`, `Delete or Backspace`, `Shift+arrow keys`), and for
/// macOS (`⇧⌘S`, `⌦ or ⌫`, `⇧ arrow keys`).
#[must_use]
pub fn keys_text(shortcut: &Shortcut) -> (String, String) {
    let m = shortcut.modifiers;
    let mut pc: Vec<&str> = Vec::new();
    let mut mac = String::new();
    for (held, name, glyph) in [
        (m.command, "Ctrl", ""),
        (m.alt, "Alt", "⌥"),
        (m.shift, "Shift", "⇧"),
    ] {
        if held {
            pc.push(name);
            mac.push_str(glyph);
        }
    }
    if m.command {
        mac.push('⌘');
    }
    if shortcut.keys == ARROWS {
        return if pc.is_empty() {
            ("Arrow keys".to_owned(), "Arrow keys".to_owned())
        } else {
            (
                format!("{}+arrow keys", pc.join("+")),
                format!("{mac} arrow keys"),
            )
        };
    }
    let (pcs, macs): (Vec<String>, Vec<String>) = shortcut
        .keys
        .iter()
        .map(|key| {
            let (pc_key, mac_key) = key_names(*key);
            let mut chord = pc.clone();
            chord.push(pc_key);
            (chord.join("+"), format!("{mac}{mac_key}"))
        })
        .unzip();
    (pcs.join(" or "), macs.join(" or "))
}

/// The Keyboard chapter's table, as the book carries it between its
/// `shortcuts` markers: one row per shortcut, in [`SHORTCUTS`]' order,
/// with its keys on Windows and Linux and on macOS, what it does, and its
/// menu row.
#[must_use]
pub fn chapter_table() -> String {
    let strings = Strings::english();
    let mut table =
        String::from("| Windows and Linux | macOS | Does | Menu |\n| --- | --- | --- | --- |\n");
    for shortcut in SHORTCUTS {
        let (pc, mac) = keys_text(shortcut);
        let menu = shortcut.menu.map_or_else(
            || "—".to_owned(),
            |(menu, label)| format!("{} ▸ {}", strings.get(menu.title()), strings.get(label)),
        );
        let does = if shortcut.available {
            shortcut.does.to_owned()
        } else {
            format!("{} (arrives with the export sheet)", shortcut.does)
        };
        table.push_str(&format!("| {pc} | {mac} | {does} | {menu} |\n"));
    }
    table
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
        // A Windows checkout may carry CRLF line endings.
        let chapter = std::fs::read_to_string(&path)
            .unwrap_or_default()
            .replace("\r\n", "\n");
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
