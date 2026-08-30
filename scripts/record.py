#!/usr/bin/env python3
"""Terminal-driven screen recording on GNOME/Wayland, for the demo captures.

GNOME's org.gnome.Shell.Screencast ties the recording to the *calling D-Bus
connection*: a one-shot `gdbus call` returns immediately, the connection drops,
and the screencast stops after a single frame. So this holds the connection
open for the duration, which is what makes it scriptable.

  scripts/record.py out.webm --secs 20            # fixed length
  scripts/record.py out.webm --until-file /tmp/x  # stop when that file appears
  scripts/record.py out.webm --area 0,0,1920,1080 # a region instead of the screen

Records video only (no audio) -- the voiceover is laid on afterwards.
"""
from __future__ import annotations
import argparse, os, sys, time
import gi
gi.require_version("Gio", "2.0")
from gi.repository import Gio, GLib


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("out")
    ap.add_argument("--secs", type=float, default=0, help="record for this long")
    ap.add_argument("--until-file", help="stop as soon as this path exists")
    ap.add_argument("--max-secs", type=float, default=900)
    ap.add_argument("--fps", type=int, default=30)
    ap.add_argument("--area", help="x,y,w,h -- record a region instead of the whole screen")
    ap.add_argument("--no-cursor", dest="cursor", action="store_false", default=True,
                    help="omit the pointer -- wanted for slide cards, harmless for terminals")
    a = ap.parse_args()

    out = os.path.abspath(os.path.expanduser(a.out))
    os.makedirs(os.path.dirname(out), exist_ok=True)
    for stale in (out,):
        if os.path.exists(stale):
            os.remove(stale)

    bus = Gio.bus_get_sync(Gio.BusType.SESSION, None)
    proxy = Gio.DBusProxy.new_sync(
        bus, Gio.DBusProxyFlags.NONE, None,
        "org.gnome.Shell.Screencast", "/org/gnome/Shell/Screencast",
        "org.gnome.Shell.Screencast", None)

    # a plain dict of Variants -- GLib.Variant("(sa{sv})", ...) builds the
    # dictionary itself and chokes on an already-constructed a{sv}.
    opts = {
        "framerate": GLib.Variant("i", a.fps),
        "draw-cursor": GLib.Variant("b", bool(a.cursor)),
    }

    if a.area:
        x, y, w, h = (int(v) for v in a.area.split(","))
        ok, used = proxy.call_sync(
            "ScreencastArea",
            GLib.Variant("(iiiisa{sv})", (x, y, w, h, out, opts)),
            Gio.DBusCallFlags.NONE, -1, None).unpack()
    else:
        ok, used = proxy.call_sync(
            "Screencast", GLib.Variant("(sa{sv})", (out, opts)),
            Gio.DBusCallFlags.NONE, -1, None).unpack()

    if not ok:
        print("ERROR: gnome-shell refused to start the screencast", file=sys.stderr)
        return 1
    print(f"RECORDING -> {used}", flush=True)

    t0 = time.time()
    try:
        while True:
            elapsed = time.time() - t0
            if a.secs and elapsed >= a.secs:
                break
            if a.until_file and os.path.exists(a.until_file):
                break
            if elapsed >= a.max_secs:
                break
            time.sleep(0.2)
    except KeyboardInterrupt:
        pass

    proxy.call_sync("StopScreencast", None, Gio.DBusCallFlags.NONE, -1, None)
    # gnome-shell finalises the container after the call returns
    for _ in range(40):
        time.sleep(0.25)
        if os.path.exists(used) and os.path.getsize(used) > 0:
            break
    print(f"STOPPED after {time.time()-t0:.1f}s -> {used} "
          f"({os.path.getsize(used)/1e6:.1f} MB)", flush=True)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
