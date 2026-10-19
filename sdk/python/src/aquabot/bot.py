"""High-level bot framework: commands, handlers, per-user state, LLM chat.

    from aquabot import Bot

    bot = Bot()                                  # $AQUA_SERVER, $AQUA_BOT_TOKEN

    @bot.command("start", help="say hello")
    def start(ctx):
        ctx.reply(f"Hi, {ctx.sender}!")

    @bot.message(r"(?i)\\bping\\b")
    def ping(ctx):
        ctx.reply("pong")

    bot.run()

Handlers run in a small thread pool; messages from one user are handled in
order, one at a time, so per-user state needs no locking.
"""
from __future__ import annotations

import logging
import re
import threading
import time
from concurrent.futures import ThreadPoolExecutor
from dataclasses import dataclass, field
from typing import Any, Callable, Dict, List, Optional, Pattern, Union

from .client import Client, Forbidden, Unauthorized, Update

log = logging.getLogger("aquabot")

Handler = Callable[["Context"], Any]


@dataclass
class Context:
    """What a handler gets: the update, a way to answer, and per-user state."""

    bot: "Bot"
    update: Update
    match: Optional[re.Match] = None
    #: Free-form per-user storage (kept in memory while the bot runs).
    state: Dict[str, Any] = field(default_factory=dict)

    @property
    def sender(self) -> str:
        return self.update.sender

    @property
    def text(self) -> str:
        return self.update.text

    @property
    def command(self) -> str:
        return self.update.command

    @property
    def args(self) -> str:
        return self.update.args

    def reply(self, text: Any) -> None:
        """Answer the sender (long texts are split; /stop'ed users are skipped)."""
        self.bot.send(self.sender, str(text))


@dataclass
class _Route:
    fn: Handler
    pattern: Optional[Pattern] = None
    when: Optional[Callable[[Update], bool]] = None


class Bot:
    """A bot. ``server``/``token`` default to ``$AQUA_SERVER``/``$AQUA_BOT_TOKEN``."""

    def __init__(self, server: Optional[str] = None, token: Optional[str] = None, *,
                 client: Optional[Client] = None, workers: int = 8, poll_timeout: int = 25):
        self.api = client or Client(server, token)
        self.workers = workers
        self.poll_timeout = poll_timeout
        self._commands: Dict[str, Handler] = {}
        self._help: Dict[str, str] = {}
        self._routes: List[_Route] = []
        self._fallback: Optional[Handler] = None
        self._on_error: Optional[Callable[[Context, BaseException], Any]] = None
        self._states: Dict[str, Dict[str, Any]] = {}
        self._locks: Dict[str, threading.Lock] = {}
        self._locks_guard = threading.Lock()
        self._stop = threading.Event()
        self.after = 0
        self.username = ""

    # ---- registration -------------------------------------------------------
    def command(self, *names: str, help: str = "") -> Callable[[Handler], Handler]:
        """``@bot.command("start", "hello", help="…")`` — for "/start", "/hello"."""
        def deco(fn: Handler) -> Handler:
            for n in names or (fn.__name__,):
                self._commands[n.lower().lstrip("/")] = fn
            if help:
                self._help[(names or (fn.__name__,))[0].lower().lstrip("/")] = help
            return fn
        return deco

    def message(self, pattern: Union[str, Pattern, None] = None, *,
                when: Optional[Callable[[Update], bool]] = None) -> Callable[[Handler], Handler]:
        """Plain messages matching a regex (``ctx.match``) and/or a predicate. First match wins."""
        rx = re.compile(pattern) if isinstance(pattern, str) else pattern

        def deco(fn: Handler) -> Handler:
            self._routes.append(_Route(fn, rx, when))
            return fn
        return deco

    def default(self, fn: Handler) -> Handler:
        """Anything nothing else handled (unknown commands included)."""
        self._fallback = fn
        return fn

    def on_error(self, fn: Callable[[Context, BaseException], Any]):
        """Called with (ctx, exception) when a handler raises."""
        self._on_error = fn
        return fn

    # ---- batteries ------------------------------------------------------------
    def use_help(self, intro: str = "") -> "Bot":
        """/help listing the commands that were given ``help=``."""
        @self.command("help")
        def _help(ctx: Context):
            lines = [intro] if intro else []
            lines += [f"/{k} — {v}" for k, v in sorted(self._help.items())]
            ctx.reply("\n".join(lines) or "No commands.")
        return self

    def use_calc(self, command: str = "calc") -> "Bot":
        """``/calc 2^10 + sqrt(16)`` with the safe evaluator from :mod:`aquabot.mathx`."""
        from . import mathx

        @self.command(command, help="calculator: /calc sin(pi/6) + 2^10, sqrt, log, fact, mean(1,2,3)…")
        def _calc(ctx: Context):
            expr = ctx.args
            if not expr:
                ctx.reply("Usage: /calc expression — e.g. /calc (1+2)*3^2, sqrt(2), log(100, 10), mean(1, 2, 3)")
                return
            try:
                ctx.reply(f"{expr} = {mathx.fmt(mathx.calc(expr))}")
            except mathx.CalcError as e:
                ctx.reply(f"Can't compute: {e}")
        return self

    def use_llm(self, llm, *, system: str = "You are a helpful assistant in the Aquamarine messenger.",
                command: Optional[str] = "ask", chat: bool = True, max_turns: int = 12,
                reset_command: str = "reset") -> "Bot":
        """Answer with a language model (:mod:`aquabot.llm`).

        ``chat=True``: every plain message is a turn of a per-user conversation.
        ``command``: also ``/ask question``. ``/reset`` forgets the history.
        """
        from .llm import ChatSession

        def session(ctx: Context) -> ChatSession:
            s = ctx.state.get("_llm")
            if s is None:
                s = ctx.state["_llm"] = ChatSession(llm, system=system, max_turns=max_turns)
            return s

        def answer(ctx: Context, text: str):
            if not text.strip():
                ctx.reply("Ask me something.")
                return
            ctx.reply(session(ctx).ask(text))

        if command:
            self.command(command, help="ask the AI")(lambda ctx: answer(ctx, ctx.args))
        self.command(reset_command, help="forget the conversation")(
            lambda ctx: (session(ctx).reset(), ctx.reply("Okay, starting over."))
        )
        if chat:
            self.message(when=lambda u: not u.is_command)(lambda ctx: answer(ctx, ctx.text))
        return self

    # ---- sending -------------------------------------------------------------------
    def send(self, to: str, text: str) -> bool:
        """Send to a user; False if they haven't written to the bot or sent /stop."""
        try:
            self.api.send(to, text)
            return True
        except Forbidden:
            log.info("can't write to @%s (no /start or /stop)", to)
            return False

    # ---- dispatch -----------------------------------------------------------------
    def _state(self, user: str) -> Dict[str, Any]:
        return self._states.setdefault(user, {})

    def _lock(self, user: str) -> threading.Lock:
        with self._locks_guard:
            return self._locks.setdefault(user, threading.Lock())

    def dispatch(self, u: Update) -> None:
        """Handle one update (runs the matching handler). Public for tests."""
        with self._lock(u.sender):
            ctx = Context(self, u, state=self._state(u.sender))
            try:
                if u.command == "stop" and "stop" not in self._commands:
                    self._states.pop(u.sender, None)
                    return
                fn: Optional[Handler] = None
                if u.is_command and u.command in self._commands:
                    fn = self._commands[u.command]
                if fn is None:
                    for r in self._routes:
                        if r.when and not r.when(u):
                            continue
                        if r.pattern:
                            m = r.pattern.search(u.text)
                            if not m:
                                continue
                            ctx.match = m
                        fn = r.fn
                        break
                if fn is None:
                    if u.command == "start":
                        ctx.reply(f"Hi! I'm @{self.username or 'a bot'}." + (" Try /help." if "help" in self._commands else ""))
                        return
                    fn = self._fallback
                if fn is not None:
                    fn(ctx)
            except Exception as e:  # noqa: BLE001 — a handler bug must not kill the bot
                log.exception("handler failed for @%s", u.sender)
                if self._on_error:
                    try:
                        self._on_error(ctx, e)
                    except Exception:  # noqa: BLE001
                        log.exception("error handler failed")

    def _dispatch_all(self, updates: List[Update]) -> None:
        for u in updates:
            self.dispatch(u)

    def stop(self) -> None:
        self._stop.set()

    def run(self, *, once: bool = False) -> None:
        """Long-poll and dispatch until :meth:`stop` (or Ctrl+C). ``once``: a single poll (tests)."""
        me = self.api.me()
        self.username = me.get("username", "")
        log.info("@%s running against %s", self.username, self.api.server)
        pool = ThreadPoolExecutor(max_workers=self.workers, thread_name_prefix="aquabot")
        backoff = 1.0
        try:
            while not self._stop.is_set():
                try:
                    batch = self.api.updates(self.after, 0 if once else self.poll_timeout)
                    backoff = 1.0
                except Unauthorized:
                    raise
                except Exception as e:  # noqa: BLE001 — network trouble: wait and retry
                    log.warning("poll failed: %s (retry in %.0fs)", e, backoff)
                    time.sleep(backoff)
                    backoff = min(backoff * 2, 30)
                    continue
                # One task per sender, their messages in order; different senders in parallel.
                by_sender: Dict[str, List[Update]] = {}
                for u in batch:
                    self.after = max(self.after, u.id + 1)
                    by_sender.setdefault(u.sender, []).append(u)
                futures = [pool.submit(self._dispatch_all, us) for us in by_sender.values()]
                if once:
                    for f in futures:
                        f.result()
                    break
        except KeyboardInterrupt:
            log.info("stopping")
        finally:
            pool.shutdown(wait=True)
            if not once:
                # Acknowledge what we handled so a restart doesn't see it again.
                try:
                    self.api.updates(self.after, 0)
                except Exception:  # noqa: BLE001
                    pass
