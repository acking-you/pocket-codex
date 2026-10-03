#!/usr/bin/env python3
"""Minimal ACP agent over stdio for process tests (standard library only).

Implements initialize, session/list, session/new and session/prompt (echo,
then end_turn). With --spawn-child it starts `sleep 60` in its process group
and writes the child pid to --pid-file, so a test can check that the whole
tree is terminated.
"""
import json
import subprocess
import sys


def main():
    args = sys.argv[1:]
    pid_file = None
    if "--pid-file" in args:
        pid_file = args[args.index("--pid-file") + 1]
    if "--spawn-child" in args:
        child = subprocess.Popen(["sleep", "60"])
        if pid_file:
            with open(pid_file, "w") as f:
                f.write(str(child.pid))
    sessions = []
    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue
        msg = json.loads(line)
        method = msg.get("method")
        if "id" not in msg or method is None:
            continue
        params = msg.get("params") or {}
        if method == "initialize":
            result = {
                "protocolVersion": 1,
                "agentCapabilities": {"sessionCapabilities": {"list": {}}},
                "authMethods": [],
                "agentInfo": {"name": "fake-agent.py", "version": "0.1"},
            }
        elif method == "session/list":
            result = {"sessions": sessions}
        elif method == "session/new":
            sid = "py-%d" % (len(sessions) + 1)
            sessions.append({"sessionId": sid, "cwd": params.get("cwd", "")})
            result = {"sessionId": sid}
        elif method == "session/prompt":
            text = " ".join(b.get("text", "") for b in params.get("prompt", []))
            update = {
                "sessionId": params.get("sessionId"),
                "update": {"sessionUpdate": "agent_message_chunk",
                           "content": {"type": "text", "text": text}},
            }
            print(json.dumps({"jsonrpc": "2.0", "method": "session/update", "params": update}),
                  flush=True)
            result = {"stopReason": "end_turn"}
        else:
            print(json.dumps({"jsonrpc": "2.0", "id": msg["id"],
                              "error": {"code": -32601, "message": "not supported"}}), flush=True)
            continue
        print(json.dumps({"jsonrpc": "2.0", "id": msg["id"], "result": result}), flush=True)


if __name__ == "__main__":
    main()
