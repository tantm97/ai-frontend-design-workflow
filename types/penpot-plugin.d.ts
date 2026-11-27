// The part of the Penpot plugin API the dfw bridge (core/studio/penpot/plugin.js) uses, for the
// type check only (tsconfig.json, javascript.md J13). A member the plugin calls that is not
// declared here fails tsc: add it with the API list at the top of plugin.js. Shapes, pages and
// library items stay loosely typed: their shapes come from Penpot, not from dfw.

/** A Penpot object the plugin reads or changes (page, board, shape, component, token set). */
type PenpotObject = any;

interface PenpotUi {
  open(name: string, url: string, options?: { width?: number; height?: number }): void;
  onMessage(callback: (message: any) => void | Promise<void>): void;
  sendMessage(message: unknown): void;
}

interface PenpotFile {
  id: string;
  name: string;
  pages: PenpotObject[];
  setPluginData(key: string, value: string): void;
  getPluginData(key: string): string;
}

interface PenpotLocalLibrary {
  components: PenpotObject[];
  createComponent(shapes: PenpotObject[]): PenpotObject;
  tokens: { sets: PenpotObject[]; themes: PenpotObject[] };
}

interface Penpot {
  ui: PenpotUi;
  currentFile: PenpotFile | null;
  /** The page the user has open (read before an update, opened again after it). */
  currentPage: PenpotObject | null;
  createPage(): PenpotObject;
  openPage(page: PenpotObject): void;
  createBoard(): PenpotObject;
  createRectangle(): PenpotObject;
  createText(text: string): PenpotObject | null;
  createShapeFromSvg(svg: string): PenpotObject | null;
  uploadMediaData(name: string, data: Uint8Array, mimeType: string): Promise<PenpotObject>;
  fonts: { findByName(name: string): PenpotObject | null };
  library: { local: PenpotLocalLibrary };
}

declare const penpot: Penpot;
