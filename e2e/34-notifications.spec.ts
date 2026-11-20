import { expect, test, type APIRequestContext, type Page } from '@playwright/test';
import {
  CORE_URL,
  closeSession,
  createAgentSession,
  createGitRepo,
  createProject,
  createProjectFolder,
  createTerminalSession,
  openApp,
  uniqueName,
} from './helpers';

interface DesktopNotificationCall {
  sessionId: string;
  title: string;
  body: string;
  kind: 'waiting' | 'done' | 'message';
  sessionVisible: boolean;
}

/**
 * Stubs the Tauri bridge and records the desktop notifications it receives.
 * The real notifications come from the packaged desktop shell.
 */
async function stubNotificationBridge(page: Page): Promise<void> {
  await page.addInitScript(() => {
    const scope = window as unknown as {
      __notifyCalls?: DesktopNotificationCall[];
    } & Record<string, unknown>;
    scope.__notifyCalls = [];
    let nextId = 1;
    scope.isTauri = true;
    scope.__TAURI_EVENT_PLUGIN_INTERNALS__ = {
      unregisterListener: () => {},
    };
    scope.__TAURI_INTERNALS__ = {
      metadata: {
        currentWindow: { label: 'main' },
        currentWebview: { windowLabel: 'main', label: 'main' },
      },
      transformCallback: () => nextId++,
      invoke: async (command: string, args?: DesktopNotificationCall) => {
        switch (command) {
          case 'show_notification':
            scope.__notifyCalls?.push(args as DesktopNotificationCall);
            return true;
          case 'notification_permission_state':
            return 'granted';
          case 'plugin:event|listen':
            return 1;
          default:
            return null;
        }
      },
    };
  });
}

function mcpCall(
  request: APIRequestContext,
  sessionId: string,
  name: string,
  argumentsValue: Record<string, unknown>,
) {
  return request.post(`${CORE_URL}/internal/mcp/${sessionId}`, {
    data: {
      jsonrpc: '2.0',
      id: 1,
      method: 'tools/call',
      params: { name, arguments: argumentsValue },
    },
  });
}

async function recorded(
  page: Page,
  kind: DesktopNotificationCall['kind'],
  body?: string,
): Promise<DesktopNotificationCall | null> {
  return page.evaluate(
    ({ wanted, wantedBody }) =>
      (window as unknown as { __notifyCalls?: DesktopNotificationCall[] })
        .__notifyCalls?.find(
          (call) =>
            call.kind === wanted &&
            (wantedBody === undefined || call.body === wantedBody),
        ) ?? null,
    { wanted: kind, wantedBody: body },
  );
}

test('sends a desktop notification for a message, a done report and a question', async ({
  page,
  request,
}) => {
  const label = uniqueName('notify-project');
  const repo = createGitRepo(label);
  const project = await createProject(request, label, repo);
  const session = await createAgentSession(request, {
    projectId: project.id,
    name: uniqueName('Notify session'),
    folder: repo,
  });
  const otherName = uniqueName('Other session');
  const other = await createTerminalSession(request, {
    projectId: project.id,
    name: otherName,
    folder: createProjectFolder(`${label}-other`),
  });

  try {
    await stubNotificationBridge(page);
    await openApp(page);
    await expect(
      page.getByRole('button', { name: new RegExp(session.name) }),
    ).toBeVisible();
    await page.getByRole('button', { name: new RegExp(session.name) }).click();
    await expect(
      page.getByRole('heading', { name: session.name }),
    ).toBeVisible();

    const message = await mcpCall(request, session.id, 'notify_me', {
      message: 'The build finished.',
    });
    expect(message.ok()).toBeTruthy();
    await expect(page.getByText(/The build finished\./)).toBeVisible();
    await expect
      .poll(() => recorded(page, 'message', 'The build finished.'))
      .not.toBeNull();
    const messageCall = await recorded(
      page,
      'message',
      'The build finished.',
    );
    expect(messageCall).toMatchObject({
      sessionId: session.id,
      kind: 'message',
      body: 'The build finished.',
      sessionVisible: true,
    });
    expect(messageCall?.title).toContain(session.name);

    await page.getByRole('button', { name: new RegExp(otherName) }).click();
    await expect(page.getByRole('heading', { name: otherName })).toBeVisible();
    const hidden = await mcpCall(request, session.id, 'notify_me', {
      message: 'While you were away.',
    });
    expect(hidden.ok()).toBeTruthy();
    await expect
      .poll(() => recorded(page, 'message', 'While you were away.'))
      .not.toBeNull();
    const hiddenCall = await recorded(
      page,
      'message',
      'While you were away.',
    );
    expect(hiddenCall).toMatchObject({
      sessionId: session.id,
      kind: 'message',
      body: 'While you were away.',
      sessionVisible: false,
    });

    await page.getByRole('button', { name: new RegExp(session.name) }).click();
    await expect(
      page.getByRole('heading', { name: session.name }),
    ).toBeVisible();

    const done = await mcpCall(request, session.id, 'report_done', {});
    expect(done.ok()).toBeTruthy();
    await expect(page.locator('.status-badge')).toHaveAttribute(
      'data-status',
      'done',
    );
    await expect.poll(() => recorded(page, 'done')).not.toBeNull();
    const doneCall = await recorded(page, 'done');
    expect(doneCall).toMatchObject({
      sessionId: session.id,
      kind: 'done',
      sessionVisible: true,
    });
    expect(doneCall?.title).toContain(session.name);
    expect(doneCall?.body.length).toBeGreaterThan(0);

    const asked = await mcpCall(request, session.id, 'ask_me', {
      text: 'Deploy now?',
      options: ['Yes', 'No'],
    });
    expect(asked.ok()).toBeTruthy();
    await expect(page.locator('.session-question')).toContainText(
      'Deploy now?',
    );
    await expect.poll(() => recorded(page, 'waiting')).not.toBeNull();
    const waitingCall = await recorded(page, 'waiting');
    expect(waitingCall).toMatchObject({
      sessionId: session.id,
      kind: 'waiting',
      sessionVisible: true,
    });
    expect(waitingCall?.title).toContain(session.name);
    expect(waitingCall?.body.length).toBeGreaterThan(0);
  } finally {
    await closeSession(request, session.id);
    await closeSession(request, other.id);
  }
});
