// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Writes the user guide's generated figures. Run from the repo root:
//! `cargo run -p doc-figures`. The `up_to_date` test fails when a committed
//! figure no longer matches what the engine produces.

use std::path::Path;

fn main() -> std::io::Result<()> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    for figure in doc_figures::all() {
        let path = root.join(figure.path);
        std::fs::write(&path, &figure.svg)?;
        std::io::Write::write_all(
            &mut std::io::stdout(),
            format!("wrote {}\n", figure.path).as_bytes(),
        )?;
    }
    Ok(())
}
