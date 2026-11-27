// @ts-check
// The fixed sets of values of the Studio, defined once (docs/conventions/javascript.md J14).
// Compare and assign with these names, never with the string: `S.mode === MODE.COMMENT`. Values
// of the platform itself (CSS values, tag names, `aria-*` strings) are not listed here.

/** The tool of the canvas and of the presenter. */
export const MODE = Object.freeze({
  SELECT: 'select',
  HAND: 'hand',
  COMMENT: 'comment',
  INTERACT: 'interact',
  /** Presenter only: click through the prototype. */
  PRESENT: 'present',
});

/** @typedef {typeof MODE[keyof typeof MODE]} Mode */

/** A review comment's status (the server's feedback contract). */
export const STATUS = Object.freeze({
  OPEN: 'open',
  ACKNOWLEDGED: 'acknowledged',
  REOPENED: 'reopened',
  FIXED: 'fixed',
  RESOLVED: 'resolved',
  DISMISSED: 'dismissed',
});

/** The filter tabs of the comment list. */
export const COMMENT_FILTER = Object.freeze({ ACTIVE: 'active', FIXED: 'fixed', CLOSED: 'closed', ALL: 'all' });

/** Which comment pins the canvas shows. */
export const PIN_FILTER = Object.freeze({ ACTIVE: 'active', ALL: 'all', OFF: 'off' });

/** The tabs of the right panel. */
export const TAB = Object.freeze({ INSPECT: 'inspect', COMMENTS: 'comments' });

/** The kind of a row on a canvas page (design.json `pages[].sections[].kind`). */
export const SECTION_KIND = Object.freeze({ BREAKPOINT: 'breakpoint', VARIANTS: 'variants', STATES: 'states', CUSTOM: 'custom' });

/** The buttons of the inspector (`data-act`). */
export const UI_ACTION = Object.freeze({
  MASTER: 'master',
  EDIT_TEXT: 'edit-text',
  LAYOUT_RESET: 'layout-reset',
  COMMENT: 'comment',
});

/** The fields of the inspector that edit an instance (`data-edit`). */
export const INSTANCE_FIELD = Object.freeze({ VARIANT: 'variant', SIZE: 'size', PROP: 'prop' });

/** The operations of a design edit (the server's `/__dfw/api/edit`). */
export const EDIT_OP = Object.freeze({
  TEXT: 'text',
  PROP: 'prop',
  VARIANT: 'variant',
  SIZE: 'size',
  SPACING: 'spacing',
  LAYOUT: 'layout',
  MOVE: 'move',
});

/** The properties a layout edit changes. */
export const LAYOUT_PROP = Object.freeze({ GRID_COLUMN: 'grid-column', WIDTH: 'width', MIN_HEIGHT: 'min-height' });

/** The non-arrow key of a layout edit (`MSG.LAYOUT_KEY`). */
export const LAYOUT_KEY = Object.freeze({ RESET: 'reset' });

/** Where a dragged element lands relative to another. */
export const DROP = Object.freeze({ BEFORE: 'before', AFTER: 'after', INSIDE: 'inside' });

/** The steps of a pan gesture (`MSG.PAN`) and of a forwarded key (`MSG.KEY`). */
export const PHASE = Object.freeze({ START: 'start', MOVE: 'move', END: 'end', DOWN: 'down', UP: 'up' });

/** What the agent is doing in a frame (the HUD). */
export const AGENT_PHASE = Object.freeze({ EDITING: 'editing', CHANGED: 'changed' });

/** The direction of a layer move (`MSG.NAV`). */
export const NAV_DIR = Object.freeze({ UP: 'up', DOWN: 'down', LEFT: 'left', RIGHT: 'right' });

/** The screen id of comments on the Design System page. */
export const SYSTEM_SCREEN = 'system';

/** The query parameter of a Studio address that opens a page (`?page=system`); the binary prints such links (design::pages::PAGE_QUERY). */
export const PAGE_QUERY = 'page';

/** What the send bar says about the agent, from the listening route: a wait listens, it works on comments it was given, or neither. */
export const LISTENER = Object.freeze({ LISTENING: 'listening', WORKING: 'working', OFF: 'off' });

/** `KeyboardEvent.key` values the Studio reacts to. */
export const KEY = Object.freeze({
  ESCAPE: 'Escape',
  ENTER: 'Enter',
  SPACE: ' ',
  UP: 'ArrowUp',
  DOWN: 'ArrowDown',
  LEFT: 'ArrowLeft',
  RIGHT: 'ArrowRight',
  HOME: 'Home',
  END: 'End',
  HELP: '?',
  TAB: 'Tab',
});
