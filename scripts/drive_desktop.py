#!/usr/bin/env python3
"""Drive a running workspace-desktop through its development automation socket.

Start the app with `--automation-socket <path>`, then:

    drive_desktop.py --socket <path> click 640 400
    drive_desktop.py --socket <path> type "Ship the toolbar"
    drive_desktop.py --socket <path> key enter
    drive_desktop.py --socket <path> scroll 1100 500 -300
    drive_desktop.py --socket <path> drag 120 180 120 320
    drive_desktop.py --socket <path> drop 640 400 ~/Desktop/shot.png notes.txt
    drive_desktop.py --socket <path> shot /tmp/window.png
    drive_desktop.py --socket <path> resize 960 600

Coordinates are window points from the top-left corner. `shot` saves the window at one pixel
per point, so positions read from a screenshot can be passed straight back to `click`; `--native`
saves it at the display's scale. The app captures its own window, which needs no Screen
Recording permission. `resize` prints the size the window reached.
"""
import argparse
import base64
import json
import os
import socket
import sys
import time


def send(path, step):
    with socket.socket(socket.AF_UNIX) as connection:
        connection.connect(path)
        connection.sendall((json.dumps(step) + "\n").encode())
        response = json.loads(connection.makefile().readline())
    if not response["ok"]:
        sys.exit(response["error"])
    return response["result"]


def shot(path, output, native):
    send(path, {"do": "window"})
    time.sleep(0.2)  # the window step asks for a repaint; let it land
    image = send(path, {"do": "capture", "native": native})
    with open(output, "wb") as written:
        written.write(base64.b64decode(image["png"]))


def drag(path, start, end, hold, settle, count):
    """Press at `start`, move to `end` in a few steps, and release unless `hold`."""
    (x1, y1), (x2, y2) = start, end
    # The press hit-tests against the last drawn frame, so draw a current one first.
    send(path, {"do": "move", "x": x1, "y": y1})
    time.sleep(settle / 4)
    send(path, {"do": "press", "x": x1, "y": y1, "count": count})
    for part in (0.1, 0.5, 0.9, 1.0):
        time.sleep(settle / 4)
        send(path, {"do": "drag", "x": x1 + (x2 - x1) * part, "y": y1 + (y2 - y1) * part})
    time.sleep(settle)
    if not hold:
        send(path, {"do": "release", "x": x2, "y": y2})


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--socket", required=True)
    parser.add_argument("--settle", type=float, default=0.4, help="seconds to wait after input")
    commands = parser.add_subparsers(dest="command", required=True)
    for name in ("click", "move"):
        command = commands.add_parser(name)
        command.add_argument("x", type=float)
        command.add_argument("y", type=float)
        if name == "click":
            command.add_argument("--count", type=int, default=1)
    scroll = commands.add_parser("scroll")
    for name, kind in (("x", float), ("y", float), ("dy", int)):
        scroll.add_argument(name, type=kind)
    commands.add_parser("key").add_argument("keystroke")
    commands.add_parser("type").add_argument("text")
    dragging = commands.add_parser("drag")
    for name in ("x1", "y1", "x2", "y2"):
        dragging.add_argument(name, type=float)
    dragging.add_argument("--hold", action="store_true", help="keep the button down, to capture mid-drag")
    dragging.add_argument("--count", type=int, default=1, help="2 presses as a double-click's second click")
    for name in ("press", "release"):
        command = commands.add_parser(name)
        command.add_argument("x", type=float)
        command.add_argument("y", type=float)
        command.add_argument("--button", choices=("left", "right"), default="left")
    drop = commands.add_parser("drop")
    drop.add_argument("x", type=float)
    drop.add_argument("y", type=float)
    drop.add_argument("paths", nargs="+", type=lambda p: os.path.abspath(os.path.expanduser(p)))
    shooting = commands.add_parser("shot")
    shooting.add_argument("output")
    shooting.add_argument("--native", action="store_true", help="at the display's scale")
    resize = commands.add_parser("resize")
    resize.add_argument("width", type=float)
    resize.add_argument("height", type=float)
    args = parser.parse_args()
    if args.command == "shot":
        return shot(args.socket, args.output, args.native)
    if args.command == "resize":
        send(args.socket, {"do": "resize", "width": args.width, "height": args.height})
        time.sleep(0.5)  # AppKit applies the resize after the step
        window = send(args.socket, {"do": "window"})
        return print(f"{window['width']:g}x{window['height']:g} (largest {window['max_width']:g}x{window['max_height']:g})")
    if args.command == "drag":
        drag(args.socket, (args.x1, args.y1), (args.x2, args.y2), args.hold, args.settle, args.count)
        return time.sleep(args.settle)
    step = {key: value for key, value in vars(args).items() if key not in ("socket", "settle", "command")}
    send(args.socket, {"do": args.command, **step})
    time.sleep(args.settle)


if __name__ == "__main__":
    main()
