#!/usr/bin/env python3
"""A stand-in for workspace-desktop under --automation-socket: it answers every step with a
960x600 window. With RECORD set, it first writes its process id, arguments and environment
there as JSON. With AGENT set, it starts a process in a session of its own, as the app's host
starts agents, and writes that process's id to AGENT. With EXIT set, it then exits, as a copy
that fails at startup does, leaving that process without its parent."""
import json
import os
import socket
import subprocess
import sys
import time

path = sys.argv[sys.argv.index("--automation-socket") + 1]
if "RECORD" in os.environ:
    with open(os.environ["RECORD"], "w") as record:
        json.dump({"pid": os.getpid(), "args": sys.argv[1:], "env": dict(os.environ)}, record)
if "AGENT" in os.environ:
    agent = subprocess.Popen(["sleep", "300"], start_new_session=True)
    with open(os.environ["AGENT"], "w") as written:
        written.write(str(agent.pid))
if "EXIT" in os.environ:
    time.sleep(0.2)  # long enough for the host's watcher to see the agent as its child
    sys.exit(1)
server = socket.socket(socket.AF_UNIX)
server.bind(path)
server.listen()
while True:
    connection, _ = server.accept()
    connection.makefile().readline()
    connection.sendall(b'{"ok":true,"result":{"width":960,"height":600,"scale":2}}\n')
    connection.close()
