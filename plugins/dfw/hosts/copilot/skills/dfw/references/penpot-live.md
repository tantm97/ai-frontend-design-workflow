# Penpot bridge: live comments

Comment threads in Penpot become dfw comments, and the agent's answers come back as replies in the thread. Overview, setup and privacy: `references/penpot.md`.

**Live comments** are on by default while the plugin panel is open: every few seconds the plugin sends the comment threads that sit on dfw boards and main components to the Studio. This is the same loop as Studio comments, inside Penpot:

- a new thread becomes one dfw comment (`provider: "penpot"`), and the agent is notified at once;
- what the agent does comes back as replies in the same thread, prefixed `[dfw]`: seen (`dfw comments ack`), fixed with the note, dismissed, or a question (`dfw comments reply`);
- a new message from the designer in the thread is added to the same dfw comment, and reopens it when it was fixed, resolved or dismissed;
- resolving the thread in Penpot accepts a fix (`resolved`, by the user), or dismisses a comment that was not fixed yet; un-resolving it reopens.

Threads elsewhere in the file are ignored. Replies are posted by the plugin as the Penpot user who has the panel open (Penpot has no bot identity), hence the `[dfw]` prefix. The link between a thread and its comment is kept in `.dfw/penpot/threads.json` (`<file id>#<thread number>`).

After a fix, the panel says the design changed; **Update boards** redraws the boards in place, so their comment threads stay (see `references/penpot-export.md`). Layer edits are not live: they come back with **Send changes to dfw** (see `references/penpot-import.md`).

Not verified without a running Penpot: comment thread positions (absolute vs board-relative), and whether `CommentThread.board` is the main component when a pin sits on the `dfw components` page.
