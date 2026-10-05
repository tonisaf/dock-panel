"""Personal YouTube WebSub relay. Bind only behind an HTTPS reverse proxy."""
import hashlib
from contextlib import contextmanager
import hmac
import json
import os
import re
import sqlite3
import threading
import time
import urllib.parse
import urllib.request
import xml.etree.ElementTree as ET
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

CHANNEL = re.compile(r"UC[A-Za-z0-9_-]{22}\Z")
HUB = "https://pubsubhubbub.appspot.com/subscribe"
TOPIC = "https://www.youtube.com/feeds/videos.xml?channel_id="
DB = os.environ.get("PUSH_DB", "/var/lib/dock-panel-push/events.sqlite")
BASE = os.environ.get("PUSH_PUBLIC_URL", "").rstrip("/")
TOKEN = os.environ.get("PUSH_TOKEN", "")
SECRET = os.environ.get("PUSH_SECRET", "")
NEWS = threading.Condition()


@contextmanager
def connection():
    db = sqlite3.connect(DB, timeout=5)
    db.row_factory = sqlite3.Row
    try:
        with db:
            yield db
    finally:
        db.close()


def initialize():
    with connection() as db:
        db.executescript("""
            PRAGMA journal_mode=WAL;
            CREATE TABLE IF NOT EXISTS channels (
                id TEXT PRIMARY KEY, active INTEGER NOT NULL,
                lease REAL NOT NULL DEFAULT 0, retry REAL NOT NULL DEFAULT 0,
                error TEXT NOT NULL DEFAULT ''
            );
            CREATE TABLE IF NOT EXISTS events (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                channel TEXT NOT NULL, fingerprint TEXT NOT NULL UNIQUE,
                created REAL NOT NULL
            );
        """)


def callback_key(channel):
    return hmac.new(SECRET.encode(), channel.encode(), hashlib.sha256).hexdigest()


def set_channels(ids):
    if not isinstance(ids, list) or len(ids) > 1000 or any(not isinstance(c, str) or not CHANNEL.fullmatch(c) for c in ids):
        raise ValueError("Invalid channel list")
    with connection() as db:
        db.execute("UPDATE channels SET active=0")
        for channel in set(ids):
            db.execute("INSERT INTO channels(id,active) VALUES(?,1) ON CONFLICT(id) DO UPDATE SET active=1", (channel,))


def validate_challenge(channel, params):
    if params.get("hub.topic") != TOPIC + channel or params.get("hub.mode") != "subscribe":
        raise ValueError("Unknown subscription")
    challenge = params.get("hub.challenge", "")
    if not challenge or len(challenge) > 4096:
        raise ValueError("Invalid challenge")
    lease = int(params.get("hub.lease_seconds", "86400"))
    if not 60 <= lease <= 31 * 86400:
        raise ValueError("Invalid lease")
    with connection() as db:
        row = db.execute("SELECT active FROM channels WHERE id=?", (channel,)).fetchone()
        if not row or not row["active"]:
            raise ValueError("Inactive channel")
        db.execute("UPDATE channels SET lease=?,retry=0,error='' WHERE id=?", (time.time() + lease, channel))
    return challenge


def receive(channel, signature, body):
    algorithm, separator, digest = signature.partition("=")
    if not separator or algorithm not in ("sha1", "sha256"):
        raise ValueError("Missing signature")
    expected = hmac.new(callback_key(channel).encode(), body, getattr(hashlib, algorithm)).hexdigest()
    if not hmac.compare_digest(expected, digest):
        raise ValueError("Invalid signature")
    if b"<!DOCTYPE" in body.upper() or b"<!ENTITY" in body.upper():
        raise ValueError("Unsafe XML")
    root = ET.fromstring(body)
    ns = {"a": "http://www.w3.org/2005/Atom", "yt": "http://www.youtube.com/xml/schemas/2015"}
    entries = root.findall("a:entry", ns)
    if len(entries) > 200:
        raise ValueError("Too many entries")
    with connection() as db:
        row = db.execute("SELECT active FROM channels WHERE id=?", (channel,)).fetchone()
        if not row or not row["active"]:
            return
        for entry in entries:
            if entry.findtext("yt:channelId", namespaces=ns) != channel:
                raise ValueError("Wrong channel")
            video = entry.findtext("yt:videoId", namespaces=ns)
            if not video or not re.fullmatch(r"[A-Za-z0-9_-]{11}", video):
                raise ValueError("Invalid video")
            fingerprint = hashlib.sha256(channel.encode() + ET.tostring(entry)).hexdigest()
            db.execute("INSERT OR IGNORE INTO events(channel,fingerprint,created) VALUES(?,?,?)", (channel, fingerprint, time.time()))
        db.execute("DELETE FROM events WHERE id < (SELECT COALESCE(MAX(id),0)-5000 FROM events)")
    with NEWS:
        NEWS.notify_all()


def get_events(after, wait=20):
    deadline = time.monotonic() + wait
    with NEWS:
        while True:
            with connection() as db:
                bounds = db.execute("SELECT MIN(id),MAX(id) FROM events").fetchone()
                first, last = bounds[0] or 0, bounds[1] or 0
                if after > last or (after and first and after < first - 1):
                    channels = [r[0] for r in db.execute("SELECT id FROM channels WHERE active=1")]
                    return {"cursor": last, "channels": channels, "reset": True}
                rows = db.execute("SELECT id,channel FROM events WHERE id>? ORDER BY id LIMIT 100", (after,)).fetchall()
                if rows:
                    return {"cursor": rows[-1]["id"], "channels": sorted({r["channel"] for r in rows}), "reset": False}
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                return {"cursor": after, "channels": [], "reset": False}
            NEWS.wait(remaining)


def renew():
    while True:
        now = time.time()
        with connection() as db:
            due = db.execute("SELECT id FROM channels WHERE active=1 AND lease<? AND retry<? LIMIT 25", (now + 3600, now)).fetchall()
        for row in due:
            channel = row["id"]
            # Set before sending: the hub can validate asynchronously during POST.
            with connection() as db:
                db.execute("UPDATE channels SET retry=? WHERE id=?", (now + 300, channel))
            form = urllib.parse.urlencode({
                "hub.mode": "subscribe", "hub.topic": TOPIC + channel,
                "hub.callback": BASE + "/websub/" + channel + "/" + callback_key(channel),
                "hub.secret": callback_key(channel), "hub.lease_seconds": "86400",
            }).encode()
            try:
                with urllib.request.urlopen(urllib.request.Request(HUB, data=form), timeout=15) as response:
                    if response.status not in (202, 204):
                        raise ValueError("Hub rejected subscription")
            except Exception:
                with connection() as db:
                    db.execute("UPDATE channels SET error='Subscription request failed' WHERE id=?", (channel,))
        time.sleep(15)


class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, *_):
        pass  # Callback URLs contain verification secrets; never log them.

    def reply(self, status, data, content_type="application/json"):
        body = json.dumps(data).encode() if content_type == "application/json" else data.encode()
        self.send_response(status)
        self.send_header("Content-Type", content_type)
        self.send_header("Content-Length", str(len(body)))
        self.send_header("Cache-Control", "no-store")
        self.send_header("Connection", "close")
        self.end_headers()
        self.close_connection = True
        self.wfile.write(body)

    def authorized(self):
        return hmac.compare_digest(self.headers.get("Authorization", ""), "Bearer " + TOKEN)

    def callback(self, path):
        parts = path.split("/")
        if len(parts) != 4 or parts[1] != "websub" or not CHANNEL.fullmatch(parts[2]):
            raise ValueError("Invalid callback")
        if not hmac.compare_digest(parts[3], callback_key(parts[2])):
            raise ValueError("Invalid callback")
        return parts[2]

    def do_GET(self):
        try:
            url = urllib.parse.urlsplit(self.path)
            params = {k: v[0] for k, v in urllib.parse.parse_qs(url.query).items()}
            if url.path == "/health":
                return self.reply(200, {"ok": True})
            if url.path.startswith("/websub/"):
                channel = self.callback(url.path)
                return self.reply(200, validate_challenge(channel, params), "text/plain")
            if not self.authorized():
                return self.reply(401, {"error": "Unauthorized"})
            if url.path == "/events":
                after = int(params.get("after", "0"))
                if after < 0:
                    raise ValueError("Invalid cursor")
                return self.reply(200, get_events(after))
            if url.path == "/status":
                with connection() as db:
                    rows = [dict(r) for r in db.execute("SELECT id,active,lease,error FROM channels")]
                return self.reply(200, {"channels": rows})
            self.reply(404, {"error": "Not found"})
        except (ValueError, ET.ParseError):
            self.reply(400, {"error": "Invalid request"})
        except (BrokenPipeError, ConnectionResetError):
            pass

    def do_POST(self):
        try:
            path = urllib.parse.urlsplit(self.path).path
            callback = path.startswith("/websub/")
            if not callback and not self.authorized():
                return self.reply(401, {"error": "Unauthorized"})
            length = int(self.headers.get("Content-Length", "0"))
            if not 0 < length <= 1_000_000:
                return self.reply(413, {"error": "Invalid body size"})
            self.connection.settimeout(10)
            body = self.rfile.read(length)
            if len(body) != length:
                raise ValueError("Incomplete body")
            if callback:
                channel = self.callback(path)
                receive(channel, self.headers.get("X-Hub-Signature", ""), body)
                return self.reply(200, {"ok": True})
            if path == "/channels":
                set_channels(json.loads(body).get("channels"))
                return self.reply(200, {"ok": True})
            self.reply(404, {"error": "Not found"})
        except (ValueError, ET.ParseError, AttributeError):
            self.reply(400, {"error": "Invalid request"})
        except (BrokenPipeError, ConnectionResetError):
            pass


class Server(ThreadingHTTPServer):
    daemon_threads = True
    slots = threading.BoundedSemaphore(24)

    def process_request(self, request, address):
        if not self.slots.acquire(blocking=False):
            request.close()
            return
        try:
            super().process_request(request, address)
        except Exception:
            self.slots.release()
            raise

    def process_request_thread(self, request, address):
        try:
            request.settimeout(30)
            super().process_request_thread(request, address)
        finally:
            self.slots.release()


if __name__ == "__main__":
    if not BASE.startswith("https://") or len(TOKEN) < 32 or len(SECRET) < 32:
        raise SystemExit("HTTPS URL and strong PUSH_TOKEN/PUSH_SECRET required")
    initialize()
    threading.Thread(target=renew, daemon=True).start()
    Server((os.environ.get("PUSH_BIND", "127.0.0.1"), 8791), Handler).serve_forever()
