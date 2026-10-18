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
2. **Rapids only at safe Z.** Every rapid (`G0`) move is at or above safe Z.
   The first move of the program is a rapid straight up to safe Z.
3. **No invented feeds.** Every cutting move carries a feed rate you set on
   the tool: the plunge rate for straight-down moves and tab steps, the feed
   rate for everything else. A tool without a feed rate or plunge rate
   stops the export, and so does one without a spindle speed unless the
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
   tool change (`M6`, grblHAL only), comes with the bit at safe Z. You may
   jog and set Z zero at a tool change, so after any stop the program
   rises to safe Z again before the spindle or router starts, and writes
   its next motion and feed words in full. GRBL cannot stop for a tool
   change where you can jog, so DreamCarve refuses a GRBL program that
   needs a second bit.
6. **The controller is in a known state.** The program sets units, the
   work coordinate system and absolute coordinates (`G90`) before it moves,
   and again after every pause and tool change, where you may have changed
   them.
7. **No mirroring.** The program's coordinates are the project's, moved to
   your work zero. Nothing is flipped.

It also checks that:

- every line is plain ASCII and short enough for the controller's buffer,
  with no character the controller would act on as a command;
- every arc is one the controller will accept once its numbers are rounded
  (an arc that would not be is written as short straight moves, with a
  warning);
- the program's motion is the toolpath DreamCarve computed, move for move.

## What DreamCarve cannot check

- **Clamps and fixtures.** DreamCarve has no idea where they are. The bit
  will cut through anything in its path, at any height below safe Z.
- **The stock.** It trusts the size and thickness in the project. If your
  stock is thinner than `thickness_mm`, the cut goes deeper into the
  spoilboard than you planned. If it is thicker, the part is not cut free.
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
- **The router, in manual mode.** It cannot tell whether you switched it on.
- **Pause messages.** Your sender may not show why the program paused. Use
  the pause list `export` prints.

## Before every cut

1. **Read the export output.** Check the operations, their depths, and the
   list of pauses.
2. **Check the stock.** Measure its thickness and compare it with
   `thickness_mm`. Make sure the stock covers the area from work zero to
   `width_mm` by `height_mm`.
3. **Check the clamps.** Keep them outside the area the bit can reach, or
   above safe Z with room to spare.
4. **Use a spoilboard** under any cut that goes through.
5. **Check the bit.** It matches `diameter_mm`, and it is tight in the
   collet.
6. **Fit the first bit, then set work zero** with it where `origin` says,
   and check it by jogging there. `export` names the first bit
   ("Before you start: fit T1 ...").
7. **Do an air cut.** Set Z zero well above the stock: more than the cut
   depth plus a margin, for example 20 mm for a 12 mm cut. Run the whole
   program and watch where the bit goes. Then set Z zero back on the stock.
8. **Stay at the machine** with the emergency stop or feed hold in reach.
   Wear eye and hearing protection, and keep dust extraction running.

Phase 1's first test part, the coaster in the
[quick start](../quick-start/first-cut.md), has not yet been cut on a real
machine by the project. Until it has, treat every program as untested and
watch the air cut closely.
