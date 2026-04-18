/**
 * Phase 3 coding hard-case acceptance scaffolding.
 *
 * These cases are intentionally marked fixme for now. They define the live
 * operator-facing proofs we want once the coding/debugging loop runtime grows
 * beyond workflow demos.
 *
 * Target cases:
 * - repo edit yields a bounded, reviewable diff
 * - failing test is repaired in the same session
 * - child-session fanout/join stays bounded for coding work
 * - long idle resume preserves the same coding turn
 * - concurrent coding sessions stay isolated under load
 *
 * Run listing only:
 *   OCTOS_TEST_URL=https://dspfac.crew.ominix.io \
 *   OCTOS_AUTH_TOKEN=octos-admin-2026 \
 *   OCTOS_PROFILE=dspfac \
 *   npx playwright test tests/coding-hardcases.spec.ts --list
 */
import { test } from '@playwright/test';

import {
  createNewSession,
  login,
  sendAndWait,
  countAssistantBubbles,
  countUserBubbles,
} from './live-browser-helpers';

test.setTimeout(600_000);

const BASE = process.env.OCTOS_TEST_URL || 'https://dspfac.crew.ominix.io';
const TOKEN = process.env.OCTOS_AUTH_TOKEN || 'octos-admin-2026';
const PROFILE = process.env.OCTOS_PROFILE || 'dspfac';

async function getTasks(sessionId: string): Promise<any[]> {
  const resp = await fetch(`${BASE}/api/sessions/${encodeURIComponent(sessionId)}/tasks`, {
    headers: { Authorization: `Bearer ${TOKEN}`, 'X-Profile-Id': PROFILE },
  });
  if (!resp.ok) {
    throw new Error(`failed to fetch tasks for ${sessionId}: ${resp.status}`);
  }
  return resp.json();
}

test.describe('Phase 3 coding hard cases', () => {
  test('repo edit task writes a bounded diff and exposes reviewable output', async ({
    page,
  }) => {
    await login(page);
    await createNewSession(page);

    const marker = `phase3-${Date.now()}`;
    const prompt = [
      'Use shell tool only.',
      `Inside the current workspace, create a temporary git repo named ${marker}.`,
      'Inside it, create notes.txt with exactly two lines: alpha and beta.',
      'Make exactly one edit: change beta to gamma.',
      'Then run git diff -- notes.txt.',
      'Return only the unified diff, nothing else.',
      'Do not start background work.',
    ].join(' ');

    const result = await sendAndWait(page, prompt, {
      maxWait: 180_000,
      label: 'bounded-diff',
    });

    const response = result.responseText;
    if (!response) {
      throw new Error('Expected a reviewable diff response, got empty assistant output');
    }

    const userBubbles = await countUserBubbles(page);
    const assistantBubbles = await countAssistantBubbles(page);

    test.expect(userBubbles).toBe(1);
    test.expect(assistantBubbles).toBe(1);
    test.expect(response).toContain('diff --git');
    test.expect(response).toContain('notes.txt');
    test.expect(response).toContain('-beta');
    test.expect(response).toContain('+gamma');
    test.expect(response.length).toBeLessThan(4_000);
  });

  test('shell repair stays in one turn and returns the repaired diff', async ({ page }) => {
    await login(page);
    await createNewSession(page);

    const marker = `phase3-repair-${Date.now()}`;
    const prompt = [
      'Use shell tool only.',
      `Inside the current workspace, create a temporary git repo named ${marker}.`,
      'Inside it, create notes.txt with exactly two lines: alpha and beta.',
      'Make exactly one edit: change beta to gamma.',
      'Intentionally run `git diff -- notes.txt` from the parent workspace once so it fails.',
      'Then recover by running the same diff from the repo root.',
      'Return only the final unified diff, nothing else.',
      'Do not start background work.',
    ].join(' ');

    const result = await sendAndWait(page, prompt, {
      maxWait: 180_000,
      label: 'shell-repair-diff',
    });

    const response = result.responseText;
    if (!response) {
      throw new Error('Expected repaired diff output, got empty assistant response');
    }

    const userBubbles = await countUserBubbles(page);
    const assistantBubbles = await countAssistantBubbles(page);

    test.expect(userBubbles).toBe(1);
    test.expect(assistantBubbles).toBe(1);
    test.expect(response).toContain('diff --git');
    test.expect(response).toContain('notes.txt');
    test.expect(response).toContain('-beta');
    test.expect(response).toContain('+gamma');
    test.expect(response).toContain('@@');
    test.expect(response.length).toBeLessThan(4_000);
  });

  test('coding fanout creates bounded child sessions and joins them cleanly', async ({
    page,
  }) => {
    await login(page);
    await createNewSession(page);

    const sessionId = await page.evaluate(() => localStorage.getItem('octos_current_session'));
    if (!sessionId) {
      throw new Error('missing current session id');
    }

    const marker = `phase3-fanout-${Date.now()}`;
    const prompt = [
      'Use the spawn tool in background mode for coding reconnaissance.',
      'Attempt exactly three coding child sessions.',
      'Each child must set allowed_tools to ["shell"] and no other tools.',
      `Use labels ${marker}-a, ${marker}-b, and ${marker}-c.`,
      'Each child should only run a tiny shell command that prints its label, then stop.',
      'The parent must not run shell directly.',
      'After dispatching what is allowed, briefly say delegation started and stop.',
    ].join(' ');

    await sendAndWait(page, prompt, {
      maxWait: 120_000,
      label: 'coding-fanout',
    });

    await test.expect
      .poll(
        async () => {
          const tasks = await getTasks(sessionId);
          return tasks.filter((task: any) => task.child_session_key).length;
        },
        {
          timeout: 90_000,
          message: 'expected bounded coding child sessions to be created',
        },
      )
      .toBe(2);

    await test.expect
      .poll(
        async () => {
          const tasks = await getTasks(sessionId);
          const codingTasks = tasks.filter((task: any) => task.child_session_key);
          return (
            codingTasks.length === 2 &&
            codingTasks.every(
              (task: any) =>
                ['completed', 'failed'].includes(task.status) &&
                ['completed', 'retryable_failure', 'terminal_failure'].includes(
                  task.child_terminal_state,
                ) &&
                ['joined', 'orphaned'].includes(task.child_join_state),
            )
          );
        },
        {
          timeout: 120_000,
          message: 'expected bounded coding child sessions to terminate with structured join state',
        },
      )
      .toBe(true);
  });

  test.fixme('long idle resume keeps the same coding turn after reconnect', async ({ page }) => {
    await login(page);
    await createNewSession(page);
    await sendAndWait(page, 'TODO: start long coding task, idle, reload, and verify turn merge');
  });

  test.fixme('concurrent coding sessions remain isolated under load', async ({ browser }) => {
    const pageA = await browser.newPage();
    const pageB = await browser.newPage();
    await login(pageA);
    await login(pageB);
    await createNewSession(pageA);
    await createNewSession(pageB);
    await sendAndWait(pageA, 'TODO: concurrent coding case A');
    await sendAndWait(pageB, 'TODO: concurrent coding case B');
  });
});
