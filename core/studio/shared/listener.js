// @ts-check
// What the Studio's send bar and the Penpot bridge panel say about the AI, defined once: both read
// `{ listening, working }` (the Studio's listening route, the panel's sync answer) and show the
// same three states with the same words.
import { LISTENER } from './enums.js';
import { TEXT } from './text.js';

/**
 * The listener state from an answer that carries `{ listening, working }`; anything else is
 * "off".
 * @param {unknown} r
 */
export function listenerState(r) {
  const a = /** @type {{ listening?: unknown, working?: unknown } | null} */ (r && typeof r === 'object' ? r : null);
  if (a?.listening === true) return LISTENER.LISTENING;
  if (a?.working === true) return LISTENER.WORKING;
  return LISTENER.OFF;
}

/**
 * What each listener state says (markup: `<b>` names the state; constant strings only).
 * @type {Map<string, string>}
 */
const LISTENER_HTML = new Map([
  [LISTENER.LISTENING, TEXT.listeningHtml],
  [LISTENER.WORKING, TEXT.workingHtml],
  [LISTENER.OFF, TEXT.notListeningHtml],
]);

/**
 * The markup for a listener state; an unknown state reads as "not listening".
 * @param {string} state
 * @returns {string}
 */
export const listenerHtml = (state) => LISTENER_HTML.get(state) ?? TEXT.notListeningHtml;
