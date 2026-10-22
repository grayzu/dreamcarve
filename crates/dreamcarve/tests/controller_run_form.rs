// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The controller-run issue form (#330): it offers every bundled
//! controller, and the end-to-end plan and the post-processor checklist
//! link it.

use std::path::{Path, PathBuf};

const FORM: &str = ".github/ISSUE_TEMPLATE/controller-run.yml";
const FORM_URL: &str =
    "https://github.com/grayzu/dreamcarve/issues/new?template=controller-run.yml";

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read(relative: &str) -> String {
    let path = repo_root().join(relative);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn indent(line: &str) -> usize {
    line.len() - line.trim_start().len()
}

/// The options of the form element whose `id` is `id`: the `- ` items
/// under its `options:` key, read line by line. The form is plain block
/// YAML with one option per line, which keeps this reader honest; a quoted
/// option has its quotes removed.
fn options_of(form: &str, id: &str) -> Vec<String> {
    let lines: Vec<&str> = form.lines().collect();
    let at = lines
        .iter()
        .position(|l| l.trim() == format!("id: {id}"))
        .unwrap_or_else(|| panic!("no element with id {id}"));
    let element_indent = indent(lines[at]);
    let rest = &lines[at + 1..];
    let end = rest
        .iter()
        .position(|l| !l.trim().is_empty() && indent(l) < element_indent)
        .unwrap_or(rest.len());
    let element = &rest[..end];
    let options = element
        .iter()
        .position(|l| l.trim() == "options:")
        .unwrap_or_else(|| panic!("element {id} has no options"));
    let options_indent = indent(element[options]);
    element[options + 1..]
        .iter()
        .take_while(|l| indent(l) > options_indent && l.trim_start().starts_with("- "))
        .map(|l| l.trim_start()[2..].trim().trim_matches('"').to_owned())
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
