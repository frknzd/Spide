#!/usr/bin/env python3
"""Real pi PTY/control smoke. No model requests, user-file edits, or permission auto-approvals.
Uses an isolated disposable working directory; leaves global pi configuration unchanged.
"""
import fcntl, json, os, pathlib, pty, select, signal, socket, struct, subprocess, tempfile, termios, time

root = pathlib.Path(__file__).resolve().parents[1]
with tempfile.TemporaryDirectory(prefix="spide-real-", dir="/tmp") as directory:
    if os.environ.get("SPIDE_TEST_MCP"):
        config = pathlib.Path(directory) / ".pi"
        config.mkdir()
        (config / "mcp.json").write_text(json.dumps({"mcpServers": {"spide": {"url": os.environ["SPIDE_TEST_MCP"], "transport": "streamable-http", "lifecycle": "keep-alive", "directTools": True}}}))
    control_path = directory + "/pi.sock"
    server = socket.socket(socket.AF_UNIX)
    server.bind(control_path)
    server.listen(1)
    server.setblocking(False)
    master, slave = pty.openpty()
    fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 40, 100, 0, 0))
    token = os.urandom(32).hex()
    env = dict(os.environ, TERM="xterm-256color", COLORTERM="truecolor", SPIDE_CONTROL_SOCKET=control_path, SPIDE_CONTROL_TOKEN=token, SPIDE_CONTROL_GENERATION="99")
    pi = subprocess.Popen([os.environ.get("SPIDE_PI", "pi"), "--tools", "*", "--session-dir", directory + "/sessions", "-e", str(root / "agent/spide-terminal.ts")], cwd=directory, env=env, stdin=slave, stdout=slave, stderr=slave, start_new_session=True)
    os.close(slave)
    output = bytearray()
    peer = None
    pending = b""
    events = []
    key_start = None
    keys = [(0.2, b"\x10"), (0.5, b"\x0c"), (0.9, b"\x1b"), (1.2, b"\x1b[Z"), (1.5, b"/reload\r")]
    key_index = 0
    model_changes = 0
    try:
        end = time.monotonic() + 20
        while time.monotonic() < end and pi.poll() is None:
            readable, _, _ = select.select([master, server] + ([peer] if peer else []), [], [], .1)
            for item in readable:
                if item == master:
                    try: chunk = os.read(master, 65536)
                    except OSError: break
                    output.extend(chunk)
                    if b"\x1b[c" in chunk or b"\x1b[0c" in chunk:
                        os.write(master, b"\x1b[?1;2c")
                    if b"\x1b[6n" in chunk:
                        os.write(master, b"\x1b[1;1R")
                elif item == server:
                    peer, _ = server.accept()
                elif item == peer:
                    chunk = peer.recv(65536)
                    if not chunk: peer.close(); peer = None; continue
                    pending += chunk
                    while b"\n" in pending:
                        line, pending = pending.split(b"\n", 1)
                        v = json.loads(line)
                        if v.get("event") == "model": model_changes += 1
                        if v.get("type") == "hello":
                            assert v.get("token") == token and v.get("generation") == 99
                            required = {"read", "bash", "edit", "write", "grep", "find", "ls"}
                            assert required.issubset(v.get("tools", [])), "missing built-in tools"
                            events.append("authenticated; all seven built-in tools active")
                            peer.sendall(b'{"type":"ready","generation":99}\n')
            if events and key_start is None:
                key_start = time.monotonic()
            if key_start and key_index < len(keys) and time.monotonic() - key_start >= keys[key_index][0]:
                # Native non-model shortcuts only; no user prompt or automatic trust approval.
                os.write(master, keys[key_index][1])
                key_index += 1
            if len(events) >= 2:
                print("native /reload: authenticated reconnect observed")
                break
        print("control:", events or "NOT CONNECTED")
        print("native output bytes:", len(output))
        print("model-select events after Ctrl+P:", model_changes)
        print("modifyOtherKeys negotiated:", b"\x1b[>4;2m" in output)
        # ANSI display output is safe but noisy; show the tail only when startup failed.
        if len(events) < 2:
            import re
            text = output.decode("utf-8", "replace")
            print(re.sub(r"\x1b(?:\[[0-?]*[ -/]*[@-~]|\][^\x07]*(?:\x07|\x1b\\))", "", text)[-4000:])
            raise SystemExit(1)
    finally:
        os.killpg(pi.pid, signal.SIGTERM)
        try: pi.wait(timeout=3)
        except subprocess.TimeoutExpired:
            os.killpg(pi.pid, signal.SIGKILL)
            pi.wait()
        os.close(master)
        if peer: peer.close()
        server.close()
