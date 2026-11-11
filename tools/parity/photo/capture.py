# Copyright 2026 The DreamCarve Contributors
# SPDX-License-Identifier: Apache-2.0

"""Capture a RasterCarve golden case into tests/golden/<case>/ (#647).

    python3 -I capture.py --rastercarve <checkout> --case photo-raster-inch

RasterCarve is GPLv2+. This script runs it as an external program and
records what its command line writes: it never imports anything from the
checkout, never adds it to this script's own sys.path, and copies none of
its source (security review M5). It runs the checkout's interpreter
(`<checkout>/.venv/bin/python`, or `--python`) by absolute path with an
argument list, no shell, as `python -m rastercarve <args> <picture>`, from
a fresh temporary working directory holding a copy of the case's picture.
The child's PYTHONPATH is the checkout, so the code that runs is the
checkout's at the commit the README records, installed in that interpreter
or not.

It writes only `program.nc`, RasterCarve's standard output byte for byte,
and `README.md`: the command line, RasterCarve's version and commit, its
statistics block, the frame mapping and the case's notes
(`<case>.notes.md` beside this script). Run it twice: the output must be
byte-identical. `README.md` beside this script says more.
"""

import argparse
import os
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO_ROOT = HERE.parent.parent.parent
FIXTURES = REPO_ROOT / "tests" / "fixtures" / "photo"
GOLDEN = REPO_ROOT / "tests" / "golden"

CASE_NAME = re.compile(r"^[a-z0-9-]+$")

# Run by the capture's interpreter with `-I` (no checkout on its path):
# the Python, OpenCV and NumPy versions, one line.
DEPENDENCY_VERSIONS = (
    "import platform, cv2, numpy; "
    "print(f'{platform.python_version()}, OpenCV {cv2.__version__}, "
    "NumPy {numpy.__version__}')"
)

# Each case: its picture in tests/fixtures/photo/, RasterCarve's arguments
# (before the picture), and the scale from RasterCarve's units to mm.
CASES = {
    "photo-raster-inch": {
        "picture": "ramp.png",
        "args": ["--width", "0.1"],
        "units": "in",
    },
    "photo-raster-metric": {
        "picture": "radial.png",
        "args": [
            "--metric", "--width", "100", "-t", "30", "-d", "2", "-a", "22.5",
            "-s", "110", "-r", "0.25", "-z", "5", "-f", "1000", "-p", "300",
        ],
        "units": "mm",
    },
}


def case_name(text):
    """An argparse type: a case name, checked before any path is built."""
    if not CASE_NAME.match(text):
        raise argparse.ArgumentTypeError(
            f"case {text!r} must match {CASE_NAME.pattern}")
    if text not in CASES:
        raise argparse.ArgumentTypeError(
            f"unknown case {text!r}; known: {', '.join(sorted(CASES))}")
    return text


def parse_args(argv):
    parser = argparse.ArgumentParser(
        description="Capture a RasterCarve golden case (#647).")
    parser.add_argument("--rastercarve", required=True, type=Path,
                        help="the RasterCarve checkout")
    parser.add_argument("--case", required=True, type=case_name,
                        help="the case: " + ", ".join(sorted(CASES)))
    parser.add_argument("--python", type=Path,
                        help="the interpreter to run RasterCarve with "
                             "[default: <checkout>/.venv/bin/python]")
    return parser.parse_args(argv)


def run(command, cwd, env):
    """Runs `command` (a list, no shell) and returns its completed process."""
    return subprocess.run(command, cwd=cwd, env=env, capture_output=True,
                          check=False)


def git(checkout, *args):
    out = subprocess.run(["git", "-C", str(checkout), *args],
                         capture_output=True, text=True, check=True)
    return out.stdout.strip()


def statistics(stderr):
    """RasterCarve's statistics block from its standard error, without the
    progress bar (whose rates change run to run) or the debug count."""
    lines = re.split(r"[\r\n]+", stderr)
    try:
        start = lines.index("=== Statistics ===")
    except ValueError:
        sys.exit("RasterCarve printed no statistics block")
    block = []
    for line in lines[start:]:
        if not line.strip() or "suppressed debug message" in line:
            break
        block.append(line.rstrip())
    return block


def readme(case, command, version, commit, deps, stats, notes):
    spec = CASES[case]
    scale = "× 25.4 (RasterCarve writes inches, `G20`)" if spec["units"] == "in" \
        else "× 1 (RasterCarve writes millimetres, `G21`)"
    text = f"""<!--
Copyright 2026 The DreamCarve Contributors
SPDX-License-Identifier: Apache-2.0
-->
# Golden case: {case}

Captured from RasterCarve {version}, commit `{commit}`, with
`tools/parity/photo/capture.py --case {case}`
(the checkout's `rastercarve/` package, the command-line program, clean at
that commit), run by Python {deps}.

- Picture: `tests/fixtures/photo/{spec["picture"]}` (synthetic; see
  `tests/fixtures/README.md`).
- Command line, run in an empty directory holding a copy of the picture:

  ```sh
  {" ".join(command)}
  ```

- `program.nc` is RasterCarve's standard output, byte for byte. It is the
  output of a GPLv2+ program on DreamCarve's own fixture picture: the GPL
  does not cover a program's output, and the file carries no RasterCarve
  source text (security review M5). Nothing else of RasterCarve's is in
  this repository.
- DreamCarve project: `dreamcarve.dcv`, written by hand (the capture never
  writes it).

## RasterCarve's statistics

```text
{chr(10).join(stats)}
```

## Frame

RasterCarve's origin is the picture's top-left corner with X to the right
and Y up, so the picture lies at Y ≤ 0 (its rows run down to
`Y = −height`), and Z is zero at the stock top. DreamCarve is Y-up with
the picture's bottom-left corner at (`x_mm`, `y_mm`) (ADR-0001), so, with
`k` the scale {scale}:

- `x_dc = x_mm + k · x_rc`
- `y_dc = y_mm + height_mm + k · y_rc`
- `z_dc = thickness_mm + k · z_rc`

RasterCarve's Y is negative down the picture, so the sign is `+`; the
design spec's draft had `− y_rc`, corrected from the captured program
(#647).
"""
    if notes:
        text += "\n" + notes.rstrip() + "\n"
    return text


def main(argv):
    args = parse_args(argv)
    case = args.case
    spec = CASES[case]
    checkout = args.rastercarve.resolve()
    python = (args.python or checkout / ".venv" / "bin" / "python")
    python = Path(os.path.abspath(python))
    if not python.is_file():
        sys.exit(f"no interpreter at {python}; pass --python")
    picture = FIXTURES / spec["picture"]
    commit = git(checkout, "rev-parse", "HEAD")
    if git(checkout, "status", "--porcelain", "--", "rastercarve"):
        sys.exit("the checkout's rastercarve/ has uncommitted changes; "
                 "the README could not name the code that ran")

    command = ["python", "-m", "rastercarve", *spec["args"], spec["picture"]]
    plain_env = {"PATH": os.environ.get("PATH", ""), "PYTHONHASHSEED": "0",
                 "LC_ALL": "C"}
    env = {**plain_env, "PYTHONPATH": str(checkout)}
    with tempfile.TemporaryDirectory() as work:
        # The libraries RasterCarve's output depends on (OpenCV resamples
        # the picture), asked of the interpreter without the checkout.
        probe = run([str(python), "-I", "-c", DEPENDENCY_VERSIONS], work,
                    plain_env)
        deps = probe.stdout.decode().strip() if probe.returncode == 0 \
            else "with unknown library versions"
        shutil.copyfile(picture, Path(work) / spec["picture"])
        done = run([str(python), *command[1:]], work, env)
        if done.returncode != 0:
            sys.stderr.write(done.stderr.decode("utf-8", "replace"))
            sys.exit(f"RasterCarve failed ({done.returncode})")
        version = run([str(python), "-m", "rastercarve", "--version"], work,
                      env)
    program = done.stdout
    stats = statistics(done.stderr.decode("utf-8", "replace"))
    version_text = (version.stdout or version.stderr).decode().strip()
    version_text = version_text.split()[-1] if version_text else "unknown"
    notes_path = HERE / f"{case}.notes.md"
    notes = notes_path.read_text() if notes_path.is_file() else ""

    out = GOLDEN / case
    out.mkdir(parents=True, exist_ok=True)
    (out / "program.nc").write_bytes(program)
    (out / "README.md").write_text(
        readme(case, command, version_text, commit, deps, stats, notes))
    print(f"Wrote {out / 'program.nc'} and {out / 'README.md'}")


if __name__ == "__main__":
    main(sys.argv[1:])
