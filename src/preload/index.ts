// Exposes the typed `window.reframe` API. MessagePorts cannot cross the context bridge, so terminal
// ports are forwarded to the page with window.postMessage.
import { contextBridge, ipcRenderer, webUtils, type IpcRendererEvent } from 'electron';
import {
  IPC,
  TERMINAL_PORT_MESSAGE,
  type ForeignDropRequest,
  type ReframeApi,
  type SessionRename,
  type WindowView,
} from '@shared/ipc';

ipcRenderer.on(IPC.terminalPort, (event, payload: { sessionId: string }) => {
  window.postMessage({ type: TERMINAL_PORT_MESSAGE, sessionId: payload.sessionId }, '*', event.ports);
});

function subscribe<T>(channel: string, listener: (value: T) => void): () => void {
  const handler = (_event: IpcRendererEvent, value: T) => listener(value);
  ipcRenderer.on(channel, handler);
  return () => ipcRenderer.removeListener(channel, handler);
}

const api: ReframeApi = {
  getView: () => ipcRenderer.invoke(IPC.getView),
  onView: (listener) => subscribe<WindowView>(IPC.view, listener),
  dispatch: (action) => ipcRenderer.send(IPC.dispatch, action),
  createSession: (request) => ipcRenderer.invoke(IPC.createSession, request),
  restartSession: (sessionId, fresh) => ipcRenderer.send(IPC.restartSession, sessionId, fresh === true),
  endSession: (sessionId) => ipcRenderer.send(IPC.endSession, sessionId),
  attachTerminal: (sessionId) => ipcRenderer.send(IPC.attachTerminal, sessionId),
  pickFolder: () => ipcRenderer.invoke(IPC.pickFolder),
  getDialogDefaults: () => ipcRenderer.invoke(IPC.dialogDefaults),
  setPreferences: (patch) => ipcRenderer.send(IPC.setPreferences, patch),
  folderExists: (path) => ipcRenderer.invoke(IPC.folderExists, path),
  onFocusTile: (listener) => subscribe<string>(IPC.focusTile, listener),
  onOpenNewSession: (listener) => subscribe<null>(IPC.openNewSession, () => listener()),
  onOpenAbout: (listener) => subscribe<null>(IPC.openAbout, () => listener()),
  showWorkspaceMenu: () => ipcRenderer.send(IPC.workspaceMenu),
  showWorkspaceItemMenu: (workspaceId) => ipcRenderer.send(IPC.workspaceItemMenu, workspaceId),
  saveWorkspace: (name) => ipcRenderer.send(IPC.saveWorkspace, name),
  openWorkspace: (workspaceId) => ipcRenderer.send(IPC.openWorkspace, workspaceId),
  renameWorkspace: (workspaceId, name) => ipcRenderer.send(IPC.renameWorkspace, workspaceId, name),
  onPromptWorkspaceName: (listener) => subscribe<null>(IPC.promptWorkspaceName, () => listener()),
  onRenameWorkspace: (listener) => subscribe<string>(IPC.startRenameWorkspace, listener),
  openExternal: (url) => ipcRenderer.send(IPC.openExternal, url),
  transferNode: (request) => ipcRenderer.send(IPC.transferNode, request),
  newWindow: () => ipcRenderer.send(IPC.newWindow),
  toggleFullscreen: () => ipcRenderer.send(IPC.toggleFullscreen),
  platform: process.platform,
  readClipboard: () => ipcRenderer.invoke(IPC.readClipboard),
  pathForFile: (file) => webUtils.getPathForFile(file),
  saveDroppedFile: (name, data) => ipcRenderer.invoke(IPC.saveDroppedFile, name, data),
  saveClipboardImage: () => ipcRenderer.invoke(IPC.saveClipboardImage),
  startWsl: (distro) => ipcRenderer.send(IPC.startWsl, distro),
  tearOut: (key, size) => ipcRenderer.send(IPC.tearOut, key, size),
  showTileMenu: (sessionId) => ipcRenderer.send(IPC.tileMenu, sessionId),
  showGroupMenu: (groupId) => ipcRenderer.send(IPC.groupMenu, groupId),
  showGroupColorMenu: (groupId) => ipcRenderer.send(IPC.groupColorMenu, groupId),
  onRenameGroup: (listener) => subscribe<string>(IPC.renameGroup, listener),
  renameSession: (sessionId, name) => ipcRenderer.send(IPC.renameSession, sessionId, name),
  onRenameSession: (listener) => subscribe<SessionRename>(IPC.startRenameSession, listener),
  getSessionList: (query, limit) => ipcRenderer.invoke(IPC.sessionList, query, limit),
  openSession: (request) => ipcRenderer.send(IPC.openSession, request),
  removeSession: (sessionId) => ipcRenderer.send(IPC.removeSession, sessionId),
  openSessions: (request) => ipcRenderer.send(IPC.openSessions, request),
  showListMenu: (items) => ipcRenderer.send(IPC.listMenu, items),
  onForeignDrag: (listener) => subscribe<{ x: number; y: number } | null>(IPC.foreignDrag, listener),
  onForeignDrop: (listener) => subscribe<ForeignDropRequest>(IPC.foreignDrop, listener),
  answerForeignDrop: (requestId, answer) => ipcRenderer.send(IPC.foreignDropAnswer, requestId, answer),
  reportForeignPointer: (report) => ipcRenderer.send(IPC.foreignPointer, report),
};

contextBridge.exposeInMainWorld('reframe', api);
