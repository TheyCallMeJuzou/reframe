// Launch options shared by main and renderer (kept free of zod so the renderer bundle stays small).

/** Values accepted by `claude --permission-mode`. */
export const PERMISSION_MODES = ['acceptEdits', 'auto', 'bypassPermissions', 'manual', 'dontAsk', 'plan'] as const;

export type PermissionMode = (typeof PERMISSION_MODES)[number];

/** Models offered for `--model` (any other name Claude Code knows works too). */
export const MODEL_SUGGESTIONS = ['fable', 'opus', 'sonnet', 'haiku'];
