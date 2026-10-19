import unittest

from aquabot import Bot, Client, Forbidden, Unauthorized, split_text
from aquabot.llm import LLM

from _fake import Fake


class FakeAPI:
    def __init__(self, updates):
        self.queue = list(updates)
        self.sent = []
        self.fail_send = set()

    def __call__(self, method, path, headers, body):
        if headers.get("authorization") != "Bot good":
            return 401, {"error": "bad token"}, None
        if path == "/v1/bot/me":
            return 200, {"username": "testbot", "display_name": "Test"}, None
        if path.startswith("/v1/bot/updates"):
            after = int(path.split("after=")[1].split("&")[0])
            rows = [u for u in self.queue if u["id"] >= after]
            return 200, rows, None
        if path == "/v1/bot/send":
            if body["to"] in self.fail_send:
                return 403, {"error": "user hasn't started the bot"}, None
            self.sent.append((body["to"], body["text"]))
            return 200, {"id": len(self.sent)}, None
        return 404, {}, None


class Echo(LLM):
    def chat(self, messages, **opts):
        return f"[{len(messages)}] {messages[-1]['content']}"


def U(i, sender, text):
    return {"id": i, "from": sender, "text": text, "ts": 0}


class BotTests(unittest.TestCase):
    def run_bot(self, updates, setup):
        api = FakeAPI(updates)
        srv = Fake(api)
        try:
            bot = Bot(srv.url, "good", workers=2)
            setup(bot)
            bot.run(once=True)
            return api, bot
        finally:
            srv.close()

    def test_commands_messages_default(self):
        def setup(bot):
            @bot.command("hi", help="greet")
            def hi(ctx):
                ctx.reply(f"hi {ctx.sender} {ctx.args}")

            @bot.message(r"ping")
            def ping(ctx):
                ctx.reply("pong")

            @bot.default
            def other(ctx):
                ctx.reply("?")

            bot.use_help()

        api, bot = self.run_bot([U(1, "ann", "/hi there"), U(2, "bob", "ping!"), U(3, "ann", "zzz"),
                                 U(4, "ann", "/help")], setup)
        sent = dict()
        for to, text in api.sent:
            sent.setdefault(to, []).append(text)
        self.assertEqual(sent["bob"], ["pong"])
        self.assertEqual(sent["ann"][:2], ["hi ann there", "?"])
        self.assertIn("/hi", sent["ann"][2])
        self.assertEqual(bot.username, "testbot")
        self.assertEqual(bot.after, 5)

    def test_calc_and_llm(self):
        def setup(bot):
            bot.use_calc().use_llm(Echo(), system="sys")

        api, _ = self.run_bot([U(1, "ann", "/calc 2^10 + 5!"), U(2, "ann", "hello"), U(3, "ann", "again"),
                               U(4, "ann", "/reset"), U(5, "ann", "fresh"), U(6, "ann", "/calc 1/0")], setup)
        texts = [t for _, t in api.sent]
        self.assertEqual(texts[0], "2^10 + 5! = 1144")
        self.assertEqual(texts[1], "[2] hello")      # system + user
        self.assertEqual(texts[2], "[4] again")      # system + 2 turns + user
        self.assertEqual(texts[4], "[2] fresh")
        self.assertIn("division by zero", texts[5])

    def test_forbidden_and_unauthorized(self):
        api = FakeAPI([])
        api.fail_send.add("carol")
        srv = Fake(api)
        try:
            c = Client(srv.url, "good", retries=0)
            with self.assertRaises(Forbidden):
                c.send("carol", "x")
            self.assertFalse(Bot(srv.url, "good").send("carol", "x"))
            with self.assertRaises(Unauthorized):
                Client(srv.url, "bad").me()
        finally:
            srv.close()

    def test_long_text_is_split(self):
        parts = split_text("word " * 2000, 4096)
        self.assertTrue(all(len(p) <= 4096 for p in parts))
        self.assertEqual(" ".join(p.strip() for p in parts).split(), ["word"] * 2000)

    def test_handler_error_is_reported(self):
        def setup(bot):
            @bot.command("boom")
            def boom(ctx):
                raise RuntimeError("kaboom")

            @bot.on_error
            def err(ctx, e):
                ctx.reply(f"error: {e}")

        api, _ = self.run_bot([U(1, "ann", "/boom")], setup)
        self.assertEqual(api.sent, [("ann", "error: kaboom")])


if __name__ == "__main__":
    unittest.main()
