import type { ReframeApi } from '../shared/ipc';

declare global {
  interface Window {
    reframe: ReframeApi;
  }
}

export {};
