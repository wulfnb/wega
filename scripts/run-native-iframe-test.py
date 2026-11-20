#!/usr/bin/env python3
import json
import os
import signal
import subprocess
import sys
import tempfile
import threading
import time
from http.server import HTTPServer, BaseHTTPRequestHandler
from pathlib import Path

ROOT = Path("/Users/noufalbabu/dev/personal/wega")
HELPER = ROOT / "target/Wega.app/Contents/Helpers/WegaContent.app/Contents/MacOS/wega-engine"
BINARY = ROOT / "target/Wega.app/Contents/MacOS/wega"

server_port = 0

class Handler(BaseHTTPRequestHandler):
    def log_message(self, format, *args):
        pass

    def do_GET(self):
        global server_port
        path = self.path.split("?")[0]
        if path == "/parent.html":
            body = f"""<!doctype html>
<html>
<head>
    <title>Iframe Parent Test</title>
    <style>
        body {{ font-family: sans-serif; background: #f0f4f8; margin: 10px; }}
        .box {{ border: 2px solid #333; margin: 5px; padding: 5px; background: white; }}
        iframe {{ width: 280px; height: 120px; border: 1px solid #666; }}
        #results {{ font-weight: bold; color: green; margin-top: 10px; }}
    </style>
</head>
<body>
    <h1>Iframe Native Test Page</h1>
    <div class="box">
        <h3>Same-Origin Frame:</h3>
        <iframe id="same" src="http://127.0.0.1:{server_port}/same.html"></iframe>
    </div>
    <div class="box">
        <h3>Cross-Origin Frame:</h3>
        <iframe id="cross" src="http://localhost:{server_port}/cross.html"></iframe>
    </div>
    <div class="box">
        <h3>Nested Cross-Origin Frame:</h3>
        <iframe id="nested" src="http://localhost:{server_port}/nested.html"></iframe>
    </div>
    <div class="box">
        <h3>Sandboxed Opaque Frame:</h3>
        <iframe id="sandboxed" sandbox="allow-scripts" srcdoc="&lt;p style='color:purple'&gt;Sandboxed Opaque Origin&lt;/p&gt;"></iframe>
    </div>
    <div id="results">Running checks...</div>
    <script>
        window.addEventListener('load', () => {{
            setTimeout(() => {{
                try {{
                    const same = document.getElementById('same');
                    const cross = document.getElementById('cross');
                    const sameDocOk = same.contentDocument !== null && same.contentWindow.document !== null;
                    let crossDocDenied = false;
                    try {{
                        cross.contentWindow.document;
                    }} catch (e) {{
                        crossDocDenied = (e.name === 'SecurityError');
                    }}
                    const res = document.getElementById('results');
                    res.textContent = 'CHECKS: sameDoc=' + sameDocOk + ' crossDenied=' + crossDocDenied;
                    console.log('NATIVE_TEST_RESULTS: sameDoc=' + sameDocOk + ' crossDenied=' + crossDocDenied);
                }} catch (e) {{
                    document.getElementById('results').textContent = 'ERROR: ' + e;
                }}
            }}, 1500);
        }});
    </script>
</body>
</html>"""
            self.send_response(200)
            self.send_header("Content-Type", "text/html; charset=utf-8")
            self.end_headers()
            self.wfile.write(body.encode("utf-8"))
        elif path == "/same.html":
            body = """<!doctype html><html><body style="background:#e6fffa"><p style="color:teal">Same-Origin Frame Loaded Successfully</p></body></html>"""
            self.send_response(200)
            self.send_header("Content-Type", "text/html; charset=utf-8")
            self.end_headers()
            self.wfile.write(body.encode("utf-8"))
        elif path == "/cross.html":
            body = """<!doctype html><html><body style="background:#fffaf0"><p style="color:orange">Cross-Origin Frame Loaded Successfully</p></body></html>"""
            self.send_response(200)
            self.send_header("Content-Type", "text/html; charset=utf-8")
            self.end_headers()
            self.wfile.write(body.encode("utf-8"))
        elif path == "/nested.html":
            body = f"""<!doctype html><html><body style="background:#fefcbf"><p style="color:brown">Nested Parent (localhost)</p><iframe src="http://127.0.0.1:{server_port}/deep.html" style="width:200px;height:60px"></iframe></body></html>"""
            self.send_response(200)
            self.send_header("Content-Type", "text/html; charset=utf-8")
            self.end_headers()
            self.wfile.write(body.encode("utf-8"))
        elif path == "/deep.html":
            body = """<!doctype html><html><body style="background:#fed7d7"><p style="color:maroon">Deep Child (127.0.0.1)</p></body></html>"""
            self.send_response(200)
            self.send_header("Content-Type", "text/html; charset=utf-8")
            self.end_headers()
            self.wfile.write(body.encode("utf-8"))
        else:
            self.send_response(404)
            self.end_headers()

def main():
    global server_port
    server = HTTPServer(("127.0.0.1", 0), Handler)
    server_port = server.server_port
    t = threading.Thread(target=server.serve_forever, daemon=True)
    t.start()

    profile = tempfile.mkdtemp(prefix="wega-native-iframe-")
    out_png = ROOT / "docs/architecture/baseline/iframe-evidence/native-window-iframes.png"
    out_json = ROOT / "docs/architecture/baseline/iframe-evidence/native-capture.json"

    env = os.environ.copy()
    env["WEGA_TEST_ALLOW_LOOPBACK"] = "1"
    env["WEGA_CONTENT_HELPER"] = str(HELPER)
    env["WEGA_ENGINE_BINARY"] = str(HELPER)

    url = f"http://127.0.0.1:{server_port}/parent.html"
    cmd = [
        str(BINARY),
        "--engine", "wega",
        "--profile-dir", profile,
        "--capture", str(out_png),
        url
    ]

    print(f"Launching real native window test with signed worker: {' '.join(cmd)}")
    log_file = Path(profile) / "browser.log"
    with log_file.open("w") as log:
        proc = subprocess.Popen(cmd, stdout=log, stderr=log, env=env)
        try:
            status = proc.wait(timeout=30)
        except subprocess.TimeoutExpired:
            proc.kill()
            proc.wait()
            status = 124

    server.shutdown()

    log_content = log_file.read_text() if log_file.exists() else ""
    print(f"Exit status: {status}")
    print(f"Log output:\n{log_content}")

    if out_png.exists() and out_png.stat().st_size > 0:
        print(f"Capture successful: {out_png} ({out_png.stat().st_size} bytes)")
    else:
        print("ERROR: Capture image not generated!")
        sys.exit(1)

    if out_json.exists():
        print(f"Capture report:\n{out_json.read_text()}")

if __name__ == "__main__":
    main()
