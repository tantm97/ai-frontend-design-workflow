// @ts-check
// The Studio server's paths that the browser code calls, defined once. A `/__dfw/` path written
// anywhere else in app/, overlay/ or live/ is refused by the lint (eslint.config.js). The server
// side of each route is native/src/studio/server.rs.

export const PATH = Object.freeze({
  /** The Design System page shown in its own frame. */
  SYSTEM_PAGE: '/__dfw/system.html',
  /** Server-Sent Events: `design`, `presence`, `comments`. */
  EVENTS: '/__dfw/events',
  MANIFEST: '/__dfw/api/manifest',
  STATE: '/__dfw/api/state',
  TOKENS: '/__dfw/api/tokens',
  COMPONENTS: '/__dfw/api/components',
  COMMENTS: '/__dfw/api/comments',
  /** Hand every draft comment to the agent (POST). */
  COMMENTS_SEND: '/__dfw/api/comments/send',
  /** Whether a `dfw comments wait` listens for this project (GET). */
  LISTENING: '/__dfw/api/listening',
  SESSIONS: '/__dfw/api/sessions',
  SHOOT: '/__dfw/api/shoot',
  EDIT: '/__dfw/api/edit',
  UNDO: '/__dfw/api/undo',
});

/** The address of one comment (PATCH). */
export const commentPath = (/** @type {string} */ id) => `${PATH.COMMENTS}/${encodeURIComponent(id)}`;
