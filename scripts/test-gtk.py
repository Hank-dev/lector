#!/usr/bin/env python3
"""Run every GTK regression in its own process on a private Xvfb display."""

import argparse
import json
import os
from pathlib import Path
import shutil
import signal
import subprocess
import sys
import tempfile


def main():
    root = Path(__file__).resolve().parent.parent
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--filter", default="", help="Run test names containing this text")
    args, cargo_flags = parser.parse_known_args()
    for tool in ("cargo", "Xvfb", "xauth", "xvfb-run", "dbus-run-session"):
        if shutil.which(tool) is None:
            sys.exit(f"Missing {tool}; see CONTRIBUTING.md for test dependencies")
    cargo_flags = cargo_flags or ["--locked"]
    build = subprocess.run(
        ["cargo", "test", "--release", *cargo_flags, "--no-run", "--message-format=json"],
        cwd=root, check=True, text=True, stdout=subprocess.PIPE,
    )
    binaries = [
        event["executable"] for line in build.stdout.splitlines()
        if (event := json.loads(line)).get("reason") == "compiler-artifact"
        and event.get("profile", {}).get("test") and event.get("executable")
    ]
    if len(binaries) != 1:
        sys.exit(f"Expected one Lector test binary, found {len(binaries)}")
    binary = binaries[0]
    listing = subprocess.check_output(
        [binary, "--ignored", "--list"], text=True, cwd=root,
    )
    tests = [line.removesuffix(": test") for line in listing.splitlines()
             if line.startswith("window::") and line.endswith(": test")
             and args.filter in line]
    if not tests:
        sys.exit("No GTK tests found")
    failures = []
    with tempfile.TemporaryDirectory(prefix="lector-gtk-") as temporary:
        for index, test in enumerate(tests, 1):
            # Each test gets its own state, D-Bus session, and X server. No
            # real reading history, annotations, desktop display, or bus.
            state = Path(temporary) / str(index)
            env = os.environ.copy()
            for key in ("DISPLAY", "WAYLAND_DISPLAY", "DBUS_SESSION_BUS_ADDRESS",
                        "BROADWAY_DISPLAY"):
                env.pop(key, None)
            for key, folder in (("XDG_CONFIG_HOME", "config"),
                                ("XDG_CACHE_HOME", "cache"),
                                ("XDG_DATA_HOME", "data"),
                                ("XDG_RUNTIME_DIR", "runtime"),
                                ("TMPDIR", "tmp")):
                path = state / folder
                path.mkdir(parents=True, mode=0o700)
                env[key] = str(path)
            env.update(GDK_BACKEND="x11", GSK_RENDERER="cairo", GTK_A11Y="none",
                       LECTOR_TEST_ON_DESKTOP="1")
            print(f"[{index}/{len(tests)}] {test}", flush=True)
            process = subprocess.Popen(
                ["xvfb-run", "-a", "-s", "-screen 0 1280x1024x24 -nolisten tcp",
                 "dbus-run-session", "--", binary, test,
                 "--ignored", "--exact", "--test-threads=1"],
                cwd=root, env=env, start_new_session=True,
            )
            try:
                if process.wait(timeout=60):
                    failures.append(test)
            except subprocess.TimeoutExpired:
                failures.append(f"{test} (timeout)")
            finally:
                # Also clean up Xvfb, D-Bus, and a hung test on timeout or
                # interruption, not just the shell wrapper.
                try:
                    os.killpg(process.pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
                process.wait()
    print(f"GTK: {len(tests) - len(failures)}/{len(tests)} passed")
    for failure in failures:
        print(f"FAIL: {failure}")
    return bool(failures)


if __name__ == "__main__":
    sys.exit(main())
