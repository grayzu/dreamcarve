#!/usr/bin/env python3
# Copyright 2026 The DreamCarve Contributors
# SPDX-License-Identifier: Apache-2.0
#
# The headless "display" for `linuxcnc -r` (issue #328). LinuxCNC starts
# it as `dreamcarve-ui.py -ini <file>` in place of a GUI. It imports
# nothing from LinuxCNC: it uses only Python's standard library, and
# drives the machine through two separate LinuxCNC programs, each through
# its documented interface:
#
# - linuxcncrsh, LinuxCNC's text-mode command server, which it starts
#   and talks to over TCP (linuxcncrsh(1),
#   https://linuxcnc.org/docs/2.9/html/man/man1/linuxcncrsh.1.html):
#   hello, set enable, estop, machine, mode, home, feed_override,
#   linear_unit_conversion, open, run, resume, abort, and get
#   program_status, program_line, abs_act_pos, joint_homed, error,
#   operator_display;
# - halcmd, for HAL pins (halcmd(1),
#   https://linuxcnc.org/docs/2.9/html/man/man1/halcmd.1.html): getp of
#   motion.current-vel and motion.in-position, and setp/getp of
#   motion-logger's reopen-log pin (motion-logger(1)).
#
# It runs each program named in jobs.txt and writes what happened to
# result.txt for `tools/linuxcnc-sim` to read.
#
# The [DREAMCARVE] section of the INI file sets:
#   HOME = 1        home every joint first (0: leave the machine unhomed)
#   LOGGER = 1      motion-logger stands in for the motion module: rename
#                   its log to <program>.log after each program
#   FEED_OVERRIDE   feed override, percent (up to [DISPLAY]
#                   MAX_FEED_OVERRIDE, a factor)
#   TIMEOUT         seconds one program may take before it is aborted
#   PORT            the TCP port for linuxcncrsh
#
# result.txt has one tab-separated record per line:
#   HOMED    1|0
#   PROGRAM  <file>
#   PAUSE    <program_line> <x> <y> <z>
#   MESSAGE  <text>         an operator message, e.g. (MSG, ...)
#   ERROR    <text>         an error from task or motion, or a refused command
#   TIMEOUT
#   END      <program_line> <x> <y> <z>
#   DONE
# Positions are machine coordinates in millimetres. Every M0 pause is
# recorded once the machine has stopped moving, then resumed.

import os
import socket
import subprocess
import sys
import time

OUT = open("result.txt", "w")


def record(*fields):
    OUT.write("\t".join(str(f).replace("\t", " ").replace("\n", " ") for f in fields) + "\n")
    OUT.flush()


def read_ini(path):
    """Section -> key -> value, the last value of a repeated key winning."""
    sections, current = {}, None
    with open(path) as ini:
        for line in ini:
            line = line.strip()
            if not line or line.startswith(("#", ";")):
                continue
            if line.startswith("[") and line.endswith("]"):
                current = sections.setdefault(line[1:-1], {})
            elif current is not None and "=" in line:
                key, value = line.split("=", 1)
                current[key.strip()] = value.strip()
    return sections


INI_PATH = sys.argv[sys.argv.index("-ini") + 1]
INI = read_ini(INI_PATH)
SETTINGS = INI.get("DREAMCARVE", {})
HOME = SETTINGS.get("HOME", "1") == "1"
LOGGER = SETTINGS.get("LOGGER", "0") == "1"
FEED_OVERRIDE = SETTINGS.get("FEED_OVERRIDE", "10000")
TIMEOUT = float(SETTINGS.get("TIMEOUT", "300"))
PORT = int(SETTINGS.get("PORT", "5007"))
JOINTS = int(INI.get("KINS", {}).get("JOINTS", "3"))

SERVER = subprocess.Popen(["linuxcncrsh", "--port", str(PORT), "--", "-ini", INI_PATH])


def connect():
    deadline = time.time() + 60
    while True:
        try:
            return socket.create_connection(("127.0.0.1", PORT))
        except OSError:
            if time.time() > deadline:
                raise
            time.sleep(0.05)


LINK = connect().makefile("rwb", buffering=0)


def ask(request):
    """Sends one request and returns its reply, without the "\\r\\n"."""
    LINK.write((request + "\r\n").encode())
    return LINK.readline().decode(errors="replace").strip()


def get(name):
    """The value of `get <name>`: the reply less its leading name."""
    reply = ask("get " + name)
    head, _, value = reply.partition(" ")
    return value if head.upper() == name.split()[0].upper() else reply


def put(request):
    """`set <request>`; with verbose on the reply is SET <NAME> ACK or NAK."""
    reply = ask("set " + request)
    if not reply.upper().endswith("ACK"):
        record("ERROR", "set " + request + ": " + reply)
        return False
    return True


def halcmd(*args):
    done = subprocess.run(["halcmd", *args], capture_output=True, text=True)
    return done.stdout.strip()


SEEN = {"ERROR": "OK", "OPERATOR_DISPLAY": "OK", "OPERATOR_TEXT": "OK"}


def drain_messages():
    # linuxcncrsh returns the latest error or message, or OK when there is
    # none; record each new one once.
    for name, kind in (("error", "ERROR"), ("operator_display", "MESSAGE"), ("operator_text", "MESSAGE")):
        value = get(name)
        key = name.upper()
        if value.upper() != "OK" and value != SEEN[key]:
            record(kind, value)
        SEEN[key] = value


def machine_position():
    values = get("abs_act_pos").split()
    return [float(v) for v in values[:3]]


def wait_until_still(seconds):
    # The program can be idle, or paused, while the last segment still
    # blends into its end, so wait for the machine itself to stop: no
    # velocity, in position, and the same position for a whole second.
    # LinuxCNC's servo thread is not realtime here, and on a loaded host
    # it can stall for a moment, which a shorter window mistakes for a
    # stop. motion-logger moves nothing and has no motion pins.
    deadline = time.time() + seconds
    last, since = None, None
    while time.time() < deadline:
        here = machine_position()
        moving = here != last
        if not LOGGER and not moving:
            moving = halcmd("getp", "motion.in-position") != "TRUE" or float(
                halcmd("getp", "motion.current-vel") or "1"
            ) != 0.0
        if moving:
            since = None
        elif since is None:
            since = time.time()
        elif time.time() - since >= 1.0:
            return True
        last = here
        time.sleep(0.02)
    return False


def status():
    return get("program_status").upper()


def wait_for_status(wanted, seconds):
    deadline = time.time() + seconds
    while time.time() < deadline:
        if status() in wanted:
            return True
        time.sleep(0.01)
    return False


def reopen_log(name):
    # motion-logger closes and reopens its log when reopen-log goes true,
    # then clears it.
    halcmd("setp", "motion-logger.reopen-log", "1")
    deadline = time.time() + 30
    while halcmd("getp", "motion-logger.reopen-log") != "FALSE" and time.time() < deadline:
        time.sleep(0.01)
    os.rename("out.motion-logger", name)


if not ask("hello EMC dreamcarve 1.0").upper().startswith("HELLO ACK"):
    record("ERROR", "linuxcncrsh refused the handshake")
ask("set echo off")
ask("set verbose on")
put("enable EMCTOO")
put("set_wait done")
put("estop off")
put("machine on")
# Positions in mm whatever the program's units. linuxcncrsh refuses this
# before the machine is on.
put("linear_unit_conversion mm")
if HOME:
    put("mode manual")
    put("teleop_enable off")
    put("home -1")
    deadline = time.time() + 60
    while "NO" in get("joint_homed").split()[:JOINTS] and time.time() < deadline:
        time.sleep(0.05)
record("HOMED", int("NO" not in get("joint_homed").split()[:JOINTS]))
drain_messages()
if LOGGER:
    reopen_log("startup.log")

with open("jobs.txt") as jobs:
    programs = [line.strip() for line in jobs if line.strip()]

for program in programs:
    record("PROGRAM", program)
    put("set_wait done")
    if get("estop").upper() != "OFF" or get("machine").upper() != "ON":
        put("estop off")
        put("machine on")
    put("linear_unit_conversion mm")
    put("mode auto")
    put("feed_override " + FEED_OVERRIDE)
    put("open " + program)
    # With set_wait done, run would wait for the whole program, through
    # every pause; received only waits for task to take it.
    put("set_wait received")
    if put("run"):
        wait_for_status(("RUNNING", "PAUSED"), 10)
    deadline = time.time() + TIMEOUT
    while True:
        drain_messages()
        now = status()
        if now == "IDLE":
            break
        if time.time() > deadline:
            record("TIMEOUT")
            put("abort")
            wait_for_status(("IDLE",), 10)
            break
        if now == "PAUSED":
            wait_until_still(60)
            drain_messages()
            x, y, z = machine_position()
            record("PAUSE", get("program_line"), x, y, z)
            put("resume")
            wait_for_status(("RUNNING", "IDLE"), 30)
            continue
        time.sleep(0.005)
    wait_until_still(60)
    drain_messages()
    x, y, z = machine_position()
    record("END", get("program_line"), x, y, z)
    if LOGGER:
        reopen_log(program + ".log")

record("DONE")
OUT.close()
put("set_wait done")
ask("shutdown")
try:
    SERVER.wait(timeout=30)
except subprocess.TimeoutExpired:
    SERVER.kill()
