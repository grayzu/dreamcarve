#!/usr/bin/env python3
# Copyright 2026 The DreamCarve Contributors
# SPDX-License-Identifier: Apache-2.0
#
# The headless "display" for `linuxcnc -r` (issue #328). LinuxCNC starts
# it as `dreamcarve-ui.py -ini <file>` in place of a GUI. It drives the
# machine through LinuxCNC's documented Python interface
# (https://linuxcnc.org/docs/html/config/python-interface.html), runs
# each program named in jobs.txt, and writes what happened to result.txt
# for `tools/linuxcnc-sim` to read. It runs inside LinuxCNC's own process
# tree, never in DreamCarve.
#
# The [DREAMCARVE] section of the INI file sets:
#   HOME = 1        home every joint first (0: leave the machine unhomed)
#   LOGGER = 1      motion-logger stands in for the motion module: rename
#                   its log to <program>.log after each program
#   FEED_OVERRIDE   feed override, a factor (up to [DISPLAY] MAX_FEED_OVERRIDE)
#   TIMEOUT         seconds one program may take before it is aborted
#
# result.txt has one tab-separated record per line:
#   HOMED    1|0
#   PROGRAM  <file>
#   PAUSE    <current_line> <motion_line> <interp_paused 1|0> <x> <y> <z>
#   MESSAGE  <text>         an operator message, e.g. (MSG, ...)
#   ERROR    <text>         an NML or operator error
#   TIMEOUT
#   END      <rcs_state> <current_line> <motion_line> <x> <y> <z>
#   DONE
# Positions are machine coordinates in the INI's units. Every M0 pause is
# recorded once the machine has stopped moving, then resumed.

import os
import subprocess
import sys
import time

import hal
import linuxcnc

OUT = open("result.txt", "w")


def record(*fields):
    OUT.write("\t".join(str(f).replace("\t", " ").replace("\n", " ") for f in fields) + "\n")
    OUT.flush()


INI = linuxcnc.ini(sys.argv[sys.argv.index("-ini") + 1])


def setting(name, default):
    value = INI.find("DREAMCARVE", name)
    return default if value is None else value


HOME = setting("HOME", "1") == "1"
LOGGER = setting("LOGGER", "0") == "1"
FEED_OVERRIDE = float(setting("FEED_OVERRIDE", "1"))
TIMEOUT = float(setting("TIMEOUT", "300"))

if LOGGER:
    # motion-logger closes and reopens its log when reopen-log goes true,
    # then clears it.
    comp = hal.component("dreamcarve-ui")
    comp.newpin("reopen-log", hal.HAL_BIT, hal.HAL_IO)
    comp.ready()
    subprocess.run(
        ["halcmd", "net", "reopen-log", "dreamcarve-ui.reopen-log", "motion-logger.reopen-log"],
        check=True,
    )

command = linuxcnc.command()
stat = linuxcnc.stat()
errors = linuxcnc.error_channel()
ERROR_KINDS = (linuxcnc.NML_ERROR, linuxcnc.OPERATOR_ERROR)


def drain_errors():
    while True:
        message = errors.poll()
        if not message:
            return
        kind, text = message
        record("ERROR" if kind in ERROR_KINDS else "MESSAGE", text)


def wait_for(condition, seconds):
    deadline = time.time() + seconds
    while time.time() < deadline:
        stat.poll()
        if condition():
            return True
        time.sleep(0.005)
    return False


def machine_position():
    return stat.actual_position[:3]


def wait_until_still(seconds):
    # The queue can be empty and inpos set while the last segment still
    # blends into its end, so wait for the machine itself to stop: no
    # velocity, and the same position for a whole second. LinuxCNC's
    # servo thread is not realtime here, and on a loaded host it can stall
    # for a moment, which a shorter window mistakes for a stop.
    deadline = time.time() + seconds
    last, same = None, 0
    while time.time() < deadline:
        stat.poll()
        here = tuple(machine_position())
        still = stat.queue == 0 and stat.inpos and stat.current_vel == 0 and here == last
        same = same + 1 if still else 0
        if same >= 50:
            return True
        last = here
        time.sleep(0.02)
    return False


def make_ready():
    stat.poll()
    if stat.task_state != linuxcnc.STATE_ON:
        command.state(linuxcnc.STATE_ESTOP_RESET)
        command.wait_complete()
        command.state(linuxcnc.STATE_ON)
        command.wait_complete()
    command.mode(linuxcnc.MODE_AUTO)
    command.wait_complete()
    command.feedrate(FEED_OVERRIDE)
    command.wait_complete()


def reopen_log(name):
    comp["reopen-log"] = True
    while comp["reopen-log"]:
        time.sleep(0.01)
    os.rename("out.motion-logger", name)


command.state(linuxcnc.STATE_ESTOP_RESET)
command.wait_complete()
command.state(linuxcnc.STATE_ON)
command.wait_complete()
if HOME:
    command.mode(linuxcnc.MODE_MANUAL)
    command.wait_complete()
    command.teleop_enable(False)
    command.wait_complete()
    command.home(-1)
    command.wait_complete()
    wait_for(lambda: all(stat.homed[j] for j in range(stat.joints)), 60)
stat.poll()
record("HOMED", int(all(stat.homed[j] for j in range(stat.joints))))
drain_errors()
if LOGGER:
    reopen_log("startup.log")

with open("jobs.txt") as jobs:
    programs = [line.strip() for line in jobs if line.strip()]

for program in programs:
    record("PROGRAM", program)
    make_ready()
    command.program_open(program)
    command.wait_complete()
    command.auto(linuxcnc.AUTO_RUN, 0)
    command.wait_complete()
    wait_for(lambda: stat.interp_state != linuxcnc.INTERP_IDLE, 10)
    deadline = time.time() + TIMEOUT
    while True:
        stat.poll()
        drain_errors()
        if stat.interp_state == linuxcnc.INTERP_IDLE:
            break
        if time.time() > deadline:
            record("TIMEOUT")
            command.abort()
            command.wait_complete()
            wait_for(lambda: stat.interp_state == linuxcnc.INTERP_IDLE, 10)
            break
        if stat.task_paused:
            wait_until_still(60)
            x, y, z = machine_position()
            record(
                "PAUSE",
                stat.current_line,
                stat.motion_line,
                int(stat.interp_state == linuxcnc.INTERP_PAUSED),
                x,
                y,
                z,
            )
            command.auto(linuxcnc.AUTO_RESUME)
            command.wait_complete()
            wait_for(lambda: not stat.task_paused, 30)
            continue
        time.sleep(0.005)
    wait_until_still(60)
    stat.poll()
    drain_errors()
    x, y, z = machine_position()
    record("END", stat.state, stat.current_line, stat.motion_line, x, y, z)
    if LOGGER:
        reopen_log(program + ".log")

record("DONE")
OUT.close()
