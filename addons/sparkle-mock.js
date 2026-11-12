// @name Sparkle & Mock
// @version 1.0
// @author Aquamarine
// @description /sparkle wraps text in ✨, /mock makes tExT lIkE tHiS, and "->" becomes an arrow.

aqua.register({ name: "Sparkle & Mock" });

aqua.command("sparkle", "✨ text ✨", (args) => "✨ " + (args || "✨") + " ✨");

aqua.command("mock", "tExT lIkE tHiS", (args) =>
  [...(args || "")].map((c, i) => (i % 2 ? c.toUpperCase() : c.toLowerCase())).join(""));

aqua.onSend((msg) => msg.text.replace(/->/g, "→"));
