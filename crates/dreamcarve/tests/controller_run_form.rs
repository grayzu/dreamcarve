// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The controller-run issue form (#330): it offers every bundled
//! controller, and the end-to-end plan and the post-processor checklist
//! link it.

#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "test helpers fail the test on a broken setup"
)]

use std::path::{Path, PathBuf};

const FORM: &str = ".github/ISSUE_TEMPLATE/controller-run.yml";
const FORM_URL: &str =
    "https://github.com/grayzu/dreamcarve/issues/new?template=controller-run.yml";

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The file at `relative`, with `\r\n` read as `\n`: a Windows checkout
/// may give the form CRLF line ends.
fn read(relative: &str) -> String {
    let path = repo_root().join(relative);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{}: {e}", path.display()))
        .replace("\r\n", "\n")
}

fn indent(line: &str) -> usize {
    line.len() - line.trim_start().len()
}

/// The options of the form element whose `id` is `id`: the `- ` items
/// under its `options:` key, read line by line. The form is plain block
/// YAML with one option per line, which keeps this reader honest; a quoted
/// option has its quotes removed.
fn options_of(form: &str, id: &str) -> Vec<String> {
    let id_line = format!("id: {id}");
    let mut lines = form.lines().skip_while(|l| l.trim() != id_line);
    let element_indent = indent(lines.next().expect("an element with the id"));
    let mut element = lines.take_while(|l| l.trim().is_empty() || indent(l) >= element_indent);
    let options_indent = indent(
        element
            .by_ref()
            .find(|l| l.trim() == "options:")
            .expect("the element has options"),
    );
    element
        .take_while(|l| indent(l) > options_indent)
        .filter_map(|l| l.trim_start().strip_prefix("- "))
        .map(|option| option.trim().trim_matches('"').to_owned())
        .collect()
}

#[test]
fn controller_run_form_lists_every_bundled_controller() {
    let form = read(FORM);
    assert!(
        form.contains("  - type: dropdown\n    id: controller\n"),
        "{FORM}'s controller element is not a dropdown"
    );
    let listed = options_of(&form, "controller");
    let bundled: Vec<String> = dc_project::latest_definitions()
        .expect("bundled definitions parse")
        .iter()
        .map(|d| d.id.to_owned())
        .collect();
    assert_eq!(listed, bundled, "{FORM}'s controller dropdown");
}

#[test]
fn plan_links_the_controller_run_form() {
    for doc in [
        "docs/testing/post-processor-e2e.md",
        "docs/book/src/contributing/post-processor-checklist.md",
    ] {
        assert!(
            read(doc).contains(FORM_URL),
            "{doc} does not link {FORM_URL}"
        );
    }
}
