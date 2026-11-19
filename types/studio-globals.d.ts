// Globals the Studio browser code sets or reads (type check only, tsconfig.json).
interface Window {
  /** The Studio state (core/studio/app/state.js), for debugging and the e2e test. */
  __dfwStudio: unknown;
  /** Set by overlay.js so a second copy in the same page does nothing. */
  __dfwOverlay: boolean;
  /** Set by live.js so a second copy in the same page does nothing. */
  __dfwLive: boolean;
  /** Comment-target helper overlay.js shares with live.js in a running app. */
  __dfwTargetInfo: (el: Element) => Record<string, unknown>;
  /** e2e marker: set in a frame to tell a patched frame from a reloaded one. */
  __dfwKept: boolean;
}
