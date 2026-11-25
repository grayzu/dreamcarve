// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! `dreamcarve simulate` (phase 6 functional spec, capability 125;
//! acceptance 131, 153): the report and the picture of the committed
//! projects, the files it will and will not write over, what it refuses,
//! `--until`, `--board` and `--cell`.

use super::*;

/// A committed project of `tests/fixtures/`.
fn project(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures")
        .join(name)
}

/// What `simulate` is expected to say of `name`, and its picture.
fn expected(name: &str) -> (String, PathBuf) {
    let dir = project("simulate");
    let text = std::fs::read_to_string(dir.join(format!("{name}.txt"))).expect("the report");
    (text, dir.join(format!("{name}.png")))
}

/// A 16-bit grayscale PNG's size and samples.
fn gray16(path: &Path) -> ((u32, u32), Vec<u16>) {
    let bytes = std::fs::read(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    let mut reader = decoder.read_info().expect("a PNG");
    let info = reader.info();
    assert_eq!(info.color_type, png::ColorType::Grayscale);
    assert_eq!(info.bit_depth, png::BitDepth::Sixteen);
    let size = (info.width, info.height);
    let mut buffer = vec![0; reader.output_buffer_size().expect("a size")];
    reader.next_frame(&mut buffer).expect("the pixels");
    let (pairs, _) = buffer.as_chunks::<2>();
    let samples = pairs.iter().map(|pair| u16::from_be_bytes(*pair)).collect();
    (size, samples)
}

/// The report, less the line that names the picture's path, and that
/// line.
fn split_report(out: &Output) -> (String, Option<String>) {
    let text = stdout(out);
    let wrote = text
        .lines()
        .find(|l| l.starts_with("Wrote heightmap "))
        .map(str::to_owned);
    let rest: String = text
        .lines()
        .filter(|l| !l.starts_with("Wrote heightmap "))
        .map(|l| format!("{l}\n"))
        .collect();
    (rest, wrote)
}

/// Acceptance 153: for the plaque and the sign at Standard, and the
/// lettered inlay's male board at Draft, the report is the committed
/// text and every pixel of the picture is within one level of the
/// committed one (a tool's angle goes through the platform's `tan`, so a
/// height may differ in its last bit from one platform to another).
#[test]
fn the_report_and_the_picture_of_each_fixture() {
    let dir = work_dir("simulate_report_and_picture");
    for (name, dcv, extra) in [
        ("plaque", "plaque.dcv", &[][..]),
        ("sign", "sign.dcv", &[][..]),
        (
            "dc-male",
            "dc.dcv",
            &["--board", "male", "--quality", "draft"][..],
        ),
    ] {
        let picture = dir.join(format!("{name}.png"));
        let mut args = vec![
            "simulate".to_owned(),
            project(dcv).to_string_lossy().into_owned(),
            "--out".to_owned(),
            picture.to_string_lossy().into_owned(),
        ];
        args.extend(extra.iter().map(|a| (*a).to_owned()));
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        let out = run(&dir, &args);
        assert_eq!(out.status.code(), Some(0), "{name}: {}", stderr(&out));
        let (report, wrote) = split_report(&out);
        let (want, want_picture) = expected(name);
        assert_eq!(report, want, "{name}");
        let wrote = wrote.unwrap_or_else(|| panic!("{name}: no picture line"));
        assert!(
            wrote.contains("16-bit: white is the board's top, black its bottom"),
            "{wrote}"
        );

        let (size, got) = gray16(&picture);
        let (want_size, want) = gray16(&want_picture);
        assert_eq!(size, want_size, "{name}");
        let worst = got
            .iter()
            .zip(&want)
            .map(|(a, b)| a.abs_diff(*b))
            .max()
            .expect("pixels");
        assert!(worst <= 1, "{name}: a pixel is {worst} levels off");
        // White is the top and black the bottom: an uncut corner of the
        // stock, and a cell the outline cut through.
        assert_eq!(got[0], u16::MAX, "{name}: the top-left corner is uncut");
        assert!(got.contains(&0), "{name}: something is cut through");
    }
}

/// `--out` writes over nothing, or over a picture `simulate` wrote
/// before, and over no other file; it must name a `.png`, and not the
/// project.
#[test]
fn a_file_that_is_not_its_picture_is_kept() {
    let dir = work_dir("simulate_out_path");
    let dcv = project("plaque.dcv");
    let dcv = dcv.to_string_lossy();
    let simulate = |out: &str| {
        run(
            &dir,
            &["simulate", &dcv, "--quality", "draft", "--out", out],
        )
    };

    // Someone's own picture, and a file that is no picture at all.
    std::fs::copy(project("photo").join("gradient.png"), dir.join("mine.png")).ok();
    if !dir.join("mine.png").exists() {
        std::fs::write(dir.join("mine.png"), b"not a png at all").expect("written");
    }
    std::fs::write(dir.join("notes.png"), b"not a png at all").expect("written");
    for kept in ["mine.png", "notes.png"] {
        let before = std::fs::read(dir.join(kept)).expect("read");
        let out = simulate(kept);
        assert_eq!(out.status.code(), Some(1), "{kept}");
        assert!(
            stderr(&out).contains("exists and is not a DreamCarve heightmap"),
            "{}",
            stderr(&out)
        );
        assert_eq!(
            std::fs::read(dir.join(kept)).expect("read"),
            before,
            "{kept} was changed"
        );
    }
    // A new file is written, and written over by the next run.
    let out = simulate("stock.png");
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let first = std::fs::read(dir.join("stock.png")).expect("the picture");
    let out = run(
        &dir,
        &[
            "simulate",
            &dcv,
            "--quality",
            "draft",
            "--until",
            "op3",
            "--out",
            "stock.png",
        ],
    );
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert_ne!(
        std::fs::read(dir.join("stock.png")).expect("the picture"),
        first
    );
    // Not a .png; the project itself.
    let out = simulate("stock.tiff");
    assert_eq!(out.status.code(), Some(1));
    assert!(
        stderr(&out).contains("--out must name a .png file"),
        "{}",
        stderr(&out)
    );
    std::fs::copy(project("plaque.dcv"), dir.join("copy.png")).expect("copied");
    let out = run(&dir, &["simulate", "copy.png", "--out", "copy.png"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        stderr(&out).contains("is the project file"),
        "{}",
        stderr(&out)
    );
}

/// A project `check` refuses is refused by `simulate` with `check`'s
/// error lines, and nothing is written.
#[test]
fn a_refused_project_is_refused_as_check_refuses_it() {
    let dir = work_dir("simulate_refused");
    let text = std::fs::read_to_string(project("plaque.dcv")).expect("the plaque");
    // The stock's thickness unset: the first thing a new project lacks.
    let unset = text.replacen("\"thickness_mm\": 11.8", "\"thickness_mm\": null", 1);
    assert_ne!(unset, text);
    std::fs::write(dir.join("plaque.dcv"), unset).expect("written");

    let check = run(&dir, &["check", "plaque.dcv"]);
    let errors: Vec<String> = stdout(&check)
        .lines()
        .filter(|l| l.contains(": error: "))
        .map(str::to_owned)
        .collect();
    assert!(!errors.is_empty(), "{}", stdout(&check));

    let out = run(&dir, &["simulate", "plaque.dcv", "--out", "stock.png"]);
    assert_eq!(out.status.code(), Some(1));
    let printed = stdout(&out);
    for error in &errors {
        assert!(
            printed.contains(error.as_str()),
            "missing `{error}` in {printed}"
        );
    }
    assert!(
        stderr(&out).contains("nothing was simulated"),
        "{}",
        stderr(&out)
    );
    assert!(!dir.join("stock.png").exists());
}

/// `--until` stops after the operation named: the report counts the
/// runs so far and says the rest are left out, the deepest cut is that
/// of the operations so far, and the check for uncut material is not
/// made. An id no enabled operation has is an error.
#[test]
fn until_stops_after_the_operation() {
    let dir = work_dir("simulate_until");
    let dcv = project("plaque.dcv");
    let dcv = dcv.to_string_lossy();
    // The plaque cuts op3, op5, op1, op2, op4: after op1, three runs.
    let out = run(
        &dir,
        &["simulate", &dcv, "--quality", "draft", "--until", "op1"],
    );
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let text = stdout(&out);
    assert!(text.contains("3 operations in 3 tool runs"), "{text}");
    assert!(
        text.contains("The 2 tool runs after them are left out (--until)."),
        "{text}"
    );
    assert!(
        text.contains("Deepest cut: 11.800 mm by `Screw holes` (op3)."),
        "{text}"
    );
    assert!(!text.contains("ncut material"), "{text}");

    let out = run(&dir, &["simulate", &dcv, "--until", "op9"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        stderr(&out).contains("--until op9: no enabled operation"),
        "{}",
        stderr(&out)
    );
}

/// The boards are simulated apart (acceptance 150): the inlay's stock
/// by default, its male board with `--board male`; a project with no
/// male board has none to simulate.
#[test]
fn the_male_board_is_its_own_picture() {
    let dir = work_dir("simulate_boards");
    let dc = project("dc.dcv");
    let dc = dc.to_string_lossy();
    let female = stdout(&run(&dir, &["simulate", &dc, "--quality", "draft"]));
    let male = stdout(&run(
        &dir,
        &["simulate", &dc, "--quality", "draft", "--board", "male"],
    ));
    assert!(female.starts_with("Simulated the stock, "), "{female}");
    assert!(female.contains("Not cut through anywhere."), "{female}");
    assert!(male.starts_with("Simulated the male board, "), "{male}");
    assert!(male.contains("Cut through: "), "{male}");
    // Nothing is asked of the male board, so no check is printed for it.
    assert!(!male.contains("ncut material"), "{male}");

    let plaque = project("plaque.dcv");
    let out = run(
        &dir,
        &["simulate", &plaque.to_string_lossy(), "--board", "male"],
    );
    assert_eq!(out.status.code(), Some(1));
    assert!(
        stderr(&out).contains("cuts no male board"),
        "{}",
        stderr(&out)
    );
}

/// Acceptance 131: a cell too small for the board is refused with the
/// smallest that fits; one under the least cell, and a threshold out of
/// its range, are refused too; `--cell` and `--quality` do not go
/// together.
#[test]
fn a_cell_too_small_is_refused() {
    let dir = work_dir("simulate_cells");
    let text = std::fs::read_to_string(project("plaque.dcv")).expect("the plaque");
    // A full sheet, 1220 × 610 mm.
    let sheet = text
        .replacen("\"width_mm\": 170.0", "\"width_mm\": 1220.0", 1)
        .replacen("\"height_mm\": 120.0", "\"height_mm\": 610.0", 1);
    assert_ne!(sheet, text);
    std::fs::write(dir.join("sheet.dcv"), sheet).expect("written");
    let out = run(&dir, &["simulate", "sheet.dcv", "--cell", "0.02"]);
    assert_eq!(out.status.code(), Some(1));
    let message = stderr(&out);
    assert!(
        message.contains("more than the 16777216 a simulation holds"),
        "{message}"
    );
    assert!(
        message.contains("the smallest cell that fits is 0.211 mm"),
        "{message}"
    );
    // The cell it names is taken.
    let out = run(
        &dir,
        &["simulate", "sheet.dcv", "--cell", "0.211", "--until", "op3"],
    );
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert!(
        stdout(&out).contains("cells of 0.211 mm"),
        "{}",
        stdout(&out)
    );

    let dcv = project("plaque.dcv");
    let dcv = dcv.to_string_lossy();
    let out = run(&dir, &["simulate", &dcv, "--cell", "0.001"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        stderr(&out).contains("a cell must be at least 0.02 mm"),
        "{}",
        stderr(&out)
    );
    let out = run(&dir, &["simulate", &dcv, "--uncut-over", "9"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        stderr(&out).contains("--uncut-over must be from 0.05 to 5 mm"),
        "{}",
        stderr(&out)
    );
    let out = run(
        &dir,
        &["simulate", &dcv, "--cell", "0.5", "--quality", "fine"],
    );
    assert_eq!(out.status.code(), Some(2), "a usage error");
}

/// Capability 120 through the command: the plaque as committed has no
/// uncut material; with its Square recess's corner relief turned off,
/// the four corners are listed under the Pocket, with the note on what
/// the grid cannot find.
#[test]
fn uncut_material_is_listed_by_operation() {
    let dir = work_dir("simulate_uncut");
    let text = std::fs::read_to_string(project("plaque.dcv")).expect("the plaque");
    let plain = text.replacen(
        "\"corner_relief\": \"dogbone\"",
        "\"corner_relief\": \"none\"",
        1,
    );
    assert_ne!(plain, text);
    std::fs::write(dir.join("plain.dcv"), plain).expect("written");
    let out = run(&dir, &["simulate", "plain.dcv"]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let report = stdout(&out);
    assert!(
        report.contains("Uncut material, more than 0.300 mm above what the shapes ask for:"),
        "{report}"
    );
    let line = report
        .lines()
        .find(|l| l.contains("`Square recess` (op2): 4 places, "))
        .unwrap_or_else(|| panic!("no line for the recess: {report}"));
    assert!(line.ends_with("up to 6.000 mm"), "{line}");
    assert!(
        report.contains("A place narrower than about two cells (0.332 mm) is not found."),
        "{report}"
    );
}
