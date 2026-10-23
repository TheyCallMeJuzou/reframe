// Local HTTP endpoint for Claude Code's HTTP hooks: POST /hook/<reframe session id>/<event>.
import { EventEmitter } from 'node:events';
import http from 'node:http';
import crypto from 'node:crypto';
import { HOOK_EVENTS, HOOK_TOKEN_HEADER, type HookEvent } from '../claude/settings';

export interface HookPayload {
  session_id?: string;
  cwd?: string;
  hook_event_name?: string;
  notification_type?: string;
  message?: string;
}

export interface HookMessage {
  sessionId: string;
  event: HookEvent;
  payload: HookPayload;
}

const MAX_BODY = 1024 * 1024;
const ROUTE = /^\/hook\/([^/]+)\/([A-Za-z]+)$/;

export class HookServer extends EventEmitter<{ hook: [HookMessage] }> {
  readonly token = crypto.randomBytes(24).toString('hex');
  private server: http.Server | null = null;
  port = 0;

  async start(): Promise<number> {
    this.server = http.createServer((req, res) => this.handle(req, res));
    await new Promise<void>((resolve, reject) => {
      this.server!.once('error', reject);
      this.server!.listen(0, '127.0.0.1', () => resolve());
    });
    this.port = (this.server.address() as { port: number }).port;
    return this.port;
  }

  stop(): void {
    this.server?.close();
    this.server = null;
  }

  private handle(req: http.IncomingMessage, res: http.ServerResponse): void {
    const reply = (status: number) => {
      res.writeHead(status, { 'content-type': 'application/json' });
      res.end('{}');
    };
    const match = ROUTE.exec(req.url ?? '');
    const token = req.headers[HOOK_TOKEN_HEADER.toLowerCase()];
    if (req.method !== 'POST' || !match) return reply(404);
    if (typeof token !== 'string' || !safeEqual(token, this.token)) return reply(403);
    const event = match[2] as HookEvent;
    if (!HOOK_EVENTS.includes(event)) return reply(404);

    let size = 0;
    const chunks: Buffer[] = [];
    req.on('data', (chunk: Buffer) => {
      size += chunk.length;
      if (size > MAX_BODY) req.destroy();
      else chunks.push(chunk);
    });
    req.on('end', () => {
      reply(200);
      let payload: HookPayload = {};
      try {
        const parsed: unknown = JSON.parse(Buffer.concat(chunks).toString('utf8'));
        if (parsed && typeof parsed === 'object') payload = parsed as HookPayload;
      } catch {
        // an empty or broken body still tells us the event happened
      }
      this.emit('hook', { sessionId: decodeURIComponent(match[1]!), event, payload });
    });
  }
}

function safeEqual(a: string, b: string): boolean {
  const x = Buffer.from(a);
  const y = Buffer.from(b);
  return x.length === y.length && crypto.timingSafeEqual(x, y);
}
