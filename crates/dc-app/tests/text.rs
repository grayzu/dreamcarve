// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Slice 24 of phase 5 (`docs/specs/phase-5/design.md`, "Test plan →
//! Text"): the Text tool, the fonts on hand, a text's sizes in
//! Properties, and a text whose font is not on this computer (functional
//! capability 91, acceptance 116).

#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::float_cmp,
    reason = "test code reports failure by panicking"
)]

use std::sync::Arc;

use dc_app::fonts::{FontSource, Fonts};
use dc_app::test_support::Harness;
use dc_app::tool::{Event, Key, SizeField, ToolOption};
use dc_app::{App, CommandError, Drawing, Pointer, Selection, Tool, text};
use dc_geometry::{Point, Rect};
use dc_project::{Align, FontRef, InstanceId, Shape, Units};

fn app() -> App {
    let mut app = Harness::from_project(App::new_project().unwrap()).app;
    app.prefs_mut().snap.enabled = false;
    app.arm(Tool::Draw(Drawing::Text));
    app
}

fn click(app: &mut App, x: f64, y: f64) {
    let at = Pointer {
        at_mm: Point::new(x, y),
        shift: false,
        alt: false,
        command: false,
    };
    app.tool_event(Event::Press(at));
    app.tool_event(Event::Release(at));
}

fn typed(app: &mut App, words: &str) {
    app.tool_event(Event::Text(words.to_owned()));
}

fn field(app: &mut App, field: SizeField, text: &str) {
    app.tool_event(Event::Field {
        field,
        text: text.to_owned(),
    });
}

fn font(family: &str, style: &str) -> FontRef {
    FontRef {
        family: family.to_owned(),
        style: style.to_owned(),
    }
}

fn bounds(outlines: &[dc_geometry::Polyline]) -> Rect {
    let mut joint: Option<Rect> = None;
    for outline in outlines {
        let b = outline.bounds().unwrap();
        joint = Some(joint.map_or(b, |a| a.including(b.min).including(b.max)));
    }
    joint.unwrap()
}

fn id(s: &str) -> InstanceId {
    InstanceId::new(s)
}

/// Capability 91: a click sets where the baseline starts, the bar takes
/// the words and previews them as they are typed, and Done makes one
/// shape named *Text 1* that keeps what it was made from.
#[test]
fn text_places_and_edits() {
    let mut app = app();
    assert!(Drawing::Text.is_ready());
    let bar = app.tool_bar().unwrap();
    assert_eq!(bar.prompt, "Click where the text starts");
    assert!(bar.text.is_none() && !bar.can_finish);
    // The fonts on hand, as a list: the bundled ones.
    let fonts = &bar.choices[0];
    assert!(fonts.list && fonts.label == "Font");
    let names: Vec<&str> = fonts.options.iter().map(|o| o.label.as_str()).collect();
    assert_eq!(
        names,
        [
            "Inter",
            "Inter Bold",
            "Source Serif 4",
            "Source Serif 4 Bold",
            "Roboto Slab Bold",
            "Pacifico",
            "Allerta Stencil",
            "Simple Stroke"
        ]
    );
    assert_eq!(fonts.selected, 0);
    let rows: Vec<(SizeField, &str)> = bar
        .fields
        .iter()
        .map(|row| (row.field, row.text.as_str()))
        .collect();
    assert_eq!(
        rows,
        [
            (SizeField::TextHeight, "10"),
            (SizeField::LetterSpacing, "0"),
            (SizeField::LineSpacing, "100"),
        ]
    );
    let align: Vec<&str> = bar.choices[1]
        .options
        .iter()
        .map(|o| o.label.as_str())
        .collect();
    assert_eq!(
        (bar.choices[1].label.as_str(), align),
        ("Lines", vec!["Left", "Centre", "Right"])
    );

    click(&mut app, 20.0, 30.0);
    let bar = app.tool_bar().unwrap();
    assert_eq!(bar.prompt, "Type the text, then Done");
    assert_eq!(bar.text.as_ref().map(|t| t.text.as_str()), Some(""));
    assert!(!bar.can_finish, "nothing typed yet");
    typed(&mut app, "HI");
    assert!(app.tool_bar().unwrap().can_finish);
    let preview = bounds(&app.draft().outlines);
    assert!(
        (preview.max.y - 40.0).abs() < 0.02,
        "an H 10 mm tall: {preview:?}"
    );
    assert!(preview.min.x > 20.0 && preview.min.x < 22.0 && (preview.min.y - 30.0).abs() < 0.02);

    let reply = app.tool_event(Event::Done);
    assert_eq!(reply.ran.as_deref(), Some("Draw “Text 1”"));
    let project = app.project();
    let instance = &project.instances[0];
    assert_eq!(instance.name, "Text 1");
    assert_eq!(
        instance.transform,
        dc_geometry::Transform::translate(20.0, 30.0)
    );
    let definition = &project.definitions[&instance.definition];
    assert_eq!(
        definition.shape,
        Some(Shape::Text {
            text: "HI".to_owned(),
            font: font("Inter", "Regular"),
            height_mm: 10.0,
            letter_spacing_fraction: 0.0,
            line_spacing_fraction: 1.0,
            align: Align::Left,
        })
    );
    assert_eq!(definition.profiles.len(), 2, "an H and an I");
    assert!(
        definition
            .profiles
            .iter()
            .all(dc_geometry::Polyline::is_closed)
    );
    // The tool stays armed, for the next text.
    assert_eq!(app.tool(), Tool::Draw(Drawing::Text));
    assert!(app.tool_bar().unwrap().text.is_none());
    assert!(app.undo().is_some());
    assert!(app.project().instances.is_empty(), "one undo step");
}

/// The tool's options: the font, the height, the spacings and the
/// alignment are kept for the texts that follow; several lines; the
/// single-line font gives open paths; a character the font lacks is
/// said in the bar.
#[test]
fn the_text_tools_options() {
    let mut app = app();
    click(&mut app, 10.0, 50.0);
    typed(&mut app, "AB\nC");
    let two_lines = bounds(&app.draft().outlines);
    assert!(two_lines.min.y < 45.0, "the second line is under the first");
    field(&mut app, SizeField::TextHeight, "20");
    assert!((bounds(&app.draft().outlines).max.y - 70.0).abs() < 0.02);
    field(&mut app, SizeField::LetterSpacing, "50");
    field(&mut app, SizeField::LineSpacing, "200");
    assert_eq!(app.prefs().text.letter_spacing_fraction, 0.5);
    assert_eq!(app.prefs().text.line_spacing_fraction, 2.0);
    app.tool_event(Event::Option(ToolOption::TextAlign(Align::Right)));
    assert_eq!(app.tool_bar().unwrap().choices[1].selected, 2);
    // A size that is none is said at its field and changes nothing.
    field(&mut app, SizeField::TextHeight, "0");
    let bar = app.tool_bar().unwrap();
    assert!(bar.fields[0].problem.is_some() && !bar.can_finish);
    field(&mut app, SizeField::TextHeight, "20");

    let stroke = font("Simple Stroke", "Regular");
    app.tool_event(Event::Option(ToolOption::Font(stroke.clone())));
    assert_eq!(app.tool_bar().unwrap().choices[0].selected, 7);
    assert!(app.draft().outlines.iter().all(|o| !o.is_closed()));
    typed(&mut app, "A#");
    assert_eq!(
        app.tool_bar().unwrap().note.as_deref(),
        Some("The font has no glyph for “#”")
    );
    app.tool_event(Event::Key(Key::Enter));
    let Some(Shape::Text {
        font,
        height_mm,
        align,
        ..
    }) = app
        .project()
        .definitions
        .values()
        .next()
        .unwrap()
        .shape
        .clone()
    else {
        panic!("a text");
    };
    assert_eq!((font, height_mm, align), (stroke, 20.0, Align::Right));
    // The options stand for the next text.
    click(&mut app, 10.0, 10.0);
    assert_eq!(app.tool_bar().unwrap().fields[0].text, "20");

    // Escape takes the click back, then puts the tool down.
    app.tool_event(Event::Key(Key::Escape));
    assert!(app.tool_bar().unwrap().text.is_none());
    assert_eq!(app.tool(), Tool::Draw(Drawing::Text));
    app.tool_event(Event::Key(Key::Escape));
    assert_eq!(app.tool(), Tool::Select);

    // An inch project starts at 0.4 in.
    let mut project = App::new_project().unwrap();
    project.display_units = Units::Inch;
    let mut inch = Harness::from_project(project).app;
    inch.arm(Tool::Draw(Drawing::Text));
    assert_eq!(inch.tool_bar().unwrap().fields[0].text, "0.4");
}

/// The fonts on hand: the bundled ones, then those the user adds, each
/// once; a file that is no font is refused with the reason.
#[test]
fn a_font_the_user_adds_is_offered() {
    let mut app = app();
    assert_eq!(app.fonts().entries().len(), 8);
    assert!(
        app.fonts()
            .entries()
            .iter()
            .all(|f| f.source == FontSource::Bundled)
    );
    let refused = app.add_font(Arc::from(&b"not a font"[..])).unwrap_err();
    assert!(
        refused
            .to_string()
            .starts_with("this is not a font file the app can read"),
        "{refused}"
    );
    // A font file under names of its own: the bundled Pacifico's bytes
    // are a font like any the user has.
    let bytes = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../dc-text/fonts/pacifico/Pacifico-Regular.ttf"
    ))
    .unwrap();
    // The same names as a bundled font: the bundled one stands.
    let added = app.add_font(Arc::from(bytes.as_slice())).unwrap();
    assert_eq!(added, font("Pacifico", "Regular"));
    assert_eq!(app.fonts().entries().len(), 8);

    let mut fonts = Fonts::default();
    assert!(fonts.entries().is_empty());
    let added = fonts.add(Arc::from(bytes.as_slice())).unwrap();
    assert_eq!(added, font("Pacifico", "Regular"));
    fonts.add(Arc::from(bytes.as_slice())).unwrap();
    assert_eq!(fonts.entries().len(), 1, "once");
    assert_eq!(fonts.entries()[0].source, FontSource::User);
    assert!(fonts.find(&added).is_some());
}

/// A text's sizes in Properties: the words, the font, the height, the
/// spacings and the alignment, each one undo step that lays the outline
/// out again where the text is.
#[test]
fn a_texts_sizes_are_edited_in_properties() {
    let mut app = app();
    click(&mut app, 20.0, 30.0);
    typed(&mut app, "HI");
    app.tool_event(Event::Done);
    app.arm(Tool::Select);
    let strings = dc_app::Strings::english();
    let section = text::section(app.project(), &strings, app.fonts(), &id("i1")).unwrap();
    assert_eq!(section.title, "Text");
    assert_eq!(section.text.as_ref().map(|t| t.text.as_str()), Some("HI"));
    assert!(!section.read_only && section.note.is_none());
    let rows: Vec<(SizeField, &str)> = section
        .rows
        .iter()
        .map(|row| (row.field, row.text.as_str()))
        .collect();
    assert_eq!(
        rows,
        [
            (SizeField::TextHeight, "10"),
            (SizeField::LetterSpacing, "0"),
            (SizeField::LineSpacing, "100"),
        ]
    );
    assert!(section.choices[0].list && section.choices[0].selected == 0);

    let edit = |app: &mut App, edit: text::TextEdit| {
        let command = text::set(app.project(), &strings, app.fonts(), &id("i1"), &edit)?;
        app.run(command)
    };
    let width = |app: &App| {
        let project = app.project();
        bounds(&project.definitions[&project.instances[0].definition].profiles).width_mm()
    };
    let before = width(&app);
    assert_eq!(
        edit(&mut app, text::TextEdit::Text("HIGH".to_owned())).unwrap(),
        "Edit the text of “Text 1”"
    );
    assert!(width(&app) > before * 1.5);
    assert_eq!(
        edit(
            &mut app,
            text::TextEdit::Size(SizeField::TextHeight, "25".to_owned())
        )
        .unwrap(),
        "Set height of “Text 1”"
    );
    let project = app.project();
    let outline = bounds(&project.definitions[&project.instances[0].definition].profiles);
    // The capitals are 25 mm; the round of the G reaches a little over.
    assert!(
        outline.max.y > 25.0 - 0.03 && outline.max.y < 25.6,
        "{outline:?}"
    );
    assert_eq!(
        project.instances[0].transform,
        dc_geometry::Transform::translate(20.0, 30.0),
        "it stays where it is"
    );
    assert_eq!(
        edit(
            &mut app,
            text::TextEdit::Font(font("Source Serif 4", "Bold"))
        )
        .unwrap(),
        "Set font of “Text 1”"
    );
    assert_eq!(
        edit(&mut app, text::TextEdit::Align(Align::Centre)).unwrap(),
        "Set alignment of “Text 1”"
    );
    assert_eq!(
        edit(
            &mut app,
            text::TextEdit::Size(SizeField::LetterSpacing, "10".to_owned())
        )
        .unwrap(),
        "Set letter spacing of “Text 1”"
    );
    // What changes nothing, and what cannot be, are no step.
    assert!(matches!(
        edit(&mut app, text::TextEdit::Align(Align::Centre)),
        Err(CommandError::NothingToDo)
    ));
    assert!(
        edit(
            &mut app,
            text::TextEdit::Size(SizeField::TextHeight, "-3".to_owned())
        )
        .is_err()
    );
    assert!(matches!(
        edit(&mut app, text::TextEdit::Text("   ".to_owned())),
        Err(CommandError::EmptyOutline)
    ));
    // A text does not stretch, and is not edited node by node until it
    // is made a path.
    app.select(Selection::Instances(vec![id("i1")]));
    let stretched = app.run(dc_app::Command::TransformInstances {
        ids: vec![id("i1")],
        delta: dc_geometry::Transform::scale(2.0, 1.0),
    });
    assert!(stretched.is_err());
}

/// Acceptance 116: a project does not carry its fonts. Without the
/// text's font its outline is kept and cut, its sizes are read-only and
/// say why, and choosing a font that is here, or adding the missing
/// one, makes it a text again.
#[test]
fn a_missing_font_keeps_the_outline_and_refuses_edits() {
    let mut app = app();
    app.tool_event(Event::Option(ToolOption::Font(font("Pacifico", "Regular"))));
    click(&mut app, 20.0, 30.0);
    typed(&mut app, "Hook");
    app.tool_event(Event::Done);
    app.arm(Tool::Select);
    let strings = dc_app::Strings::english();
    let project = app.project().clone();
    let kept = project.definitions[&project.instances[0].definition]
        .profiles
        .clone();

    // On a computer with no font at all.
    let mut fonts = Fonts::default();
    let section = text::section(&project, &strings, &fonts, &id("i1")).unwrap();
    assert!(section.read_only);
    assert_eq!(
        section.note.as_deref(),
        Some("Font “Pacifico” is not on this computer: the outline is kept")
    );
    assert_eq!(section.text.as_ref().map(|t| t.text.as_str()), Some("Hook"));
    for edit in [
        text::TextEdit::Text("Hooks".to_owned()),
        text::TextEdit::Size(SizeField::TextHeight, "20".to_owned()),
        text::TextEdit::Align(Align::Right),
    ] {
        let refused = text::set(&project, &strings, &fonts, &id("i1"), &edit).unwrap_err();
        assert_eq!(
            refused.to_string(),
            "font “Pacifico” is not on this computer: choose another font, or add it"
        );
    }
    // The outline is the file's: nothing lays it out again.
    assert_eq!(
        project.definitions[&project.instances[0].definition].profiles,
        kept
    );

    // The missing font added: a text again.
    let bytes = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../dc-text/fonts/pacifico/Pacifico-Regular.ttf"
    ))
    .unwrap();
    fonts.add(Arc::from(bytes.as_slice())).unwrap();
    let section = text::section(&project, &strings, &fonts, &id("i1")).unwrap();
    assert!(!section.read_only && section.note.is_none());
    assert!(
        text::set(
            &project,
            &strings,
            &fonts,
            &id("i1"),
            &text::TextEdit::Text("Hooks".to_owned())
        )
        .is_ok()
    );
    // Or another font chosen, with the app's own.
    assert!(
        text::set(
            &project,
            &strings,
            app.fonts(),
            &id("i1"),
            &text::TextEdit::Font(font("Inter", "Bold"))
        )
        .is_ok()
    );
}
