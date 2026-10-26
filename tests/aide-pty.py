#!/usr/bin/env python3
"""Give an acceptance-test subprocess a real terminal without a GUI."""
import os
import pty
import select
import subprocess
import sys

master, slave = pty.openpty()
child = subprocess.Popen(sys.argv[1:], stdin=slave, stdout=slave, stderr=slave)
os.close(slave)
try:
    while True:
        ready, _, _ = select.select([master, sys.stdin.fileno()], [], [], 1)
        if master in ready:
            try:
                data = os.read(master, 65536)
            except OSError:
                break
            if not data:
                break
            os.write(sys.stdout.fileno(), data)
        if sys.stdin.fileno() in ready:
            data = os.read(sys.stdin.fileno(), 65536)
            if data:
                os.write(master, data)
        if child.poll() is not None and not ready:
            break
finally:
    os.close(master)
    if child.poll() is None:
        child.terminate()
    child.wait()
sys.exit(child.returncode)
