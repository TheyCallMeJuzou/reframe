import { afterEach, describe, expect, it } from 'vitest';
import { HookServer, type HookMessage } from './server';

let server: HookServer | null = null;
afterEach(() => server?.stop());

async function post(path: string, body: unknown, token?: string): Promise<number> {
  const res = await fetch(`http://127.0.0.1:${server!.port}${path}`, {
    method: 'POST',
    headers: token ? { 'X-Reframe-Token': token } : {},
    body: JSON.stringify(body),
  });
  return res.status;
}

describe('HookServer', () => {
  it('emits authenticated hook events with the session from the path', async () => {
    server = new HookServer();
    await server.start();
    const received = new Promise<HookMessage>((resolve) => server!.once('hook', resolve));
    expect(await post('/hook/app%201/Stop', { session_id: 'c-1', cwd: '/w' }, server.token)).toBe(200);
    expect(await received).toEqual({ sessionId: 'app 1', event: 'Stop', payload: { session_id: 'c-1', cwd: '/w' } });
  });

  it('rejects wrong tokens, unknown events and other routes', async () => {
    server = new HookServer();
    await server.start();
    let count = 0;
    server.on('hook', () => count++);
    expect(await post('/hook/a/Stop', {}, 'wrong')).toBe(403);
    expect(await post('/hook/a/Stop', {})).toBe(403);
    expect(await post('/hook/a/PreToolUse', {}, server.token)).toBe(404);
    expect(await post('/other', {}, server.token)).toBe(404);
    expect(count).toBe(0);
  });

  it('answers a broken escape in the path with 400 and keeps going', async () => {
    server = new HookServer();
    await server.start();
    let count = 0;
    server.on('hook', () => count++);
    expect(await post('/hook/%E0%A4%A/Stop', {}, server.token)).toBe(400);
    expect(await post('/hook/a/Stop', {}, server.token)).toBe(200);
    await expect.poll(() => count).toBe(1);
  });
});
