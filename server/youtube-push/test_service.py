import hashlib
import hmac
import json
import tempfile
import threading
import unittest
import urllib.error
import urllib.request
from pathlib import Path

import service

CHANNEL = "UC" + "a" * 22
VIDEO = "b" * 11


class RelayTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        service.DB = str(Path(self.directory.name) / "events.sqlite")
        service.SECRET = "s" * 64
        service.TOKEN = "t" * 64
        service.initialize()
        service.set_channels([CHANNEL])

    def tearDown(self):
        self.directory.cleanup()

    def feed(self, channel=CHANNEL):
        return f'<feed xmlns="http://www.w3.org/2005/Atom" xmlns:yt="http://www.youtube.com/xml/schemas/2015"><entry><yt:channelId>{channel}</yt:channelId><yt:videoId>{VIDEO}</yt:videoId><title>Video</title><updated>2026-10-05T10:00:00Z</updated></entry></feed>'.encode()

    def signature(self, body):
        return "sha1=" + hmac.new(service.callback_key(CHANNEL).encode(), body, hashlib.sha1).hexdigest()

    def test_signed_delivery_and_duplicate_retry(self):
        body = self.feed()
        for _ in range(2):
            service.receive(CHANNEL, self.signature(body), body)
        event = service.get_events(0, wait=0)
        self.assertEqual(event["channels"], [CHANNEL])
        self.assertEqual(event["cursor"], 1)
        self.assertEqual(service.get_events(1, wait=0)["channels"], [])

    def test_wrong_signature_or_wrong_channel_does_not_create_events(self):
        with self.assertRaises(ValueError):
            service.receive(CHANNEL, "sha1=wrong", self.feed())
        body = self.feed("UC" + "z" * 22)
        with self.assertRaises(ValueError):
            service.receive(CHANNEL, self.signature(body), body)
        self.assertEqual(service.get_events(0, wait=0)["channels"], [])

    def test_challenge_requires_registered_topic(self):
        params = {"hub.topic": service.TOPIC + CHANNEL, "hub.mode": "subscribe", "hub.challenge": "verify", "hub.lease_seconds": "86400"}
        self.assertEqual(service.validate_challenge(CHANNEL, params), "verify")
        params["hub.topic"] = "https://attacker.example/feed"
        with self.assertRaises(ValueError):
            service.validate_challenge(CHANNEL, params)

    def test_removed_channel_does_not_deliver(self):
        service.set_channels([])
        body = self.feed()
        service.receive(CHANNEL, self.signature(body), body)
        self.assertEqual(service.get_events(0, wait=0)["channels"], [])

    def test_long_poll_wakes_on_event(self):
        result = []
        thread = threading.Thread(target=lambda: result.append(service.get_events(0, wait=2)))
        thread.start()
        body = self.feed()
        service.receive(CHANNEL, self.signature(body), body)
        thread.join(timeout=3)
        self.assertFalse(thread.is_alive())
        self.assertEqual(result[0]["channels"], [CHANNEL])

    def test_http_requires_token_and_accepts_subscription_updates(self):
        server = service.Server(("127.0.0.1", 0), service.Handler)
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        base = "http://127.0.0.1:" + str(server.server_address[1])
        try:
            with self.assertRaises(urllib.error.HTTPError) as error:
                urllib.request.urlopen(base + "/status")
            self.assertEqual(error.exception.code, 401)
            request = urllib.request.Request(base + "/channels", data=json.dumps({"channels": [CHANNEL]}).encode(), headers={"Authorization": "Bearer " + service.TOKEN})
            with urllib.request.urlopen(request) as response:
                self.assertEqual(response.status, 200)
        finally:
            server.shutdown()
            server.server_close()
            thread.join(timeout=2)


if __name__ == "__main__":
    unittest.main()
