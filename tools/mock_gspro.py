#!/usr/bin/env python3
"""Minimal GSPro OpenConnect mock for development and tests/gspro_mock.rs.

Behaves like GSPro where it matters: expects exactly one JSON object per TCP read
(otherwise replies 501 "Bad format"), answers heartbeats with 200 and shots with
200 + 201 player info. Each read comes 50 ms late, so messages sent back to back
merge and show up as BAD. Usage:  python3 tools/mock_gspro.py [port]   (default 921,
0 takes a free port)

Prints the port it listens on, then one line per read: `SHOT <json>` or
`heartbeat <json>` with the whole message, or `BAD ...`.
"""
import json
import socket
import sys
import time

port = int(sys.argv[1]) if len(sys.argv) > 1 else 921
srv = socket.socket()
srv.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
srv.bind(("127.0.0.1", port))
srv.listen(1)
print(f"mock GSPro listening on 127.0.0.1:{srv.getsockname()[1]}", flush=True)
while True:
    conn, _ = srv.accept()
    with conn:
        while True:
            time.sleep(0.05)
            data = conn.recv(65536)
            if not data:
                break
            try:
                msg = json.loads(data.decode())
            except ValueError:
                print("BAD (merged or partial message)", data[:80], flush=True)
                conn.sendall(b'{"Code":501,"Message":"Bad format","Player":null}')
                continue
            opts = msg.get("ShotDataOptions", {})
            if opts.get("ContainsBallData"):
                print("SHOT", json.dumps(msg), flush=True)
                conn.sendall(b'{"Code":200,"Message":"Ball Data received","Player":null}')
                time.sleep(0.05)
                conn.sendall(b'{"Code":201,"Message":"GSPro Player Information","Player":{"Handed":"RH","Club":"I7"}}')
            else:
                print("heartbeat", json.dumps(msg), flush=True)
                conn.sendall(b'{"Code":200,"Message":"Heartbeat","Player":null}')
