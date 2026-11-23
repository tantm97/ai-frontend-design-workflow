// @ts-check
// Every sentence the Studio shows the user in a toast, a status or a hint, defined once
// (docs/conventions/javascript.md J14). A sentence with values is a function. Static markup
// labels stay in app.html. English only; a translation would replace this one file. The text
// names "the AI", never a product: dfw works with any coding agent (AGENTS.md rule 10).

import { STATUS } from './enums.js';

/** @param {number} n */
const plural = (n) => (n === 1 ? '' : 's');

export const TEXT = Object.freeze({
  // Comments
  draftSaved: (/** @type {string} */ n) => `Draft #${n} saved · press Send to AI when you are done.`,
  draftLabel: 'Draft · not sent yet',
  sendButton: (/** @type {number} */ drafts) => (drafts ? `Send to AI (${drafts})` : 'Send to AI'),
  sentListening: (/** @type {number} */ n) => `Sent ${n} comment${plural(n)} to the AI.`,
  sentNotListening: (/** @type {number} */ n) => `Sent ${n} comment${plural(n)}. The AI gets them with your next message in the terminal.`,
  noDrafts: 'No drafts to send.',
  /** Markup: `<b>` names the state (studio-ui.md U1). */
  listeningHtml: '<b>The AI is listening.</b> Sent comments wake it at once.',
  notListeningHtml: '<b>The AI is not listening.</b> Sent comments reach it with your next message in the terminal.',
  agentWorking: (/** @type {string} */ n) => `The AI is working on #${n}.`,
  agentFixed: (/** @type {string} */ n) => `The AI fixed #${n}: please verify (Comments → To verify).`,
  newRound: 'New review round started.',
  screenNotOnPage: 'That screen is not on any page.',
  frameNotVisible: 'That frame is not visible (check breakpoints and States).',
  statusLabel: Object.freeze({
    [STATUS.OPEN]: 'Open · waiting for the AI',
    [STATUS.ACKNOWLEDGED]: 'AI is working',
    [STATUS.REOPENED]: 'Reopened · waiting for the AI',
    [STATUS.FIXED]: 'Fixed · please verify',
    [STATUS.RESOLVED]: 'Resolved',
    [STATUS.DISMISSED]: 'Dismissed',
  }),
  resolveButton: 'Looks right · resolve',
  reopenButton: 'Not yet · reopen',
  dismissButton: 'Dismiss',
  reopenAgainButton: 'Reopen',
  verifiedInStudio: 'Verified in studio.',
  reasonPlaceholder: 'Why? (sent to the AI)',
  reasonSend: 'Send',
  composerTarget: (/** @type {string} */ where, /** @type {string} */ target) => `${where} → ${target}`,
  // Canvas and toolbar
  zoom: (/** @type {number} */ z) => `${Math.round(z * 100)}%`,
  dims: (/** @type {number} */ w, /** @type {number} */ h) => `${Math.round(w)} × ${Math.round(h)}`,
  /** @param {boolean} all @param {string[]} on */
  statesSummary: (all, on) => `States: ${all ? 'all' : !on.length ? 'off' : on.length <= 2 ? on.join(', ') : on.length}`,
  // Pins
  pinTitle: (/** @type {string} */ n, /** @type {string} */ status) => `Comment #${n} · ${status}`,
  pinLabel: (/** @type {string} */ n, /** @type {string} */ text) => `Comment ${n}: ${text}`,
  pinsHidden: 'Comment pins hidden (Shift+C shows them).',
  pinsAll: 'Showing every pin, closed comments too.',
  pinsActive: 'Showing pins of open and to-verify comments.',
  // Agent presence
  agentLabel: (/** @type {boolean} */ editing, /** @type {string} */ what, /** @type {string} */ more) =>
    `AI ${editing ? 'is editing' : 'changed'} ${what}${more}`.trim(),
  agentWholeFrame: 'this frame',
  // Present
  nothingToPresent: 'Nothing to present on this page.',
  noScreen: (/** @type {string} */ id) => `No screen "${id}".`,
  linkNotAScreen: (/** @type {string} */ to) => `Link target is not a screen in design.json: ${to}`,
  screenNotShown: (/** @type {string} */ name) => `${name} is not shown with the current breakpoints.`,
  noFullscreen: 'Fullscreen is not available here.',
  // Screenshots
  shootCount: (/** @type {number} */ n) => `${n} frame${plural(n)} · shown breakpoints/states`,
  shootButton: (/** @type {number} */ n) => `Screenshot ${n}`,
  noScreenToShoot: 'No screen to screenshot.',
  shooting: (/** @type {number} */ n) => `Screenshotting ${n} screen${plural(n)}…`,
  shot: (/** @type {number} */ ok, /** @type {number} */ all, /** @type {string} */ dir, /** @type {string | null} */ failed) =>
    `${ok}/${all} PNG saved${dir ? ` in ${dir}` : ''}${failed ? ` · failed: ${failed}` : ''}`,
  shootNeedsNewServer: 'Screenshot needs a newer Studio server: run "dfw studio stop" then "dfw studio start", and reload.',
  shootFailed: (/** @type {string} */ why) => `Screenshot failed: ${why}`,
  // Edits
  saved: (/** @type {string} */ label, /** @type {string} */ file, /** @type {number} */ line) =>
    `${label ? `${label}. ` : ''}Saved to ${file}:${line}  (⌘Z to undo)`,
  noChange: 'No change.',
  undid: (/** @type {string} */ op, /** @type {string} */ file) => `Undid ${op} edit in ${file}.`,
  onlyTextOnly: 'Only text-only elements can be edited in place.',
  noText: 'This element has no text to edit.',
  needsIdToEdit: 'Give this element a data-dfw-id to edit it in place, or leave a comment.',
  // Layout edits (inside a frame)
  widthHandle:
    'Drag to resize the width (snaps to grid columns, size tokens or percentages). Alt+Shift+Left/Right. Double-click: back to auto',
  minHeightHandle: 'Drag to set a min-height (snaps to spacing/size tokens). Alt+Shift+Up/Down. Double-click: back to auto',
  noSizeTokens: 'No size tokens to snap to.',
  needsIdToMove: 'Select an element with a data-dfw-id to move or resize it.',
  onlyFlexGridChildren: 'Only children of a flex or grid container can be reordered.',
  alreadyFirst: 'Already the first item.',
  alreadyLast: 'Already the last item.',
  alreadyAt: (/** @type {string} */ label) => `Already ${label}.`,
  elementNotFound: (/** @type {string} */ what) => `Element ${what} not found in this frame.`,
  layoutConflict: (/** @type {string} */ id, /** @type {string} */ prop, /** @type {string | null} */ by) =>
    `Saved, but ${id} did not change: ${by ? `"${by}"` : 'another rule'} still sets ${prop}${by === 'style attribute' ? '' : ' with higher priority'}. Remove ${prop} there (or ask the AI), then resize again.`,
  // Live review (in the running app)
  liveToggleTitle: (/** @type {string} */ shortcut) => `Comment on an element${shortcut ? ` (${shortcut})` : ''}`,
  liveSent: (/** @type {string} */ n) => `Sent to AI · comment #${n}`,
  liveNotSent: (/** @type {string} */ why) => `Not sent: ${why}`,
});
