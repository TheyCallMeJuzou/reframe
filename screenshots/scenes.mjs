// What each demo session shows, by session name: an invented conversation in invented projects (a web shop, its
// API gateway, docs site and mobile app). Drawn by demo-claude.mjs.
//
// status: the registry status (idle: done, busy: working, waiting: needs a permission).
// blocks: { user }, { say }, { tool, arg, result, diff, running }, { todos: [[state, text], ...] }
// spinner: shown while busy. permission: the question shown instead of the input box while waiting.
// draft: text already in the input box. reply: played after Enter (spinner first, then the blocks one by one).
// Text in `backticks` is shown as code.

export const scenes = {
  'checkout-fix': {
    status: 'idle',
    blocks: [
      { user: 'Some orders are off by a cent. Find out why and fix it.' },
      { say: "I'll start with how the total is computed." },
      { tool: 'Read', arg: 'src/checkout/total.ts', result: ['Read 64 lines'] },
      {
        say: 'Tax is rounded per item, so the rounding errors add up. I will apply it to the subtotal and round once.',
      },
      {
        tool: 'Update',
        arg: 'src/checkout/total.ts',
        result: ['Updated src/checkout/total.ts with 3 additions and 2 removals'],
        diff: [
          [31, ' ', 'const subtotal = sum(items.map(lineTotal));'],
          [32, '-', 'const tax = items.reduce((t, i) => t + round(i.price * rate), 0);'],
          [33, '-', 'return subtotal + tax;'],
          [32, '+', 'const tax = subtotal * rate;'],
          [33, '+', '// Round once, at the end: per-item rounding drifts.'],
          [34, '+', 'return roundCents(subtotal + tax);'],
        ],
      },
      {
        tool: 'Bash',
        arg: 'npm test -- checkout',
        result: ['PASS  src/checkout/total.test.ts (14 tests)', 'PASS  src/checkout/cart.test.ts (9 tests)'],
      },
      {
        say: 'Fixed: totals are rounded once, after tax. I added the cart from the bug report as a test (3 × €19.99 at 20 % VAT); all 23 checkout tests pass.',
      },
    ],
    draft: 'also check the refunds',
  },

  'rate-limiter': {
    status: 'busy',
    blocks: [
      { user: 'Add rate limiting per API key: 100 requests a minute, then 429 with Retry-After.' },
      {
        todos: [
          ['done', 'Sliding-window limiter in Redis'],
          ['done', 'Hook it into the auth middleware'],
          ['doing', 'Answer 429 with a Retry-After header'],
          ['todo', 'Load test with k6'],
        ],
      },
      {
        tool: 'Write',
        arg: 'src/middleware/rate-limit.ts',
        result: ['Wrote 58 lines to src/middleware/rate-limit.ts'],
      },
      { tool: 'Bash', arg: 'npm run test:integration -- rate-limit', running: true },
    ],
    spinner: { verb: 'Running integration tests…', seconds: 41, tokens: '3.2k' },
    hint: 'accept',
  },

  'db-migration': {
    status: 'waiting',
    blocks: [
      { user: 'Orders search is slow. Add an index and migrate staging.' },
      {
        tool: 'Write',
        arg: 'prisma/migrations/20261024_order_search/migration.sql',
        result: ['Wrote 4 lines to prisma/migrations/20261024_order_search/migration.sql'],
      },
      {
        say: 'The migration only adds an index (`CONCURRENTLY`, so the table stays writable). Applying it to staging now.',
      },
    ],
    permission: {
      title: 'Bash command',
      command: 'npx prisma migrate deploy',
      description: 'Apply pending migrations to the staging database',
      options: [
        'Yes',
        "Yes, and don't ask again for npx prisma commands in ~\\projects\\webshop",
        'No, and tell Claude what to do differently (esc)',
      ],
    },
  },

  'payment-webhooks': {
    status: 'idle',
    blocks: [
      { user: 'Stripe sometimes sends a webhook twice. Make the handler idempotent.' },
      { tool: 'Search', arg: 'pattern: "constructEvent", path: "src/webhooks"', result: ['Found 2 files'] },
      { tool: 'Read', arg: 'src/webhooks/stripe.ts', result: ['Read 112 lines'] },
      {
        tool: 'Update',
        arg: 'src/webhooks/stripe.ts',
        result: ['Updated src/webhooks/stripe.ts with 5 additions'],
        diff: [
          [41, '+', 'if (!(await events.claim(event.id))) {'],
          [42, '+', "  log.info('duplicate webhook', { id: event.id });"],
          [43, '+', '  return res.status(200).end();'],
          [44, '+', '}'],
        ],
      },
      {
        say: 'Done. Each event id is claimed once (a unique key in `webhook_events`); a repeat gets a 200 and is not processed again.',
      },
    ],
  },

  'docs-refresh': {
    status: 'busy',
    blocks: [
      { user: 'Document the new rate limits, with an example for handling 429.' },
      { tool: 'Read', arg: 'docs/api/authentication.md', result: ['Read 86 lines'] },
      {
        tool: 'Update',
        arg: 'docs/api/limits.md',
        result: ['Updated docs/api/limits.md with 24 additions and 3 removals'],
        diff: [
          [12, '-', 'Requests are not limited yet.'],
          [12, '+', 'Each API key can make 100 requests a minute.'],
          [13, '+', 'Over the limit, you get `429 Too Many Requests`'],
          [14, '+', 'and a `Retry-After` header with the seconds to wait.'],
        ],
      },
    ],
    spinner: { verb: 'Writing the retry example…', seconds: 18, tokens: '1.1k' },
  },

  'ios-crash': {
    status: 'idle',
    blocks: [
      { user: 'The app crashes on launch on iOS 17 since 2.4.0. Crash log: [Pasted text #1 +38 lines]' },
      {
        say: 'It crashes in `CartStore.restore()`: it force-unwraps the saved cart, which is nil after a fresh install.',
      },
      {
        tool: 'Update',
        arg: 'ios/Store/CartStore.swift',
        result: ['Updated ios/Store/CartStore.swift with 2 additions and 1 removal'],
        diff: [
          [27, '-', 'let cart = try! decoder.decode(Cart.self, from: data!)'],
          [27, '+', 'guard let data else { return .empty }'],
          [28, '+', 'let cart = try decoder.decode(Cart.self, from: data)'],
        ],
      },
      { say: 'Fixed, with a test that starts without a saved cart.' },
    ],
  },

  'android-push': {
    status: 'busy',
    blocks: [
      { user: 'Push notifications stopped arriving on Android 14. Why?' },
      { tool: 'Read', arg: 'android/app/src/main/AndroidManifest.xml', result: ['Read 74 lines'] },
      {
        say: 'Android 13 and later need the `POST_NOTIFICATIONS` permission at runtime, and the app never asks for it.',
      },
      {
        tool: 'Update',
        arg: 'android/app/src/main/java/shop/Notifications.kt',
        result: ['Updated Notifications.kt with 9 additions'],
      },
    ],
    spinner: { verb: 'Building the debug APK…', seconds: 27, tokens: '2.4k' },
  },

  'release-notes': {
    status: 'idle',
    blocks: [
      { user: 'Write the release notes for 2.4.1 from the merged pull requests.' },
      { tool: 'Bash', arg: 'gh pr list --state merged --base release/2.4', result: ['12 pull requests'] },
      {
        say: 'Drafted `RELEASE_NOTES.md`: the launch crash fix, push notifications on Android 14, and faster order search.',
      },
    ],
  },

  'landing-page': {
    status: 'idle',
    blocks: [
      { user: 'Make the checkout summary fit on small phones.' },
      { tool: 'Read', arg: 'src/components/CheckoutSummary.tsx', result: ['Read 96 lines'] },
      { say: 'The summary has a fixed height of 320px. Can you show me how it looks on the phone?' },
    ],
    reply: {
      spinner: { verb: 'Looking at the screenshot…', seconds: 2, tokens: '0.4k' },
      blocks: [
        {
          say: 'I see it: the total wraps under the Pay button below 380px width. The summary needs to grow instead.',
        },
        {
          tool: 'Update',
          arg: 'src/components/CheckoutSummary.module.css',
          result: ['Updated CheckoutSummary.module.css with 2 additions and 1 removal'],
          diff: [
            [18, '-', 'height: 320px;'],
            [18, '+', 'min-height: 320px;'],
            [19, '+', 'flex-wrap: wrap;'],
          ],
        },
        { say: 'The total now stays above the button at any width.' },
      ],
    },
  },

  'mobile-header': {
    status: 'idle',
    blocks: [
      { user: 'The header looks broken on the product page. Check it.' },
      { tool: 'Read', arg: 'src/components/Header.tsx', result: ['Read 58 lines'] },
      { say: 'The markup looks fine to me. What exactly looks broken? A screenshot would help.' },
    ],
    reply: {
      spinner: { verb: 'Looking at the screenshot…', seconds: 2, tokens: '0.3k' },
      blocks: [
        { say: 'The search field pushes the cart icon out of the header. It needs to shrink.' },
        {
          tool: 'Update',
          arg: 'src/components/Header.module.css',
          result: ['Updated Header.module.css with 1 addition'],
          diff: [[24, '+', 'min-width: 0;']],
        },
        { say: 'Fixed: the field shrinks, and the cart stays in view.' },
      ],
    },
  },
};
