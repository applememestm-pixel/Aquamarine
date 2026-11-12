// @name Random track
// @version 1.0
// @author Aquamarine
// @description 🎲 in the 📎 menu: a random song from your Navidrome, straight into the chat (needs the Navidrome add-on).

aqua.button({ id: "dice", label: "Random track", icon: "🎲" }, async ({ chat }) => {
  const songs = await aqua.music.search("a");
  if (!songs.length) return aqua.toast("Nothing found");
  await aqua.music.send(chat, songs[Math.floor(Math.random() * songs.length)]);
});
