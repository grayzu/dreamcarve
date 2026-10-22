# Safety

A G-code program drives a real machine with a sharp bit turning at high
speed. DreamCarve checks every program it writes, but it only knows what the
project file tells it. It cannot see your machine, your stock, or your
clamps. This chapter says what it checks, what it cannot, and what to check
yourself before every cut.

## What DreamCarve checks

After writing a program, `export` reads the text back the way the controller
will, and checks it. If any check fails, the export fails and nothing is
written. There are no safety warnings, only errors.

1. **Never below the stock bottom.** No move goes lower than the stock
   bottom, minus the operation's `through_cut_allowance_mm` (0 unless you
   set it). A depth deeper than that is refused before any toolpath is made.
   So is a stock without `thickness_mm` or an operation without
   `depth_mm`: DreamCarve has no defaults for them.
2. **Rapids only at safe Z.** Every rapid (`G0`) move is at or above safe Z.
   The first move of the program is a rapid straight up to safe Z. There
   are two exceptions, both straight down and only into air. Above a ramp
   or helix [entry](pocket.md#entry), and above every straight plunge (a
   `plunge` entry, an [Engrave](engrave.md) pass, a corner-relief notch),
   a rapid straight down from safe Z stops 1 mm above the stock top, or
   1 mm above the floor the same operation has already cut at that point;
   the ramp, helix or plunge at the plunge rate starts there, not at safe
   Z. The check refuses a rapid that stops lower, or over a floor only
   another operation cut. With a [clearance height](post-processors.md#heights-clearance-tool-change-and-park)
   set, moves between parts and between operations rise to it, which is
   higher still: the check holds each operation's first move across the
   work, every move across outside the operations (such as the move to
   the park point) and every stop to it. Without one, moves after the last
   operation are held to the highest safe Z of the program. Inside a [Drill](drill.md), a rapid straight
   down may stop at the retract height above the stock, or inside the
   part of the hole already drilled, and never in uncut material.
3. **No invented feeds.** Every cutting move carries a feed rate you set on
   the tool: the plunge rate for straight-down moves and tab steps, the feed
   rate for everything else. The one value in between is a move that goes
   down while it moves sideways, a ramp or helix [entry](pocket.md#entry)
   or a smooth [tab](tabs.md)'s ramp down: its feed rate is lowered just
   enough that it goes down no faster than the plunge rate (rule 11). A
   feed rate that would go down faster once rounded to the decimal places
   written is rounded down instead: a plunge rate the written places cannot
   hold, such as 1000 mm/min in inches (`F39.3700`, not `F39.3701`), is
   written one unit in the last place lower. A
   tool without a feed rate or
   plunge rate stops the export, and so does one without a spindle speed unless the
   router is switched by hand. DreamCarve has no defaults for these.
4. **The spindle runs before it cuts.** With automatic spindle control, the
   spindle starts at the tool's RPM and the program waits for it to reach
   speed before the first cut, and stops it at the end. With
   [manual spindle control](post-processors.md#manual-spindle-control), the
   program pauses at safe Z before each tool's first cut for you to turn
   the router on, and pauses again for you to turn it off after the tool's
   last cut.
5. **Stops happen at safe Z, and the program lifts again after them.**
   The program does not stop for its first bit: you fit it and set work
   zero with it before you start. Every later stop, a pause (`M0`) or a
   tool change (`M6`, only in one program with `M6`), comes with the bit
   at safe Z. You may
   jog and set Z zero at any stop, so after it the program rises to safe Z
   again before the spindle starts, before the next stop, and before any
   move across the work, and writes its next motion and feed words in
   full. The safety check treats Z as unknown after every stop, so a
   program that moves sideways before it lifts is refused.
6. **One bit per program, unless you choose otherwise.** A project with
   more than one bit exports, by default, one program per tool run, and
   no program changes the bit. Whether an `M6` tool change stops the
   machine depends on your sender, and several do not stop by default, so
   a program with `M6` could cut the next bit's paths with the old bit
   still fitted. Each program passes every check here on its own, and if
   any fails, none is written. One program with `M6` is a setting you
   choose, on grblHAL only, after setting your sender to stop at `M6`
   ([Senders](post-processors.md#senders)). GRBL cannot stop for a tool
   change where you can jog, so DreamCarve refuses `M6` with GRBL.
7. **The controller is in a known state.** The program sets units, the
   work coordinate system and absolute coordinates (`G90`) before it moves,
   and again after every pause and tool change, where you may have changed
   them.
8. **No mirroring, and nothing else.** The program's coordinates are the
   project's, moved to your work zero. Nothing is flipped. Read back, each
   operation's moves are its toolpath, move for move: every straight move
   ends where the toolpath's does, every arc turns the same way about the
   same centre, and there are no moves in between except a rapid split
   into its up-or-down and sideways parts, an arc written as short
   straight moves along it, and a lift to safe Z, or to the clearance
   height, before or after the operation. An arc written in place of a run of straight moves
   ([Arc fitting](project-file.md#arc-fitting)) is accepted only if it
   starts and ends on the toolpath's corners, every corner between lies
   on it within `arc_fit_tolerance_mm`, it stays that close to every
   straight move it replaces along the move's whole length (so the four
   corners of a square, which lie on one circle, never become a circle),
   it sweeps at most 90°, and it turns the way the corners do.
9. **The cut stays on the stock.** Every cutting move, widened by the
   bit's radius, stays inside the stock's `width_mm` by `height_mm`
   rectangle; arcs count all the way round, not just at their ends. A shape
   moved or scaled off the stock stops the export with an error that names
   the operation and how far past which edge the bit goes, for example
   ``operation `op1` cuts off the stock: 71.350 mm past the +X (right)
   edge``. A cut that runs exactly along an edge is allowed. Toolpath
   generation checks this first, and the read-back program is checked
   again.

10. **Canned cycles mean what the plain moves mean.** When a Drill is
    written as [canned cycles](drill.md#canned-cycles) (`G81`, `G82`,
    `G83`, `G73`), each cycle line is read back as the moves the
    controller makes for it, and rules 1, 2, 8 and 9 are checked on those
    moves: they must be the Drill's own moves, hole for hole, depth for
    depth. A cycle the check cannot read back exactly, such as one with a
    word missing, is refused, and so is any other move before the `G80`
    that ends the cycles.
11. **Nothing goes down faster than the plunge rate.** For every cutting
    move that goes down, the check works out its slope from the numbers
    written in the program, and its speed straight down: the feed rate
    times the sine of the slope. That must not be faster than the tool's
    plunge rate. This holds for a ramp, a helix, a smooth tab's ramp down,
    and a straight plunge, whose speed straight down is its feed rate.
    Nothing is allowed for rounding: DreamCarve rounds such a move's feed
    rate down, by as many units in the last decimal place as its rounded
    coordinates need, so the move as written goes down no faster than the
    plunge rate. A move that goes down too fast stops the export with an
    error that names the line and the operation, for example ``emitted
    line 42 of operation `op2` descends faster than its tool's plunge
    rate``.
12. **Each operation cuts only its own material.** The check works out
    what each operation may cut from the project's shapes, not from the
    toolpath, and holds every cutting move to it:

    - a [Pocket](pocket.md): the whole width of the bit stays inside the
      pocket's outline and outside its islands, `allowance_mm` away from
      both, arcs counted all the way round. Entry ramps and helixes count
      too. A rest machining Pocket is held to its own outline the same
      way.
    - a [Drill](drill.md): the bit stays inside one of the circles it
      drills.
    - an [Engrave](engrave.md): the centre of the bit stays on the line.
    - a [Profile](profile.md): the whole width of the bit stays in the
      waste. Cut **outside**, that means outside every part, and cut
      **inside**, inside the hole and off any island left in it. Cut **on
      line**, the centre of the bit stays on the line, at least a bit's
      width from the part's other lines, so a cut that would cut through
      a ring narrower than the bit stops the export. On an open path in a
      part, the bit's centre stays on the path and a bit's width from the
      part's lines and its other open paths, except near a point where the
      two meet. A cut also keeps a bit's width from the stretches of its
      own line farther along it, so a cut along a U, a tight spiral or a
      narrow neck stops the export, and so does any cut round a part
      narrower than the bit everywhere, which the cut would remove whole,
      as DreamCarve's own check (see
      [Bands narrower than the bit](profile.md#bands-narrower-than-the-bit)). Laps, tab rises,
      entry ramps and helixes, lead arcs and corner relief notches all
      count, so a ramp or lead arc that swings into a part, its own or a
      neighbour's, stops the export. A lap that runs along one outline at
      the bit's radius passes even where another, separate shape is
      closer than the bit is wide: that gouge is the `toolpaths_overlap`
      warning. It never passes closer than the radius to another line of
      its own shape, such as an island in a band narrower than the bit;
      DreamCarve refuses that cut before export (see
      [Bands narrower than the bit](profile.md#bands-narrower-than-the-bit)).
      A Profile also leaves its [tabs](tabs.md) standing: a straight plunge
      on the bit's path, below a tab's top, within the stretch a pass rises
      over for that tab (its width plus the bit's diameter, along the path;
      under a smooth tab's slope, below the slope), stops the export. The
      tabs are the project's placed tabs and those a tab request places.

    A move may miss by as much as the program may already differ from the
    toolpath: one unit in the last decimal place written, plus
    `flatten_tolerance_mm`, plus `arc_fit_tolerance_mm` when arc fitting
    is on. A cut outside stops the export with an error that names the
    line and the operation, for example ``emitted line 57 takes operation
    `op2`'s tool outside the material it may cut``.

13. **Machine coordinates only for the tool-change lift.** A program
    writes `G53` only as `G53 G0 Z<v>`, alone on its line, where `<v>` is
    the project's `tool_change_machine_z_mm` (below 0), on a
    post-processor that documents `G53` (grblHAL, GRBL, LinuxCNC), and only right
    before a tool-change stop or the end of the program, from at or above
    safe Z, at the spot the bit is already over, or again right after a
    stop that followed a lift (the router-off pause, then the tool change)
    with nothing moved between. Any other `G53` is refused. After the lift the check treats Z as unknown, as after a
    stop (rule 5), so a move across the work needs a lift to safe Z in
    work coordinates first; a program that moves before the stop or the
    end it lifted for is refused.

It also checks that:

- every line is plain ASCII and short enough for the controller's buffer,
  with no character the controller would act on as a command;
- every arc is one the controller will accept once its numbers are rounded
  (an arc that would not be is written as short straight moves, with a
  warning).

## What DreamCarve cannot check

- **Clamps and fixtures.** DreamCarve has no idea where they are. The bit
  will cut through anything in its path, at any height below safe Z.
  `export` and `check` print where the bit goes (see "Before every cut"),
  but keeping clamps out of that area is up to you.
- **The stock.** It trusts the size and thickness in the project. It has no
  default thickness: `init` leaves `thickness_mm` and the operation's
  `depth_mm` unset, and export refuses until you enter them, but it cannot
  tell whether what you entered is right. With the default
  `origin.z: "stock_top"`, if your stock is thinner than `thickness_mm`,
  the cut goes deeper into the spoilboard than you planned. If it is
  thicker, the part is not cut free.
  With `origin.z: "stock_bottom"`, a `thickness_mm` entered too thin is
  worse: safe Z is `thickness_mm` plus `safe_z_mm` above the spoilboard,
  not above the real stock top. If the stock is thicker than that, every
  rapid at safe Z drives the bit sideways through the stock; if it is
  thicker by less, the bit clears the stock by less than `safe_z_mm`.
- **Work zero.** It trusts that you set X, Y and Z zero where `origin` says.
  A Z zero set at the spoilboard instead of the stock top puts every pass
  12 mm deeper than intended.
- **The tool.** It trusts that the bit in the spindle is the one in the
  project, with that diameter, and that you set Z zero with that bit.
- **Whether the feeds suit the job.** It uses your values exactly. It cannot
  tell whether they suit your machine, bit, and material.
- **Safe Z height.** It keeps rapids at safe Z, but cannot tell whether safe
  Z clears your clamps.
- **Machine limits.** It does not know your machine's travel. Use homing and
  soft limits on the controller.
- **Homing, with a tool-change machine Z.** A `G53` lift goes to a machine
  position, which is right only if the machine was homed since it was
  switched on. `export` reminds you, but cannot tell. Nor can it tell
  whether the machine Z you chose is above the clearance height on your
  machine, so that the lift only goes up. The 6000 mm limit on project coordinates
  only catches typing mistakes; it is not your machine's size.
- **The router, in manual mode.** It cannot tell whether you switched it on.
- **Pause messages.** Your sender may not show why the program paused. Use
  the pause list `export` prints.
- **Your sender, with one program with `M6`.** Whether the program stops at
  a tool change depends on your sender's setting, which DreamCarve cannot
  read ([Senders](post-processors.md#senders)). Nor can it tell that you
  run separate programs in the order listed, with the right bit in each.

## Before every cut

1. **Read the export output.** Check the operations, their depths, the
   extents, and the list of pauses. The extents line gives the range of
   every axis in machine coordinates, the numbers your controller shows
   once work zero is set: `X min..max, Y min..max` for the centre of the
   bit and `Z min..max`, where Z min is the deepest cut and Z max is safe Z.
   The line after it widens X and Y by the bit's radius (the largest bit's,
   if there are several): that is where the bit's edge reaches.
2. **Check the stock.** Measure its thickness and compare it with
   `thickness_mm`. Make sure the stock covers the area from work zero to
   `width_mm` by `height_mm`.
3. **Check the clamps.** Keep them outside the area the bit's edge
   reaches, from the printed extents, with room to spare for the collet
   nut. Rapids cross that area at safe Z (Z max), so anything left inside
   it must be well below safe Z.
4. **Use a spoilboard** under any cut that goes through.
5. **Check the bit.** It matches `diameter_mm`, and it is tight in the
   collet.
6. **Fit the first bit, then set work zero** with it where `origin` says,
   and check it by jogging there. `export` names the first bit
   ("Before you start: fit T1 ..."). With more than one bit, run the
   programs in the order `export` lists them. Before each later one, fit
   the bit it names and set Z zero with it; keep X and Y zero.
7. **Do an air cut.** Set Z zero well above the stock: more than the cut
   depth plus a margin, for example 20 mm for a 12 mm cut. Run the whole
   program and watch where the bit goes. Then set Z zero back on the stock.
8. **Stay at the machine** with the emergency stop or feed hold in reach.
   Wear eye and hearing protection, and keep dust extraction running.

Neither test part, the coaster in the
[first quick start](../quick-start/first-cut.md) nor the plaque in the
[second](../quick-start/plaque.md), has yet been cut on a real machine by
the project. Until they have, treat every program as untested and watch
the air cut closely.
