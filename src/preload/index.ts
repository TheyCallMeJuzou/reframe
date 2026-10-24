// Exposes the typed `window.reframe` API. MessagePorts cannot cross the context bridge, so terminal
// ports are forwarded to the page with window.postMessage.
import { contextBridge, ipcRenderer, type IpcRendererEvent } from 'electron';
import { IPC, TERMINAL_PORT_MESSAGE, type ReframeApi, type WindowView } from '@shared/ipc';

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
  getRecentFolders: () => ipcRenderer.invoke(IPC.recentFolders),
  folderExists: (path) => ipcRenderer.invoke(IPC.folderExists, path),
  onFocusTile: (listener) => subscribe<string>(IPC.focusTile, listener),
  onOpenNewSession: (listener) => subscribe<null>(IPC.openNewSession, () => listener()),
  openExternal: (url) => ipcRenderer.send(IPC.openExternal, url),
  transferTile: (request) => ipcRenderer.send(IPC.transferTile, request),
  tearOut: (sessionId, size) => ipcRenderer.send(IPC.tearOut, sessionId, size),
};

contextBridge.exposeInMainWorld('reframe', api);
