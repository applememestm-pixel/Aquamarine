"""Low-level client of the Aquamarine Bot API (docs/BOTS.md).

>>> api = Client("https://relay.example", token)
>>> api.me()
{'username': 'weatherbot', 'display_name': 'Weather'}
>>> for u in api.updates(after=0, timeout=25): ...
>>> api.send("alice", "hi")
"""
from __future__ import annotations

import os
import time
from dataclasses import dataclass, field
from typing import Any, Dict, List, Optional, Sequence, Union

from ._http import HTTPError, request

#: The relay refuses longer texts (413).
MAX_TEXT = 4096

#: ``["Yes", "No"]``, or rows of strings / ``{"text", "data" | "url" | "app"}``.
Buttons = Sequence[Any]


class BotError(Exception):
    """Base class of the errors below."""


class Unauthorized(BotError):
    """401: the token is wrong or was revoked. Stop the bot."""


class Forbidden(BotError):
    """403: the user hasn't written to the bot yet, or sent /stop."""


class RateLimited(BotError):
    """429 after all retries."""


@dataclass
class Update:
    """One incoming message."""

    id: int
    sender: str
    text: str
    ts: int = 0
    raw: dict = field(default_factory=dict, repr=False)
    #: A button of the bot's keyboard was pressed: its ``data`` (else None).
    callback: Optional[str] = None
    #: The message that keyboard belonged to.
    callback_message: Optional[int] = None

    @property
    def is_callback(self) -> bool:
        return self.callback is not None

    @property
    def is_command(self) -> bool:
        return self.text.startswith("/")

    @property
    def command(self) -> str:
        """"/ask@bot hello" → "ask" ("" when not a command)."""
        if not self.is_command:
            return ""
        return self.text[1:].split(maxsplit=1)[0].split("@", 1)[0].lower() if len(self.text) > 1 else ""

    @property
    def args(self) -> str:
        """Text after the command ("/ask hello there" → "hello there")."""
        parts = self.text.split(maxsplit=1)
        return parts[1].strip() if self.is_command and len(parts) > 1 else ("" if self.is_command else self.text)


def split_text(text: str, limit: int = MAX_TEXT) -> List[str]:
    """Cut a long text into pieces ≤ limit, preferring paragraph, line and word breaks."""
    text = text or ""
    out: List[str] = []
    while len(text) > limit:
        cut = -1
        for sep in ("\n\n", "\n", ". ", " "):
            cut = text.rfind(sep, 0, limit)
            if cut > limit // 2:
                cut += len(sep)
                break
        if cut <= 0:
            cut = limit
        out.append(text[:cut].rstrip())
        text = text[cut:].lstrip()
    if text:
        out.append(text)
    return out or [""]


class Client:
    """Thin, retrying wrapper over the HTTP API.

    ``server`` defaults to ``$AQUA_SERVER``, ``token`` to ``$AQUA_BOT_TOKEN``.
    """

    def __init__(self, server: Optional[str] = None, token: Optional[str] = None, *, retries: int = 4):
        self.server = (server or os.environ.get("AQUA_SERVER") or "http://127.0.0.1:7777").rstrip("/")
        self.token = token or os.environ.get("AQUA_BOT_TOKEN") or ""
        if not self.token:
            raise Unauthorized("no bot token: pass token=… or set AQUA_BOT_TOKEN")
        self.retries = retries

    # -- plumbing --------------------------------------------------------------
    def _call(self, method: str, path: str, body=None, timeout: float = 40):
        url = self.server + path
        delay = 1.1
        for attempt in range(self.retries + 1):
            try:
                return request(method, url, headers={"Authorization": "Bot " + self.token}, body=body, timeout=timeout)
            except HTTPError as e:
                if e.status == 401:
                    raise Unauthorized(e.body or "token rejected") from None
                if e.status == 403:
                    raise Forbidden(e.body or "not allowed") from None
                if e.status == 429 and attempt < self.retries:
                    time.sleep(delay)
                    delay = min(delay * 1.6, 8)
                    continue
                if e.status == 429:
                    raise RateLimited(e.body) from None
                if e.status >= 500 and attempt < self.retries:
                    time.sleep(delay)
                    continue
                raise
        raise RateLimited("gave up")

    # -- API -------------------------------------------------------------------
    def me(self) -> dict:
        """``{"username", "display_name"}`` — also checks the token."""
        return self._call("GET", "/v1/bot/me")

    def updates(self, after: int = 0, timeout: int = 25) -> List[Update]:
        """Long-poll for new messages. ``after`` acknowledges everything before it."""
        timeout = max(0, min(int(timeout), 30))
        rows = self._call("GET", f"/v1/bot/updates?after={int(after)}&timeout={timeout}", timeout=timeout + 15) or []
        return [Update(id=int(r.get("id", 0)), sender=str(r.get("from", "")), text=str(r.get("text", "")),
                       ts=int(r.get("ts", 0) or 0), raw=r,
                       callback=r.get("callback"), callback_message=r.get("callback_msg")) for r in rows]

    def send(self, to: str, text: str, *, parse_mode: Optional[str] = None,
             buttons: Optional[Buttons] = None, keyboard: Optional[Buttons] = None,
             one_time: bool = False, remove_keyboard: bool = False) -> int:
        """Send ``text`` (split if longer than 4096). Returns how many messages went out.

        ``parse_mode``: "Markdown", "MarkdownV2" or "HTML".
        ``buttons``: under the message — ``["Yes", "No"]`` or rows
        ``[[{"text": "Open", "url": "https://…"}], [{"text": "Pay", "data": "pay"}]]``;
        a pressed button arrives as an update with ``callback`` = its data.
        ``keyboard``: buttons over the message box that send their text.
        """
        parts = [p for p in split_text(text) if p.strip()]
        extra: Dict[str, Any] = {}
        if parse_mode:
            extra["parse_mode"] = parse_mode
        if remove_keyboard:
            extra["remove_keyboard"] = True
        for i, part in enumerate(parts):
            body: Dict[str, Any] = {"to": to, "text": part, **extra}
            if i + 1 == len(parts):          # buttons go on the last piece
                if buttons is not None:
                    body["buttons"] = buttons
                if keyboard is not None:
                    body["keyboard"] = keyboard
                    body["one_time"] = one_time
            self._call("POST", "/v1/bot/send", body=body)
            if i + 1 < len(parts):
                time.sleep(1.05)   # 1 message/s per recipient (burst 5)
        return len(parts)

    def edit(self, to: str, text: str, *, message_id: Optional[int] = None,
             parse_mode: Optional[str] = None, buttons: Optional[Buttons] = None) -> Optional[int]:
        """A new version of a message (shown as an edit). Returns the new message id."""
        body: Dict[str, Any] = {"to": to, "text": text}
        if message_id is not None:
            body["message_id"] = message_id
        if parse_mode:
            body["parse_mode"] = parse_mode
        if buttons is not None:
            body["buttons"] = buttons
        r = self._call("POST", "/v1/bot/edit", body=body) or {}
        return r.get("message_id")

    def broadcast(self, text: str, *, parse_mode: Optional[str] = None, buttons: Optional[Buttons] = None) -> int:
        """Send one message to every contact (in the background, at the rate limit). Returns how many."""
        body: Dict[str, Any] = {"text": text}
        if parse_mode:
            body["parse_mode"] = parse_mode
        if buttons is not None:
            body["buttons"] = buttons
        return int((self._call("POST", "/v1/bot/broadcast", body=body) or {}).get("queued", 0))

    def contacts(self) -> List[dict]:
        """Everyone who wrote to the bot and didn't /stop: ``[{username, display_name, id}]``."""
        return self._call("GET", "/v1/bot/contacts") or []

    def user(self, name: str) -> dict:
        """``{username, display_name, id, is_bot, is_contact}``."""
        return self._call("GET", "/v1/bot/user/" + name.lstrip("@"))

    def commands(self) -> List[dict]:
        return self._call("GET", "/v1/bot/commands") or []

    def set_commands(self, commands: Union[Dict[str, str], Sequence[dict]]) -> None:
        """The "/" menu users see: ``{"help": "what I can do", …}``."""
        self._call("POST", "/v1/bot/commands", body=commands if isinstance(commands, list) else dict(commands))

    def delete_commands(self) -> None:
        self._call("DELETE", "/v1/bot/commands")

    def profile(self) -> dict:
        return self._call("GET", "/v1/bot/profile")

    def set_profile(self, *, description: Optional[str] = None, short_description: Optional[str] = None) -> None:
        body = {k: v for k, v in (("description", description), ("short_description", short_description)) if v is not None}
        self._call("POST", "/v1/bot/profile", body=body)

    def stats(self) -> dict:
        """``{username, contacts, pending_updates, webhook, commands}``."""
        return self._call("GET", "/v1/bot/stats")

    def set_webhook(self, url: str, *, secret: Optional[str] = None, drop_pending: bool = False) -> None:
        """Push updates to ``url`` instead of polling (``X-Telegram-Bot-Api-Secret-Token`` carries ``secret``)."""
        body: Dict[str, Any] = {"url": url, "drop_pending": drop_pending}
        if secret:
            body["secret"] = secret
        self._call("POST", "/v1/bot/webhook", body=body)

    def delete_webhook(self) -> None:
        self._call("DELETE", "/v1/bot/webhook")

    def webhook(self) -> dict:
        return self._call("GET", "/v1/bot/webhook")

    def call(self, method: str, **params: Any) -> Any:
        """Any Telegram Bot API method (``getMe``, ``sendPoll``, ``answerCallbackQuery``…). Returns its ``result``."""
        r = self._call("POST", "/v1/bot/call/" + method, body=params) or {}
        if isinstance(r, dict) and r.get("ok") is False:
            raise BotError(r.get("description", "error"))
        return r.get("result") if isinstance(r, dict) else r
