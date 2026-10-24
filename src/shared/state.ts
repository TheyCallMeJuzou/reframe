// Persisted application state (userData/state.json) and its schema.
import { z } from 'zod';
import { PERMISSION_MODES } from './launch';
import type { GroupNode, LayoutNode } from './layout';

export const STATE_VERSION = 1;

const side = z.enum(['left', 'right', 'top', 'bottom']);

export const groupNodeSchema: z.ZodType<GroupNode> = z.lazy(() =>
  z.object({
    kind: z.literal('group'),
    id: z.string(),
    name: z.string(),
    color: z.string(),
    child: layoutNodeSchema,
  }),
);

export const layoutNodeSchema: z.ZodType<LayoutNode> = z.lazy(() =>
  z.union([
    z.object({ kind: z.literal('tile'), sessionId: z.string() }),
    z.object({
      kind: z.literal('split'),
      id: z.string(),
      dir: z.enum(['row', 'col']),
      children: z.array(layoutNodeSchema).min(2),
      sizes: z.array(z.number()),
    }),
    groupNodeSchema,
  ]),
);

export const restoreHintSchema = z.object({
  anchor: z.string(),
  side,
  ratio: z.number(),
  share: z.number().optional(),
});

/** A collapsed tile, or a whole collapsed group with its layout. */
export const dockEntrySchema = z.union([
  z.object({ kind: z.literal('tile'), sessionId: z.string(), hint: restoreHintSchema.nullable() }),
  z.object({ kind: z.literal('group'), node: groupNodeSchema, hint: restoreHintSchema.nullable() }),
]);

export const boundsSchema = z.object({ x: z.number(), y: z.number(), width: z.number(), height: z.number() });

export const windowStateSchema = z.object({
  id: z.string(),
  bounds: boundsSchema.nullable(),
  maximized: z.boolean(),
  /** Full screen (F11); `bounds` stay the window's normal size meanwhile. */
  fullscreen: z.boolean().optional(),
  hidden: z.boolean(),
  layout: layoutNodeSchema.nullable(),
  dock: z.array(dockEntrySchema),
  /** Whether the session list is shown in this window. */
  sidebarOpen: z.boolean().optional(),
});

export const launchOptionsSchema = z.object({
  model: z.string().optional(),
  permissionMode: z.enum(PERMISSION_MODES).optional(),
  worktree: z.boolean().optional(),
  worktreeName: z.string().optional(),
});

export const sessionRecordSchema = z.object({
  id: z.string(),
  /** The Claude Code session id; changes on /clear, so it is updated while the session runs. */
  claudeSessionId: z.string(),
  cwd: z.string(),
  /** The name reframe shows; without one, Claude Code's name for the session. */
  name: z.string().optional(),
  /**
   * The name Claude Code gets (--name) on every start, so it keeps it when resumed: null for none. Absent until
   * the session is renamed in reframe, and until then the same as `name`; renaming changes only reframe's.
   */
  claudeName: z.string().nullable().optional(),
  launch: launchOptionsSchema,
  /** open: should run (resumed on start). ended: kept in the list but not running. */
  lifecycle: z.enum(['open', 'ended']),
  createdAt: z.number(),
  lastActiveAt: z.number(),
  /** Last known process id, used to clean up orphans after a crash. */
  pid: z.number().optional(),
  /**
   * Whether the current claudeSessionId has had a prompt. Claude Code writes a transcript only from the
   * first message on, so an unused session cannot be resumed and is started fresh under the same id.
   */
  used: z.boolean().optional(),
});

export const LANGUAGES = ['system', 'en', 'de'] as const;
export type Language = (typeof LANGUAGES)[number];

export const preferencesSchema = z.object({
  /** "New session" fills the name with the folder's name. */
  nameFromFolder: z.boolean(),
  /** The UI language: the system's, or one chosen in About. */
  language: z.enum(LANGUAGES).default('system'),
});

export const DEFAULT_PREFERENCES: z.infer<typeof preferencesSchema> = { nameFromFolder: true, language: 'system' };

export const appStateSchema = z.object({
  version: z.literal(STATE_VERSION),
  sessions: z.record(z.string(), sessionRecordSchema),
  windows: z.array(windowStateSchema),
  recentFolders: z.array(z.string()),
  // Added after the first release of the format; older files get the defaults.
  preferences: preferencesSchema.default(DEFAULT_PREFERENCES),
});

export type DockEntry = z.infer<typeof dockEntrySchema>;
export type Bounds = z.infer<typeof boundsSchema>;
export type WindowState = z.infer<typeof windowStateSchema>;
export type LaunchOptions = z.infer<typeof launchOptionsSchema>;
export type SessionRecord = z.infer<typeof sessionRecordSchema>;
export type Preferences = z.infer<typeof preferencesSchema>;
export type AppState = z.infer<typeof appStateSchema>;

export function emptyWindow(id: string): WindowState {
  return { id, bounds: null, maximized: false, hidden: false, layout: null, dock: [] };
}

export function defaultState(): AppState {
  return {
    version: STATE_VERSION,
    sessions: {},
    windows: [emptyWindow(globalThis.crypto.randomUUID())],
    recentFolders: [],
    preferences: { ...DEFAULT_PREFERENCES },
  };
}
