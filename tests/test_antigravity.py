"""Antigravity's quota for Gemini 订阅, asked of the language server its app noted (antigravity.rs reads it the same way)."""
from http.server import BaseHTTPRequestHandler, HTTPServer
import json
import os
from pathlib import Path
import socket
import sys
import tempfile
import threading
import time
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'lib'))
import swiftbar_antigravity as antigravity


class AntigravityQuotaTests(unittest.TestCase):
    def test_a_response_of_the_wrong_shape_is_no_windows(self):
        for summary in ({'response': 'x'}, {'response': ['groups']}, {'response': {'groups': 'x'}}, [], 'x', None):
            with self.subTest(summary=summary):self.assertEqual(antigravity.windows(summary), [])

    def test_a_live_servers_answer_is_the_reason_not_a_gone_ones_refusal(self):
        # The newer note's server is up and answers 500; an older note's process id is alive (another's now) but
        # nothing listens on its port. The reason is the 500, not 没有运行.
        class Failing(BaseHTTPRequestHandler):
            def do_POST(self):
                self.send_response(500);self.send_header('Content-Length', '0');self.end_headers()
            def log_message(self, *args):pass
        server = HTTPServer(('127.0.0.1', 0), Failing)
        threading.Thread(target=server.serve_forever, daemon=True).start()
        self.addCleanup(server.server_close);self.addCleanup(server.shutdown)
        with socket.socket() as spare:
            spare.bind(('127.0.0.1', 0));gone = spare.getsockname()[1]
        home = tempfile.TemporaryDirectory();self.addCleanup(home.cleanup)
        daemon = Path(home.name) / '.gemini' / 'antigravity' / 'daemon';daemon.mkdir(parents=True)
        note = lambda port: json.dumps({'pid': os.getpid(), 'httpPort': port, 'csrfToken': 't'})
        (daemon / 'hub_1.json').write_text(note(gone));(daemon / 'hub_2.json').write_text(note(server.server_port))
        earlier = time.time() - 600;os.utime(daemon / 'hub_1.json', (earlier, earlier))
        self.assertEqual(antigravity.quota(home.name), (None, 'Antigravity 没有给出额度（HTTP 500）'))
        (daemon / 'hub_2.json').unlink()
        self.assertEqual(antigravity.quota(home.name), (None, antigravity.NOT_RUNNING))


if __name__ == '__main__':
    unittest.main()
