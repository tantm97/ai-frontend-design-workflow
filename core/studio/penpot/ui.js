// @ts-check
// The decisions of the dfw bridge panel (ui.html), kept apart from the page so node tests can run
// them (tests/unit/penpot-panel.test.mjs). A classic script: it defines one global, `dfwPanel`.
// Served by the Studio at /__dfw/penpot/ui.js, same origin as the panel.
// eslint-disable-next-line no-unused-vars -- the global the panel page reads
const dfwPanel = (() => {
  /** The design must be quiet this long before the boards are redrawn: a burst of writes gives one update. */
  const QUIET_MS = 10000;
  /** After a failed update, wait this long before trying again. */
  const RETRY_MS = 60000;
  /** An update that never finished is given up after this long. */
  const BUSY_MS = 300000;
  /** The canvas whose panels keep boards up to date by default. */
  const PENPOT = 'penpot';
  /** The status of a write the Studio refused for its token (a restarted Studio has a new one). */
  const FORBIDDEN = 403;

  /**
   * Whether the panel updates the boards now. `auto`: the box is checked; `built`: the file holds
   * dfw boards (never draw into a file the user did not build them in); `stale`: a design file
   * changed after the latest export; `boardsExportId` / `latestExportId`: the export the boards
   * were built from and the latest one; `quietMs`: time since the design last changed (null: no
   * design files); `busyMs`: how long a running update has run (0: none); `sinceFailMs`: time since
   * the last failed update (null: none).
   * @param {{ auto: boolean, built: boolean, stale: boolean, boardsExportId: string | null, latestExportId: string | null, quietMs: number | null, busyMs?: number, sinceFailMs?: number | null }} s
   * @returns {boolean}
   */
  function shouldAutoUpdate(s) {
    if (!s.auto || !s.built) return false;
    if (s.busyMs && s.busyMs < BUSY_MS) return false;
    if (s.sinceFailMs != null && s.sinceFailMs < RETRY_MS) return false;
    const behind = s.stale || (Boolean(s.latestExportId) && s.latestExportId !== s.boardsExportId);
    return behind && typeof s.quietMs === 'number' && s.quietMs >= QUIET_MS;
  }

  /**
   * Whether the panel must read session.json again before its next write: the Studio refused the
   * token (403), or could not be reached (`status` 0 or missing).
   * @param {number | null | undefined} status
   * @returns {boolean}
   */
  function needsSession(status) {
    return !status || status === FORBIDDEN;
  }

  /**
   * Whether "Keep boards up to date automatically" starts checked: what the user chose last in this
   * browser ("1" or "0"), else on when the project reviews in Penpot.
   * @param {string | null | undefined} stored
   * @param {string | null | undefined} canvas
   * @returns {boolean}
   */
  function autoDefault(stored, canvas) {
    if (stored === '1') return true;
    if (stored === '0') return false;
    return canvas === PENPOT;
  }

  return { shouldAutoUpdate, needsSession, autoDefault, QUIET_MS, RETRY_MS, BUSY_MS };
})();
