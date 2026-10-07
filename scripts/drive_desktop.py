#!/usr/bin/env python3
"""Drive a running workspace-desktop through its development automation socket.

Start the app with `--automation-socket <path>`, then:

    drive_desktop.py --socket <path> click 640 400
    drive_desktop.py --socket <path> type "Ship the toolbar"
    drive_desktop.py --socket <path> key enter
    drive_desktop.py --socket <path> scroll 1100 500 -300
    drive_desktop.py --socket <path> shot /tmp/window.png

Coordinates are window points from the top-left corner. `shot` saves the window at one pixel
per point, so positions read from a screenshot can be passed straight back to `click`.
"""
import argparse
import json
import socket
import subprocess
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


def shot(path, output):
    window = send(path, {"do": "window"})
    time.sleep(0.2)  # the window step asks for a repaint; let it land
    capture = ["screencapture", "-x", "-o", "-l", str(window["number"]), output]
    if subprocess.run(capture, capture_output=True).returncode:
        sys.exit("Could not capture the window. macOS refuses captures while the screen is locked.")
    size = [str(round(window["height"])), str(round(window["width"]))]
    subprocess.run(["sips", "-z", *size, output], check=True, capture_output=True)


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
    commands.add_parser("shot").add_argument("output")
    args = parser.parse_args()
    if args.command == "shot":
        return shot(args.socket, args.output)
    step = {key: value for key, value in vars(args).items() if key not in ("socket", "settle", "command")}
    send(args.socket, {"do": args.command, **step})
    time.sleep(args.settle)


if __name__ == "__main__":
    main()
