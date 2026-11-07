# The http neurons' test server (one for a run): a mode and a port, the mode
# naming what every request gets. The server prints its bound port as its
# first stdout line, so a test can name the URL.
#   json       200 {"seen": <the body parsed>, "ok": true}
#   echo       200 the request's body as it arrived
#   empty      200 no body (Content-Length: 0)
#   no204      HTTP 204, no body
#   bad        HTTP 500, an html-ish body
#   text       HTTP 200, the body "not a json value at all"
#   slow <ms>  the request waits <ms>, then answers "json"
#   hang <ms>  the request waits <ms>, then answers "json" and keeps reading
import json
import sys
import threading
from http.server import BaseHTTPRequestHandler, HTTPServer


class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, *args):
        pass

    def do_POST(self):
        length = int(self.headers.get("Content-Length", 0) or 0)
        body = self.rfile.read(length) if length else b""
        mode = MODE
        extra = 0
        if mode.startswith("slow "):
            extra = int(float(mode.split()[1]))
        if mode.startswith("hang "):
            extra = int(float(mode.split()[1]))
        if extra:
            threading.Event().wait(extra / 1000.0)
        if mode == "json" or mode.startswith("slow") or mode.startswith("hang"):
            try:
                seen = json.loads(body) if body else None
            except Exception:
                seen = None
            out = json.dumps({"seen": seen, "ok": True}).encode()
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(out)))
            self.end_headers()
            self.wfile.write(out)
        elif mode == "echo":
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)
        elif mode == "empty":
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", "0")
            self.end_headers()
        elif mode == "no204":
            self.send_response(204)
            self.end_headers()
        elif mode == "bad":
            out = b"<html>gone wrong</html>"
            self.send_response(500)
            self.send_header("Content-Type", "text/html")
            self.send_header("Content-Length", str(len(out)))
            self.end_headers()
            self.wfile.write(out)
        elif mode == "text":
            out = b"not a json value at all"
            self.send_response(200)
            self.send_header("Content-Type", "text/plain")
            self.send_header("Content-Length", str(len(out)))
            self.end_headers()
            self.wfile.write(out)
        else:
            out = b"{}"
            self.send_response(200)
            self.send_header("Content-Length", str(len(out)))
            self.end_headers()
            self.wfile.write(out)
        # The connection stays open until the engine's side closes it; a
        # client that cut the request reads nothing until its own shutdown.


class Server(HTTPServer):
    # One thread per request, so a slow first request does not hold the next.
    daemon_threads = True

    def process_request(self, request, client_address):
        threading.Thread(target=self._run_one, args=(request, client_address), daemon=True).start()

    def _run_one(self, request, client_address):
        try:
            self.finish_request(request, client_address)
        except Exception:
            pass
        finally:
            try:
                request.close()
            except Exception:
                pass


if __name__ == "__main__":
    MODE = sys.argv[1] if len(sys.argv) > 1 else "json"
    server = Server(("127.0.0.1", 0), Handler)
    print(server.server_address[1], flush=True)
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass
