#!/usr/bin/env python3
"""
aqua-console — read and write an Aquamarine chat from your terminal.

It is a tiny bot: create one in Aquamarine (Settings -> Bots), copy its
token, run this script, and write to the bot from the app (or let friends
write to it). Every message shows up in the console and whatever you type
goes back as the bot.

    python3 aqua-console.py --server https://relay.example:8443
    python3 aqua-console.py --server http://127.0.0.1:8080 --chat alice
    python3 aqua-console.py --server ... --room      # console chat room

The token is asked for interactively (input hidden) unless AQUA_BOT_TOKEN
is set; it is never taken from the command line, where other users of the
machine could see it in the process list. Only the Python standard library is used,
so it runs anywhere Python 3.8+ does (Linux, macOS, Windows).

Commands while it runs:
    /to <user>    write to @user from now on
    /who          people who wrote to the bot in this session
    /room         toggle room mode (the bot forwards every message to all
                  who have joined; anyone joins by writing /join)
    /quit         exit (Ctrl+C / Ctrl+D work too)
Everything else is sent as a message.

Note: bot chats are NOT end-to-end encrypted — the relay sees the text.
"""

import argparse
import getpass
import json
import os
import sys
import threading
import time
import urllib.error
import urllib.parse
import urllib.request

COLORS = ["\033[96m", "\033[95m", "\033[93m", "\033[92m", "\033[94m", "\033[91m"]
DIM, BOLD, RESET = "\033[2m", "\033[1m", "\033[0m"

out_lock = threading.Lock()
state = {"to": None, "room": False, "members": set(), "seen": [], "prompt": "> "}


def color_for(name):
    return COLORS[sum(map(ord, name)) % len(COLORS)]


def say(line):
    """Print above the prompt without mangling what is being typed."""
    with out_lock:
        sys.stdout.write("\r\033[K" + line + "\n" + state["prompt"])
        sys.stdout.flush()


def api(server, token, path, query=None, body=None, timeout=40):
    url = server.rstrip("/") + path
    if query:
        url += "?" + urllib.parse.urlencode(query)
    data = None
    # The token travels in a header, never in the URL (URLs end up in logs).
    headers = {"User-Agent": "aqua-console/2", "Authorization": "Bot " + token}
    if body is not None:
        data = json.dumps(body).encode()
        headers["Content-Type"] = "application/json"
    req = urllib.request.Request(url, data=data, headers=headers, method="POST" if body is not None else "GET")
    with urllib.request.urlopen(req, timeout=timeout) as r:
        raw = r.read()
        return json.loads(raw) if raw else None


def send(server, token, to, text):
    for attempt in range(3):
        try:
            api(server, token, "/v1/bot/send", body={"to": to, "text": text[:4096]})
            return
        except urllib.error.HTTPError as e:
            if e.code == 429:  # rate limit: 30 msg/s per bot, 1 msg/s per chat
                time.sleep(1.2)
                continue
            if e.code == 403:
                say(f"\033[93m@{to} hasn't written to this bot (or sent /stop) — bots can only answer.\033[0m")
                return
            raise


def poll(server, token, me, stop):
    after = 0
    first = True
    while not stop.is_set():
        try:
            ups = api(server, token, "/v1/bot/updates", {"after": after, "timeout": 25}, timeout=40)
        except urllib.error.HTTPError as e:
            if e.code == 401:
                say("\033[91mThe token was rejected (deleted or reset?). Exiting.\033[0m")
                stop.set()
                os._exit(2)
            say(f"{DIM}relay error {e.code}, retrying…{RESET}")
            time.sleep(3)
            continue
        except Exception as e:  # network hiccup, sleep and retry
            say(f"{DIM}offline ({e.__class__.__name__}), retrying…{RESET}")
            time.sleep(3)
            continue
        for u in ups or []:
            after = max(after, u["id"] + 1)
            if first and time.time() - u.get("ts", 0) > 3600:
                continue  # skip stale backlog from long ago
            on_message(server, token, me, u)
        first = False


def on_message(server, token, me, u):
    who, text = u["from"], u["text"]
    ts = time.strftime("%H:%M", time.localtime(u.get("ts") or time.time()))
    if who not in state["seen"]:
        state["seen"].append(who)
    if state["to"] is None:
        state["to"] = who
        set_prompt()
    c = color_for(who)
    say(f"{DIM}{ts}{RESET} {c}{BOLD}@{who}{RESET}: {text}")
    if text.strip() == "/start":
        send(server, token, who, f"Hi! You're talking to @{me} — a console on the other end reads this.")
    if state["room"]:
        if text.strip() == "/join":
            state["members"].add(who)
            send(server, token, who, "Joined the room. /leave to stop.")
            return
        if text.strip() == "/leave":
            state["members"].discard(who)
            send(server, token, who, "Left the room.")
            return
        for m in list(state["members"]):
            if m != who:
                send(server, token, m, f"@{who}: {text}")


def set_prompt():
    to = state["to"]
    if state["room"]:
        state["prompt"] = f"{BOLD}[room]{RESET} > "
    elif to:
        state["prompt"] = f"{color_for(to)}@{to}{RESET} > "
    else:
        state["prompt"] = "> "


def main():
    ap = argparse.ArgumentParser(description="Read and write an Aquamarine chat from the terminal (as a bot).")
    ap.add_argument("--server", default=os.environ.get("AQUA_SERVER", "http://127.0.0.1:8080"),
                    help="relay URL (default: $AQUA_SERVER or http://127.0.0.1:8080)")
    ap.add_argument("--chat", help="start writing to this @user")
    ap.add_argument("--room", action="store_true", help="room mode: forward each message to everyone who /join-ed")
    a = ap.parse_args()

    if os.name == "nt":
        os.system("")  # turn on ANSI colours in the Windows console

    token = os.environ.get("AQUA_BOT_TOKEN") or getpass.getpass("Bot token (aqbot_…): ").strip()
    try:
        me = api(a.server, token, "/v1/bot/me")
    except urllib.error.HTTPError as e:
        sys.exit("The relay rejected the token." if e.code == 401 else f"Relay error: {e.code}")
    except Exception as e:
        sys.exit(f"Can't reach {a.server}: {e}")

    name = me["username"]
    print(f"{BOLD}Connected as @{name}{RESET} ({me.get('display_name') or name}) on {a.server}")
    print(f"{DIM}Write to @{name} in Aquamarine. /to <user>, /who, /room, /quit. Not end-to-end encrypted.{RESET}")
    state["to"] = a.chat.lstrip("@").lower() if a.chat else None
    state["room"] = a.room
    set_prompt()

    stop = threading.Event()
    threading.Thread(target=poll, args=(a.server, token, name, stop), daemon=True).start()

    while True:
        try:
            with out_lock:
                sys.stdout.write(state["prompt"])
                sys.stdout.flush()
            line = sys.stdin.readline()
        except KeyboardInterrupt:
            break
        if not line:
            break
        line = line.rstrip("\n")
        if not line.strip():
            continue
        cmd = line.strip().split()
        if cmd[0] == "/quit":
            break
        if cmd[0] == "/to" and len(cmd) > 1:
            state["to"] = cmd[1].lstrip("@").lower()
            state["room"] = False
            set_prompt()
            continue
        if cmd[0] == "/who":
            print(", ".join("@" + w for w in state["seen"]) or "nobody yet")
            continue
        if cmd[0] == "/room":
            state["room"] = not state["room"]
            print("room mode " + ("on — people join by writing /join" if state["room"] else "off"))
            set_prompt()
            continue
        targets = sorted(state["members"]) if state["room"] else ([state["to"]] if state["to"] else [])
        if not targets:
            print("Nobody to write to yet — wait for a message or use /to <user>.")
            continue
        for t in targets:
            try:
                send(a.server, token, t, line)
            except Exception as e:
                print(f"\033[91mnot sent to @{t}: {e}\033[0m")
    stop.set()
    print()


if __name__ == "__main__":
    main()
