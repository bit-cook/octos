import { expect, test, type Page } from '@playwright/test';

import {
  createNewSession,
  getAssistantMessageText,
  login,
  sendAndWait,
} from './live-browser-helpers';

const BASE = process.env.OCTOS_TEST_URL || 'https://dspfac.crew.ominix.io';
const TOKEN = process.env.OCTOS_AUTH_TOKEN || 'octos-admin-2026';
const PROFILE = process.env.OCTOS_PROFILE || 'dspfac';

test.setTimeout(600_000);

function parseSessionAddress(value: string | null): { sessionId: string; topic?: string } {
  const raw = value || '';
  const separator = raw.indexOf('#');
  if (separator === -1) return { sessionId: raw };
  const sessionId = raw.slice(0, separator);
  const topic = raw.slice(separator + 1).trim() || undefined;
  return { sessionId, topic };
}

async function currentSession(page: Page) {
  const stored = await page.evaluate(() => localStorage.getItem('octos_current_session'));
  return parseSessionAddress(stored);
}

async function fetchSessionStatus(sessionId: string, topic?: string) {
  const params = new URLSearchParams();
  if (topic?.trim()) params.set('topic', topic.trim());
  const suffix = params.toString() ? `?${params.toString()}` : '';
  const resp = await fetch(
    `${BASE}/api/sessions/${encodeURIComponent(sessionId)}/status${suffix}`,
    {
      headers: {
        Authorization: `Bearer ${TOKEN}`,
        'X-Profile-Id': PROFILE,
      },
    },
  );
  if (!resp.ok) {
    throw new Error(`status fetch failed: ${resp.status}`);
  }
  return resp.json() as Promise<{
    active: boolean;
    has_deferred_files: boolean;
    has_bg_tasks: boolean;
  }>;
}

async function waitForBackgroundWork(sessionId: string, topic?: string, timeoutMs = 120_000) {
  const deadline = Date.now() + timeoutMs;
  let last = { active: false, has_deferred_files: false, has_bg_tasks: false };
  while (Date.now() < deadline) {
    last = await fetchSessionStatus(sessionId, topic);
    if (last.has_bg_tasks) return last;
    await new Promise((resolve) => setTimeout(resolve, 3_000));
  }
  throw new Error(
    `background work never appeared for ${sessionId}${topic ? `#${topic}` : ''}: ${JSON.stringify(last)}`,
  );
}

test.describe('Background research while normal chatting', () => {
  test('normal chat stays usable while deep research runs in background', async ({ page }) => {
    await login(page);
    await createNewSession(page);

    const researchPrompt =
      "Do a deep research on the latest Rust programming language developments in 2026. Run the pipeline directly, don't ask me to choose.";

    const researchStart = await sendAndWait(page, researchPrompt, {
      label: 'bg-research-start',
      maxWait: 120_000,
      throwOnTimeout: false,
    });

    expect(researchStart.responseLen).toBeGreaterThan(0);

    const { sessionId, topic } = await currentSession(page);
    expect(sessionId).toMatch(/^web-/);

    const beforeChatStatus = await waitForBackgroundWork(sessionId, topic, 120_000);
    expect(beforeChatStatus.has_bg_tasks).toBe(true);

    const marker = `BG_CHAT_OK_${Date.now().toString(36)}`;
    const normalChat = await sendAndWait(
      page,
      `Reply with exactly ${marker}. Do not mention the research task.`,
      {
        label: 'bg-normal-chat',
        maxWait: 90_000,
      },
    );

    expect(normalChat.responseText).toContain(marker);

    const assistantText = await getAssistantMessageText(page);
    expect(assistantText).toContain(marker);
  });
});
