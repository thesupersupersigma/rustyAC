# SPDX-License-Identifier: MIT OR Apache-2.0

"""Spawn kawaiidra like Claude Code does and run an MCP handshake (debug helper)."""
import json, os, subprocess, sys, threading, time

env = dict(os.environ,
           GHIDRA_INSTALL_DIR=r"C:\ghidra_12.1.2_PUBLIC",
           JAVA_HOME=r"C:\Program Files\Java\jdk-21.0.10",
           KAWAIIDRA_PROJECT_DIR=r"C:\Users\thesupersupersigma\Desktop\project5\rustyAC\ghidra",
           KAWAIIDRA_DEFAULT_PROJECT="acs")
srv = r"C:\Users\thesupersupersigma\Desktop\project5\rustyAC\tools\kawaiidra-mcp\run_server.py"
p = subprocess.Popen([sys.executable, srv], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                     stderr=subprocess.PIPE, env=env, text=True, bufsize=1)
err = []
threading.Thread(target=lambda: [err.append(l) for l in p.stderr], daemon=True).start()

def send(obj):
    p.stdin.write(json.dumps(obj) + "\n"); p.stdin.flush()

send({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
    "protocolVersion": "2025-06-18", "capabilities": {},
    "clientInfo": {"name": "handshake-test", "version": "0"}}})
t0 = time.time(); line = p.stdout.readline()
print(f"initialize reply after {time.time()-t0:.1f}s:", line[:300] or "<no reply>")
if line:
    send({"jsonrpc": "2.0", "method": "notifications/initialized"})
    send({"jsonrpc": "2.0", "id": 2, "method": "tools/list"})
    line = p.stdout.readline()
    try:
        print("tools:", len(json.loads(line)["result"]["tools"]))
    except Exception:
        print("tools/list reply:", line[:300] or "<no reply>")
time.sleep(1)
print("exit code:", p.poll())
print("--- stderr (last 25 lines) ---"); print("".join(err[-25:]))
p.kill()
