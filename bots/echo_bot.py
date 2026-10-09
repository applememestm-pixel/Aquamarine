#!/usr/bin/env python3
"""
Aquamarine example bot — an echo bot.

Shows how to drive a bot over the Aquamarine HTTP Bot API. A bot is a
registered @username with an API token (create one in the desktop app:
Settings → Bots → Create, then copy the token).

The bot polls the server for messages sent to it and replies. Messages
to/from bots are PLAINTEXT (no E2EE) — bots can't run the interactive
X3DH handshake. This is clearly marked in the client UI.

Usage:
    pip install requests
    python echo_bot.py --server http://127.0.0.1:7777 --token aqbot_xxxxx

Then, in the Aquamarine app, search for the bot's @username and start a
chat — whatever you send comes straight back.
"""
import argparse
import time
import sys

try:
    import requests
except ImportError:
    sys.exit("pip install requests first")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--server", default="http://127.0.0.1:7777")
    ap.add_argument("--token", required=True, help="bot token from Settings → Bots")
    ap.add_argument("--poll", type=float, default=1.0, help="poll interval seconds")
    args = ap.parse_args()

    base = args.server.rstrip("/")
    after = 0
    print(f"echo bot running against {base} — Ctrl+C to stop")

    while True:
        try:
            r = requests.get(
                f"{base}/v1/bot/updates",
                params={"token": args.token, "after": after},
                timeout=30,
            )
            if r.status_code != 200:
                print("updates error:", r.status_code, r.text)
                time.sleep(args.poll)
                continue
            updates = r.json()
            for u in updates:
                after = max(after, u["id"] + 1)
                frm, text = u["from"], u["text"]
                print(f"<- {frm}: {text}")
                reply = f"echo: {text}"
                sr = requests.post(
                    f"{base}/v1/bot/send",
                    json={"token": args.token, "to": frm, "text": reply},
                    timeout=30,
                )
                if sr.status_code not in (200, 204):
                    print("send error:", sr.status_code, sr.text)
                else:
                    print(f"-> {frm}: {reply}")
        except requests.RequestException as e:
            print("network error:", e)
        time.sleep(args.poll)


if __name__ == "__main__":
    main()
