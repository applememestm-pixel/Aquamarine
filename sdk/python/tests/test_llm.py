import json
import unittest

from aquabot import llm

from _fake import Fake


def openai_handler(method, path, headers, body):
    if path == "/v1/models":
        return 200, {"data": [{"id": "qwen"}, {"id": "llama"}]}, None
    if path == "/health":
        return 200, {"status": "ok"}, None
    if path == "/tokenize":
        return 200, {"tokens": [1, 2, 3]}, None
    if path == "/completion":
        return 200, {"content": "raw:" + body["prompt"]}, None
    if path == "/v1/embeddings":
        return 200, {"data": [{"embedding": [0.1, 0.2]}]}, None
    if path == "/v1/chat/completions":
        last = body["messages"][-1]["content"]
        if body.get("stream"):
            chunks = [f'data: {json.dumps({"choices": [{"delta": {"content": w}}]})}\n\n' for w in ["he", "llo ", last]]
            return 200, ("".join(chunks) + "data: [DONE]\n\n").encode(), "text/event-stream"
        return 200, {"choices": [{"message": {"content": f"{body.get('model', '')}|{last}|{body.get('temperature')}"}}]}, None
    return 404, {}, None


def ollama_handler(method, path, headers, body):
    if path == "/api/tags":
        return 200, {"models": [{"name": "llama3.1"}]}, None
    if path == "/api/chat":
        last = body["messages"][-1]["content"]
        if body["stream"]:
            lines = [json.dumps({"message": {"content": p}, "done": False}) for p in ["a", "b"]]
            lines.append(json.dumps({"done": True}))
            return 200, ("\n".join(lines) + "\n").encode(), "application/x-ndjson"
        return 200, {"message": {"role": "assistant", "content": f"{body['model']}:{last}:{body['options'].get('num_predict')}"}}, None
    return 404, {}, None


def anthropic_handler(method, path, headers, body):
    if headers.get("x-api-key") != "k" or headers.get("anthropic-version") != "2023-06-01":
        return 401, {"error": "auth"}, None
    if body["stream"]:
        ev = [{"type": "message_start"}, {"type": "content_block_delta", "delta": {"type": "text_delta", "text": "Hi"}},
              {"type": "content_block_delta", "delta": {"type": "text_delta", "text": "!"}}, {"type": "message_stop"}]
        return 200, "".join(f"event: x\ndata: {json.dumps(e)}\n\n" for e in ev).encode(), "text/event-stream"
    return 200, {"content": [{"type": "text", "text": f"{body.get('system')}/{body['messages'][-1]['content']}"}]}, None


class LLMTests(unittest.TestCase):
    def setUp(self):
        self.servers = []

    def tearDown(self):
        for s in self.servers:
            s.close()

    def srv(self, h):
        s = Fake(h)
        self.servers.append(s)
        return s

    def test_llamacpp(self):
        s = self.srv(openai_handler)
        m = llm.LlamaCpp(s.url, temperature=0.2)
        self.assertTrue(m.health())
        self.assertEqual(m.ask("q"), "|q|0.2")
        self.assertEqual("".join(m.stream([{"role": "user", "content": "x"}])), "hello x")
        self.assertEqual(m.tokenize("abc"), [1, 2, 3])
        self.assertEqual(m.complete("2+2="), "raw:2+2=")
        self.assertEqual(m.models(), ["qwen", "llama"])
        self.assertEqual(m.embed("t"), [0.1, 0.2])

    def test_openai_compatible_and_connect(self):
        s = self.srv(openai_handler)
        m = llm.connect(f"openai-compatible:{s.url}/v1:qwen")
        self.assertIsInstance(m, llm.OpenAICompatible)
        self.assertEqual(m.ask("q"), "qwen|q|0.7")
        auth = [c for c in s.calls if c[1] == "/v1/chat/completions"]
        self.assertNotIn("authorization", auth[-1][2])
        k = llm.OpenAICompatible(s.url, "m", api_key="sk")
        k.ask("x")
        self.assertEqual(s.calls[-1][2]["authorization"], "Bearer sk")

    def test_ollama(self):
        s = self.srv(ollama_handler)
        m = llm.Ollama("llama3.1", s.url, max_tokens=50)
        self.assertEqual(m.ask("q"), "llama3.1:q:50")
        self.assertEqual("".join(m.stream([{"role": "user", "content": "x"}])), "ab")
        self.assertEqual(m.models(), ["llama3.1"])

    def test_anthropic(self):
        s = self.srv(anthropic_handler)
        m = llm.Anthropic("some-model", api_key="k", url=s.url)
        self.assertEqual(m.ask("q", system="S"), "S/q")
        self.assertEqual("".join(m.stream([{"role": "user", "content": "x"}])), "Hi!")
        bad = llm.Anthropic("some-model", api_key="nope", url=s.url)
        with self.assertRaises(llm.LLMError):
            bad.ask("q")

    def test_chat_session_memory(self):
        s = self.srv(openai_handler)
        sess = llm.ChatSession(llm.LlamaCpp(s.url), system="sys", max_turns=2)
        for t in ["a", "b", "c"]:
            sess.ask(t)
        msgs = s.calls[-1][3]["messages"]
        self.assertEqual(msgs[0], {"role": "system", "content": "sys"})
        self.assertEqual(len(msgs), 1 + 2 * 2 + 1)   # system + 2 turns + new question
        self.assertEqual("".join(sess.stream("d")), "hello d")
        self.assertEqual(sess.history[-1]["content"], "hello d")
        sess.reset()
        self.assertEqual(sess.history, [])

    def test_unreachable(self):
        self.assertFalse(llm.LlamaCpp("http://127.0.0.1:9").health())
        with self.assertRaises(llm.LLMError):
            llm.OpenAICompatible("http://127.0.0.1:9", timeout=2).chat([{"role": "user", "content": "x"}])

    def test_connect_specs(self):
        self.assertIsInstance(llm.connect("llamacpp"), llm.LlamaCpp)
        self.assertEqual(llm.connect("ollama:qwen2.5").model, "qwen2.5")
        self.assertEqual(llm.connect("llamacpp:http://box:8080").base, "http://box:8080/v1")
        with self.assertRaises(llm.LLMError):
            llm.connect("nope")


if __name__ == "__main__":
    unittest.main()
