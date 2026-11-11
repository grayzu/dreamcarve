# Copyright 2026 The DreamCarve Contributors
# SPDX-License-Identifier: Apache-2.0

"""The capture tool's argument check (#647).

    python3 -I tools/parity/photo/test_capture.py

Loads `capture.py` beside it by path (with `-I` the script's directory is
not on `sys.path`) and runs nothing of RasterCarve's.
"""

import contextlib
import importlib.util
import io
import sys
import unittest
from pathlib import Path
from unittest import mock

sys.dont_write_bytecode = True  # no __pycache__ beside capture.py
HERE = Path(__file__).resolve().parent
SPEC = importlib.util.spec_from_file_location("capture", HERE / "capture.py")
capture = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(capture)


class CaseNameIsCheckedFirst(unittest.TestCase):
    def refused(self, case):
        """Runs `main` with `case`, failing if anything past the argument
        check (a path, a subprocess) is reached; returns the exit code."""
        stderr = io.StringIO()
        with mock.patch.object(capture, "run", side_effect=AssertionError("ran")), \
                mock.patch.object(capture, "git", side_effect=AssertionError("git")), \
                mock.patch.object(capture.Path, "mkdir",
                                  side_effect=AssertionError("mkdir")), \
                contextlib.redirect_stderr(stderr), \
                self.assertRaises(SystemExit) as exit_:
            capture.main(["--rastercarve", "/nonexistent", "--case", case])
        self.assertIn("--case", stderr.getvalue())
        return exit_.exception.code

    def test_a_case_with_a_path_is_refused_before_any_path_is_built(self):
        for case in ["../x", "../../etc", "a/b", "/tmp/x", ".", ""]:
            with self.subTest(case=case):
                self.assertEqual(self.refused(case), 2)

    def test_a_case_with_capitals_or_spaces_is_refused(self):
        for case in ["Photo-Raster-Inch", "photo raster", "photo_raster"]:
            with self.subTest(case=case):
                self.assertEqual(self.refused(case), 2)

    def test_an_unknown_case_is_refused(self):
        self.assertEqual(self.refused("photo-hatch"), 2)

    def test_the_known_cases_pass_the_check(self):
        for case in ["photo-raster-inch", "photo-raster-metric"]:
            with self.subTest(case=case):
                self.assertEqual(capture.case_name(case), case)


if __name__ == "__main__":
    unittest.main()
