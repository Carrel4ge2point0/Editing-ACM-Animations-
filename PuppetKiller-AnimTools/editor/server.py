"""Local server for the animation editor (127.0.0.1 only).
Usage: python server.py <folder with Puppet Native style JSON animations> [port]
Open http://127.0.0.1:8765/  — edited versions are saved to ./edits next to this file."""
import http.server, json, os, sys, threading
from urllib.parse import parse_qs, urlparse
HERE = os.path.dirname(os.path.abspath(__file__))
SRC = os.path.abspath(sys.argv[1]) if len(sys.argv) > 1 else os.path.join(HERE, 'animations')
EDITS = os.path.join(HERE, 'edits')
SOURCES = {'pn': SRC, 'ed': EDITS}
lock = threading.Lock()

class H(http.server.SimpleHTTPRequestHandler):
    def __init__(self, *a, **k): super().__init__(*a, directory=HERE, **k)
    def _json(self, d):
        d = json.dumps(d, ensure_ascii=False).encode()
        self.send_response(200); self.send_header('Content-Type', 'application/json'); self.end_headers(); self.wfile.write(d)
    def do_GET(self):
        if self.path.startswith('/sources'):
            out = []
            for k, d in SOURCES.items():
                if os.path.isdir(d): out += [{'src': k, 'f': f} for f in sorted(os.listdir(d)) if f.endswith('.json') and not f.endswith('.edition.json')]
            return self._json(out)
        if self.path.startswith('/source?'):
            q = parse_qs(urlparse(self.path).query); k, f = q.get('src', [''])[0], os.path.basename(q.get('f', [''])[0])
            if k not in SOURCES or not f.endswith('.json'): self.send_error(400); return
            try: return self._json(json.load(open(os.path.join(SOURCES[k], f))))
            except Exception: self.send_error(404); return
        super().do_GET()
    def do_POST(self):
        if not self.path.startswith('/edition'): self.send_error(404); return
        n = int(self.headers.get('Content-Length', 0))
        if n > 20000000: self.send_error(413); return
        e = json.loads(self.rfile.read(n))
        name = ''.join(c for c in str(e.get('nom', '')) if c.isalnum() or c in '-_.')[:120]
        if not name or not isinstance(e.get('animation'), dict): self.send_error(400); return
        os.makedirs(EDITS, exist_ok=True)
        with lock:
            json.dump(e['animation'], open(os.path.join(EDITS, name + '.json'), 'w'))
            json.dump({k: v for k, v in e.items() if k != 'animation'}, open(os.path.join(EDITS, name + '.edition.json'), 'w'), indent=1)
        return self._json({'ok': True, 'fichier': 'edits/' + name + '.json'})

port = int(sys.argv[2]) if len(sys.argv) > 2 else 8765
print('Animation editor: http://127.0.0.1:%d/   sources: %s   edits: %s' % (port, SRC, EDITS))
http.server.ThreadingHTTPServer(('127.0.0.1', port), H).serve_forever()
