"""Command line:

    aquabot calc "2^10 + sqrt(16)"        # the built-in calculator
    aquabot ask "Why is the sky blue?"    # one question to $AQUA_LLM (default: local llama.cpp)
    aquabot chat                          # talk to the model in the terminal
    aquabot run                           # a ready AI bot: /help /calc /ask /reset + chat
    aquabot models                        # list models on the LLM server

Options: --llm SPEC (e.g. llamacpp, ollama:llama3.1, openai:gpt-4o-mini), --system TEXT,
--server URL / --token TOKEN for ``run`` (or $AQUA_SERVER / $AQUA_BOT_TOKEN).
"""
from __future__ import annotations

import argparse
import logging
import sys

from . import __version__, llm, mathx


def main(argv=None) -> int:
    p = argparse.ArgumentParser(prog="aquabot", description="Aquamarine bot toolkit")
    p.add_argument("--version", action="version", version=f"aquabot {__version__}")
    p.add_argument("--llm", help="model spec (default: $AQUA_LLM or llamacpp)")
    p.add_argument("--system", default="You are a helpful assistant in the Aquamarine messenger. Answer briefly.")
    sub = p.add_subparsers(dest="cmd", required=True)
    c = sub.add_parser("calc", help="evaluate an expression")
    c.add_argument("expr", nargs="+")
    a = sub.add_parser("ask", help="ask the model once (streams the answer)")
    a.add_argument("prompt", nargs="+")
    sub.add_parser("chat", help="interactive chat with the model")
    sub.add_parser("models", help="list models on the server")
    r = sub.add_parser("run", help="run an AI + calculator bot")
    r.add_argument("--server")
    r.add_argument("--token")
    r.add_argument("--no-chat", action="store_true", help="answer only /ask, not every message")
    r.add_argument("-v", "--verbose", action="store_true")
    args = p.parse_args(argv)

    try:
        if args.cmd == "calc":
            print(mathx.fmt(mathx.calc(" ".join(args.expr))))
            return 0
        model = llm.connect(args.llm)
        if args.cmd == "ask":
            for piece in model.stream([{"role": "system", "content": args.system},
                                       {"role": "user", "content": " ".join(args.prompt)}]):
                sys.stdout.write(piece)
                sys.stdout.flush()
            print()
        elif args.cmd == "chat":
            s = llm.ChatSession(model, system=args.system)
            print("Ctrl+D to quit, /reset to start over.")
            while True:
                try:
                    line = input("> ").strip()
                except (EOFError, KeyboardInterrupt):
                    print()
                    break
                if not line:
                    continue
                if line == "/reset":
                    s.reset()
                    continue
                for piece in s.stream(line):
                    sys.stdout.write(piece)
                    sys.stdout.flush()
                print()
        elif args.cmd == "models":
            lister = getattr(model, "models", None)
            if not lister:
                print("this provider has no model list", file=sys.stderr)
                return 1
            print("\n".join(lister()))
        elif args.cmd == "run":
            from .bot import Bot
            logging.basicConfig(level=logging.DEBUG if args.verbose else logging.INFO,
                                format="%(asctime)s %(levelname)s %(message)s")
            bot = Bot(args.server, args.token)
            bot.use_help("I'm an AI assistant. Just write to me.").use_calc()
            bot.use_llm(model, system=args.system, chat=not args.no_chat)
            try:
                bot.run()
            except KeyboardInterrupt:
                bot.stop()
    except (mathx.CalcError, llm.LLMError) as e:
        print(f"error: {e}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
