#!/usr/bin/env python3
"""
Aquamarine example bot: echo.

Reference implementation of a bot for the Aquamarine HTTP Bot API
(docs/BOTS.md). Standard library only.

    AQUA_BOT_TOKEN=aqbot_… python3 echo_bot.py --server https://relay.example

The token is read from the AQUA_BOT_TOKEN environment variable (or asked
for with hidden input) so that it does not appear in the process list or
in shell history. Messages to and from bots are not end-to-end encrypted.
"""
import argparse
import getpass
import json
import os
import sys
import time
import urllib.error
import urllib.parse
import urllib.request


class Bot:
    def __init__(self, server: str, token: str):
        self.base = server.rstrip("/")
        self.token = token

    def call(self, method: str, path: str, query=None, body=None, timeout=40):
        url = self.base + path + ("?" + urllib.parse.urlencode(query) if query else "")
        data = json.dumps(body).encode() if body is not None else None
        req = urllib.request.Request(url, data=data, method=method, headers={
            "Authorization": "Bot " + self.token,
            "Content-Type": "application/json",
            "User-Agent": "aqua-echo-bot/2",
        })
        with urllib.request.urlopen(req, timeout=timeout) as r:
            raw = r.read()
            return json.loads(raw) if raw else None

    def me(self):
        return self.call("GET", "/v1/bot/me")

    def updates(self, after: int, timeout: int = 25):
        return self.call("GET", "/v1/bot/updates", {"after": after, "timeout": timeout}, timeout=timeout + 10)

    def send(self, to: str, text: str):
        for _ in range(3):
            try:
                return self.call("POST", "/v1/bot/send", body={"to": to, "text": text[:4096]})
            except urllib.error.HTTPError as e:
                if e.code == 429:          # rate limit: back off and retry
                    time.sleep(1.1)
                    continue
                if e.code == 403:          # the user has not written to the bot, or sent /stop
                    return None
                raise


def main():
    ap = argparse.ArgumentParser(description="Aquamarine echo bot")
    ap.add_argument("--server", default=os.environ.get("AQUA_SERVER", "http://127.0.0.1:7777"))
    a = ap.parse_args()
    token = os.environ.get("AQUA_BOT_TOKEN") or getpass.getpass("Bot token (aqbot_…): ").strip()
    bot = Bot(a.server, token)
    try:
        me = bot.me()
    except urllib.error.HTTPError as e:
        sys.exit("token rejected" if e.code == 401 else f"relay error {e.code}")
    print(f"@{me['username']} is running against {bot.base} (Ctrl+C to stop)")

    after = 0
    while True:
        try:
            for u in bot.updates(after):
                after = max(after, u["id"] + 1)    # acknowledges everything before
                text = u["text"].strip()
                if text == "/stop":
                    continue
                reply = "Send me anything and I will repeat it." if text == "/start" else text
                bot.send(u["from"], reply)
                print(f"{u['from']}: {text!r}")
        except urllib.error.HTTPError as e:
            if e.code == 401:
                sys.exit("token rejected (reset or deleted)")
            time.sleep(2)
        except (urllib.error.URLError, TimeoutError, ConnectionError):
            time.sleep(2)                          # relay restarting or network down


if __name__ == "__main__":
    try:
        main()
    except KeyboardInterrupt:
        pass
