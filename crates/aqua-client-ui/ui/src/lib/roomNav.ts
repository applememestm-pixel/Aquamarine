/** "Open this room of that server" — from the list that a long press on a
 *  server's icon shows (ChatList) to the chat window that owns the rooms. */
import { writable } from "svelte/store";

export const openRoomRequest = writable<{ gid: string; room: string } | null>(null);
