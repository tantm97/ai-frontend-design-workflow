// @ts-check
// Every call the Studio page makes to its server, one function per route (paths in
// shared/paths.js). Feature modules call these, never `fetch` or a `/__dfw/` string.
import { api } from './state.js';
import { PATH, commentPath, METHOD } from '#studio/shared/paths.js';

/** @param {unknown} body */
const post = (body) => ({ method: METHOD.POST, body: JSON.stringify(body ?? {}) });

export const server = {
  manifest: () => api(PATH.MANIFEST),
  state: () => api(PATH.STATE),
  tokens: () => api(PATH.TOKENS),
  components: () => api(PATH.COMPONENTS),
  comments: () => api(PATH.COMMENTS),
  /** @param {Record<string, any>} body a new comment (saved as a draft by the composer) */
  addComment: (body) => api(PATH.COMMENTS, post(body)),
  /**
   * @param {string} id
   * @param {Record<string, any>} body status change and/or a thread line
   */
  patchComment: (id, body) => api(commentPath(id), { method: METHOD.PATCH, body: JSON.stringify(body) }),
  sendDrafts: () => api(PATH.COMMENTS_SEND, { method: METHOD.POST }),
  listening: () => api(PATH.LISTENING),
  newSession: () => api(PATH.SESSIONS, { method: METHOD.POST }),
  /** @param {Record<string, any>} body */
  shoot: (body) => api(PATH.SHOOT, post(body)),
  /** @param {Record<string, any>} body */
  edit: (body) => api(PATH.EDIT, post(body)),
  undo: () => api(PATH.UNDO, post({})),
};

/** The server's event stream. */
export const events = () => new EventSource(PATH.EVENTS);
