// MessagePorts cannot cross contextBridge, so forward them to the page with window.postMessage.
const { contextBridge, ipcRenderer } = require('electron');

ipcRenderer.on('terminal-port', (e) => window.postMessage('terminal-port', '*', e.ports));

const allowedSend = new Set(['drag-start', 'tile-dropped', 'drag-end']);
const allowedOn = new Set(['detach', 'owner']);

contextBridge.exposeInMainWorld('spike', {
  send: (channel, data) => allowedSend.has(channel) && ipcRenderer.send(channel, data),
  on: (channel, cb) => allowedOn.has(channel) && ipcRenderer.on(channel, (_e, data) => cb(data)),
});
