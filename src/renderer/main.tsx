import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import '@xterm/xterm/css/xterm.css';
import './styles.css';
import { App } from './App';
import { TerminalRegistry } from './terminals';

// One registry for the window's lifetime; it outlives React re-mounts.
const terminals = new TerminalRegistry();

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <App terminals={terminals} />
  </StrictMode>,
);
