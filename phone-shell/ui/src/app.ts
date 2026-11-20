import {
  onConnection,
  openAddress,
  openSubscription,
  phone,
  scanTicket,
} from './bridge';
import {
  appendOutput,
  mergeSession,
  outputFromEvent,
  sessionFromEvent,
  type OutputState,
} from './live';
import {
  demoServerRows,
  demoStateClass,
  forwardPhaseFor,
  sessionDemoServers,
  type ForwardView,
} from './demo';
import {
  createTranslator,
  htmlLanguage,
  resolveLanguage,
  type MessageKey,
  type MessageValues,
  type Translate,
} from './i18n';
import type {
  BootstrapResponse,
  CommandError,
  ComputerRecord,
  ComputerView,
  ConnectionState,
  DemoServer,
  ForwardAddress,
  PairConfirm,
  PhoneInfo,
  Session,
  WorkState,
} from './types';

type Filter = 'all' | 'running' | 'waiting';

interface Notice {
  key: MessageKey;
  values?: MessageValues;
  tone: 'ok' | 'error' | 'neutral';
  detail?: string;
}

type Screen =
  | { name: 'loading' }
  | { name: 'failed'; error: CommandError }
  | { name: 'computers'; notice?: Notice }
  | { name: 'pair'; error?: Notice; ticket?: string }
  | { name: 'code'; confirm: PairConfirm }
  | {
      name: 'sessions';
      computer: ComputerRecord;
      sessions: Session[];
      filter: Filter;
      loading: boolean;
      error?: Notice;
      connection?: ConnectionState;
    }
  | {
      name: 'session';
      computer: ComputerRecord;
      sessions: Session[];
      session: Session;
      output: OutputState;
      demoServers: DemoServer[];
      loading: boolean;
      ended: boolean;
      error?: Notice;
      connection?: ConnectionState;
    };

const PAIR_STATUS_KEYS: Record<string, MessageKey> = {
  rejected: 'error.rejected',
  denied: 'error.denied',
  used: 'error.used',
  cancelled: 'error.cancelled',
  expired: 'error.expired',
  too_many_attempts: 'error.tooMany',
  protocol: 'error.protocol',
};

const ERROR_KEYS: Record<string, MessageKey> = {
  invalid_ticket: 'error.invalidTicket',
  expired: 'error.expired',
  cancelled: 'error.cancelledByUser',
  revoked: 'error.revoked',
  not_paired: 'error.notPaired',
  forbidden: 'error.forbidden',
  timeout: 'error.timeout',
  connection_closed: 'error.connectionClosed',
  transport: 'error.unreachable',
  not_connected: 'error.notConnected',
  no_pairing: 'error.noPairing',
};

const DETAIL_CODES = new Set([
  'keychain',
  'storage',
  'startup',
  'unknown',
  'internal',
  'store',
]);

function asError(value: unknown): CommandError {
  if (
    typeof value === 'object' &&
    value !== null &&
    'code' in value &&
    typeof (value as { code: unknown }).code === 'string'
  ) {
    return value as CommandError;
  }
  return {
    code: 'unknown',
    message: value instanceof Error ? value.message : String(value),
  };
}

function noticeFor(error: CommandError): Notice {
  const key =
    error.code === 'rejected' && error.status
      ? (PAIR_STATUS_KEYS[error.status] ?? 'error.rejected')
      : (ERROR_KEYS[error.code] ?? 'error.generic');
  return {
    key,
    tone: 'error',
    detail: DETAIL_CODES.has(error.code) ? error.message : undefined,
  };
}

function sessionStatusKey(session: Session): MessageKey {
  if (session.status === 'closed') {
    return 'status.closed';
  }
  if (session.waitingFor) {
    return 'status.waiting';
  }
  const labels: Record<WorkState, MessageKey> = {
    working: 'status.working',
    waiting: 'status.waiting',
    input: 'status.input',
    paused: 'status.paused',
    done: 'status.done',
  };
  return session.workState ? labels[session.workState] : 'status.active';
}

function matchesFilter(session: Session, filter: Filter): boolean {
  if (filter === 'running') {
    return session.status === 'active' && !session.waitingFor;
  }
  if (filter === 'waiting') {
    return session.waitingFor !== null;
  }
  return true;
}

type Child = Node | string | null | undefined | false;

interface ElementOptions {
  class?: string;
  text?: string;
  attrs?: Record<string, string>;
  on?: Record<string, EventListener>;
}

function el<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  options: ElementOptions = {},
  children: Child[] = [],
): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag);
  if (options.class) {
    node.className = options.class;
  }
  if (options.text !== undefined) {
    node.textContent = options.text;
  }
  for (const [name, value] of Object.entries(options.attrs ?? {})) {
    node.setAttribute(name, value);
  }
  for (const [name, handler] of Object.entries(options.on ?? {})) {
    node.addEventListener(name, handler);
  }
  for (const child of children) {
    if (child !== null && child !== undefined && child !== false) {
      node.append(child);
    }
  }
  return node;
}

function iconButton(
  label: string,
  svg: string,
  onClick: () => void,
): HTMLElement {
  const button = el('button', {
    class: 'phone-icon',
    attrs: { type: 'button', 'aria-label': label },
    on: { click: onClick },
  });
  button.innerHTML = svg;
  return button;
}

const PLUS_ICON =
  '<svg viewBox="0 0 20 20" aria-hidden="true"><path d="M10 4v12M4 10h12" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round"/></svg>';
const REFRESH_ICON =
  '<svg viewBox="0 0 20 20" aria-hidden="true"><path d="M16 10a6 6 0 1 1-1.8-4.3M16 4v4h-4" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"/></svg>';

export class ShellApp {
  private readonly root: HTMLElement;
  private t: Translate;
  private info: PhoneInfo | null = null;
  private computers: ComputerView[] = [];
  private screen: Screen = { name: 'loading' };
  private pairToken = 0;
  private sessionsToken = 0;
  private stopEvents: (() => Promise<void>) | null = null;
  private stopOutput: (() => Promise<void>) | null = null;
  private readonly connectionStates = new Map<string, ConnectionState>();
  private readonly forwards = new Map<number, ForwardView>();
  private renderPending = false;
  private idCounter = 0;

  constructor(root: HTMLElement) {
    this.root = root;
    this.t = createTranslator('en');
  }

  async start(): Promise<void> {
    const tags =
      navigator.languages && navigator.languages.length > 0
        ? navigator.languages
        : [navigator.language || 'en'];
    const language = resolveLanguage(tags);
    this.t = createTranslator(language);
    document.documentElement.lang = htmlLanguage(language);
    document.title = this.t('app.title');
    await onConnection((state) => {
      void this.handleConnection(state);
    });
    await this.loadComputers();
  }

  private async loadComputers(notice?: Notice): Promise<void> {
    try {
      this.info = await phone.info();
      this.computers = await phone.computers();
      this.screen = { name: 'computers', notice };
    } catch (error) {
      this.screen = { name: 'failed', error: asError(error) };
    }
    this.render();
  }

  private render(): void {
    this.root.replaceChildren();
    const shell = el('div', { class: 'phone' });
    shell.append(this.renderHead());
    shell.append(this.renderBody());
    this.root.append(shell);
  }

  private renderHead(): HTMLElement {
    const screen = this.screen;
    const head = el('header', { class: 'phone-head' });

    if (screen.name === 'session') {
      head.append(this.backButton(() => this.goSessions(screen.computer)));
      head.append(
        el('div', { class: 'phone-head-text' }, [
          el('span', { class: 'phone-title', text: screen.session.name }),
          el('span', { class: 'phone-sub', text: screen.computer.name }),
        ]),
      );
      return head;
    }

    if (screen.name === 'sessions') {
      head.append(this.backButton(() => this.goComputers()));
      head.append(
        el('div', { class: 'phone-head-text' }, [
          el('span', { class: 'phone-title', text: this.t('sessions.title') }),
          el('span', { class: 'phone-sub', text: screen.computer.name }),
        ]),
      );
      head.append(
        iconButton(this.t('sessions.refresh'), REFRESH_ICON, () => {
          void this.openSessions(screen.computer);
        }),
      );
      return head;
    }

    if (screen.name === 'pair' || screen.name === 'code') {
      head.append(this.backButton(() => this.goComputers()));
      const text = [
        el('span', {
          class: 'phone-title',
          text: this.t(
            screen.name === 'code' ? 'pair.codeTitle' : 'pair.title',
          ),
        }),
      ];
      if (screen.name === 'code') {
        text.push(
          el('span', {
            class: 'phone-sub',
            text: screen.confirm.computerName,
          }),
        );
      }
      head.append(el('div', { class: 'phone-head-text' }, text));
      return head;
    }

    if (screen.name === 'loading' || screen.name === 'failed') {
      head.append(
        el('div', { class: 'phone-head-text' }, [
          el('span', { class: 'phone-title', text: this.t('app.title') }),
        ]),
      );
      return head;
    }

    head.append(
      el('div', { class: 'phone-head-text' }, [
        el('span', { class: 'phone-title', text: this.t('computers.title') }),
        el('span', {
          class: 'phone-sub',
          text: this.t('computers.subtitle', {
            count: this.computers.length,
          }),
        }),
      ]),
    );
    head.append(
      iconButton(this.t('computers.add'), PLUS_ICON, () => {
        this.screen = { name: 'pair' };
        this.render();
      }),
    );
    return head;
  }

  private backButton(onClick: () => void): HTMLElement {
    return el('button', {
      class: 'phone-back',
      text: '‹',
      attrs: { type: 'button', 'aria-label': this.t('common.back') },
      on: { click: onClick },
    });
  }

  private renderBody(): HTMLElement {
    const screen = this.screen;
    switch (screen.name) {
      case 'loading':
        return el('main', { class: 'phone-list' }, [
          el('p', {
            class: 'phone-empty',
            text: this.t('app.loading'),
            attrs: { 'aria-busy': 'true' },
          }),
        ]);
      case 'failed':
        return this.renderFailed(screen.error);
      case 'computers':
        return this.renderComputers(screen.notice);
      case 'pair':
        return this.renderPair(screen);
      case 'code':
        return this.renderCode(screen.confirm);
      case 'sessions':
        return this.renderSessions(screen);
      case 'session':
        return this.renderSession(screen);
    }
  }

  private renderFailed(error: CommandError): HTMLElement {
    const notice = noticeFor(error);
    const main = el('main', { class: 'phone-list' }, [
      this.noticeEl(notice),
      el('div', { class: 'screen-actions' }, [
        el('button', {
          class: 'btn btn-primary',
          text: this.t('common.retry'),
          attrs: { type: 'button' },
          on: { click: () => void this.loadComputers() },
        }),
      ]),
    ]);
    return main;
  }

  private renderComputers(notice?: Notice): HTMLElement {
    const main = el('main', { class: 'phone-list' });
    if (notice) {
      main.append(this.noticeEl(notice));
    }
    if (this.computers.length === 0) {
      main.append(
        el('p', { class: 'phone-empty', text: this.t('computers.empty') }),
      );
      return main;
    }
    const list = el('ul', { class: 'rows' });
    for (const view of this.computers) {
      const state = view.connected
        ? this.t('computers.connected')
        : this.t('computers.offline');
      const mainButton = el('button', {
        class: 'row-main',
        attrs: { type: 'button' },
        on: {
          click: () => {
            void this.openSessions(view.computer);
          },
        },
      });
      mainButton.append(
        el('span', { class: 'row-name', text: view.computer.name }),
        el('span', { class: 'row-meta' }, [
          el('span', {
            class: view.connected ? 'row-state is-on' : 'row-state',
            text: state,
          }),
          el('span', {
            class: 'row-separator',
            text: '·',
            attrs: { 'aria-hidden': 'true' },
          }),
          el('span', {
            class: 'row-id',
            text: view.computer.id.slice(0, 8),
          }),
        ]),
      );
      const forget = el('button', {
        class: 'row-action',
        text: this.t('computers.forget'),
        attrs: {
          type: 'button',
          'aria-label': `${this.t('computers.forget')} ${view.computer.name}`,
        },
        on: {
          click: () => {
            void this.forget(view.computer.id);
          },
        },
      });
      list.append(el('li', { class: 'row' }, [mainButton, forget]));
    }
    main.append(list);
    return main;
  }

  private renderPair(
    screen: Extract<Screen, { name: 'pair' }>,
  ): HTMLElement {
    const main = el('main', { class: 'phone-list' });
    main.append(el('p', { class: 'lead', text: this.t('pair.lead') }));

    const label = el('label', {
      class: 'field-label',
      text: this.t('pair.ticketLabel'),
      attrs: { for: 'pair-ticket' },
    });
    const input = el('textarea', {
      class: 'ticket-input',
      attrs: {
        id: 'pair-ticket',
        rows: '3',
        spellcheck: 'false',
        autocapitalize: 'off',
        autocomplete: 'off',
        placeholder: this.t('pair.ticketPlaceholder'),
      },
    });
    input.value = screen.ticket ?? '';

    const scan = el('button', {
      class: 'btn btn-secondary',
      text: this.t('pair.scan'),
      attrs: { type: 'button' },
      on: {
        click: () => {
          void this.scanInto(input);
        },
      },
    });
    const submit = el('button', {
      class: 'btn btn-primary',
      text: this.t('pair.submit'),
      attrs: { type: 'submit' },
    });
    const form = el(
      'form',
      {
        class: 'pair-form',
        on: {
          submit: (event) => {
            event.preventDefault();
            void this.startPair(input.value.trim());
          },
        },
      },
      [label, input, el('div', { class: 'screen-actions' }, [scan, submit])],
    );
    main.append(form);
    if (screen.error) {
      main.append(this.noticeEl(screen.error));
    }
    return main;
  }

  private renderCode(confirm: PairConfirm): HTMLElement {
    const main = el('main', { class: 'phone-list code-screen' });
    main.append(el('p', { class: 'lead', text: this.t('pair.codeLead') }));
    main.append(
      el('div', { class: 'code-card' }, [
        el('output', {
          class: 'code-value',
          text: confirm.code,
          attrs: { 'aria-label': confirm.code },
        }),
      ]),
    );
    main.append(
      el('p', { class: 'code-computer', text: confirm.computerName }),
    );
    main.append(
      el('p', {
        class: 'code-wait',
        text: this.t('pair.waiting'),
        attrs: { 'aria-live': 'polite' },
      }),
    );
    main.append(
      el('div', { class: 'screen-actions' }, [
        el('button', {
          class: 'btn btn-secondary',
          text: this.t('common.cancel'),
          attrs: { type: 'button' },
          on: { click: () => void this.cancelPair() },
        }),
      ]),
    );
    return main;
  }

  private renderSessions(
    screen: Extract<Screen, { name: 'sessions' }>,
  ): HTMLElement {
    const main = el('main', { class: 'phone-list' });

    if (screen.loading) {
      main.append(
        el('p', {
          class: 'phone-empty',
          text: this.t('sessions.loading'),
          attrs: { 'aria-busy': 'true' },
        }),
      );
      return main;
    }

    if (screen.error) {
      main.append(this.noticeEl(screen.error));
      const actions = el('div', { class: 'screen-actions' }, [
        el('button', {
          class: 'btn btn-primary',
          text: this.t('common.retry'),
          attrs: { type: 'button' },
          on: { click: () => void this.openSessions(screen.computer) },
        }),
      ]);
      if (
        screen.error.key === 'error.revoked' ||
        screen.error.key === 'error.notPaired'
      ) {
        actions.append(
          el('button', {
            class: 'btn btn-secondary',
            text: this.t('computers.forget'),
            attrs: { type: 'button' },
            on: { click: () => void this.forget(screen.computer.id) },
          }),
        );
      }
      main.append(actions);
      return main;
    }

    const visible = screen.sessions.filter((session) =>
      matchesFilter(session, screen.filter),
    );
    const connectionNotice = screen.connection
      ? this.connectionNotice(screen.connection)
      : null;
    if (connectionNotice) {
      main.append(this.noticeEl(connectionNotice));
    }
    const waiting = visible.filter((session) => session.waitingFor !== null);
    const rest = visible.filter((session) => session.waitingFor === null);

    main.append(
      el(
        'div',
        {
          class: 'phone-filters',
          attrs: { role: 'group', 'aria-label': this.t('sessions.filter') },
        },
        (['all', 'running', 'waiting'] as const).map((filter) =>
          el('button', {
            class: screen.filter === filter ? 'chip is-on' : 'chip',
            text: this.t(
              filter === 'all'
                ? 'sessions.all'
                : filter === 'running'
                  ? 'sessions.running'
                  : 'status.waiting',
            ),
            attrs: {
              type: 'button',
              'aria-pressed':
                screen.filter === filter ? 'true' : 'false',
            },
            on: {
              click: () => {
                const current = this.screen;
                if (current.name !== 'sessions') {
                  return;
                }
                this.screen = { ...current, filter };
                this.render();
              },
            },
          }),
        ),
      ),
    );

    if (waiting.length === 0 && rest.length === 0) {
      main.append(
        el('p', { class: 'phone-empty', text: this.t('sessions.empty') }),
      );
      return main;
    }

    if (waiting.length > 0) {
      main.append(
        el('h2', { class: 'phone-section-title', text: this.t('sessions.waiting') }),
      );
      main.append(
        this.sessionList(waiting, (session) => {
          void this.openSession(screen.computer, session, screen.sessions);
        }),
      );
    }

    if (rest.length > 0) {
      if (waiting.length > 0) {
        main.append(
          el('h2', { class: 'phone-section-title', text: this.t('sessions.other') }),
        );
      }
      main.append(
        this.sessionList(rest, (session) => {
          void this.openSession(screen.computer, session, screen.sessions);
        }),
      );
    }

    return main;
  }

  private sessionList(
    sessions: Session[],
    onOpen: (session: Session) => void,
  ): HTMLElement {
    const list = el('ul', { class: 'rows' });
    for (const session of sessions) {
      const row = el('li', { class: 'row' }, [
        el('button', {
          class: 'row-main',
          attrs: {
            type: 'button',
            'aria-label': `${this.t('session.open')} ${session.name}`,
          },
          on: { click: () => onOpen(session) },
        }, [
          el('span', { class: 'row-name', text: session.name }),
          el('span', { class: 'row-meta' }, [
            el('span', {
              class: session.waitingFor
                ? 'row-state is-waiting'
                : session.status === 'active'
                  ? 'row-state is-on'
                  : 'row-state',
              text: this.t(sessionStatusKey(session)),
            }),
            el('span', {
              class: 'row-separator',
              text: '·',
              attrs: { 'aria-hidden': 'true' },
            }),
            el('span', { class: 'row-id', text: session.projectId }),
          ]),
          session.preview
            ? el('span', { class: 'row-preview', text: session.preview })
            : null,
        ]),
      ]);
      list.append(row);
    }
    return list;
  }

  private renderSession(
    screen: Extract<Screen, { name: 'session' }>,
  ): HTMLElement {
    const main = el('main', { class: 'phone-list session-screen' });
    const connectionNotice = screen.connection
      ? this.connectionNotice(screen.connection)
      : null;
    if (connectionNotice) {
      main.append(this.noticeEl(connectionNotice));
    }
    if (screen.error) {
      main.append(this.noticeEl(screen.error));
    }
    main.append(
      el('div', { class: 'session-meta' }, [
        el('span', {
          class:
            screen.session.status === 'active'
              ? 'row-state is-on'
              : 'row-state',
          text: this.t(sessionStatusKey(screen.session)),
        }),
        el('span', {
          class: 'row-separator',
          text: '·',
          attrs: { 'aria-hidden': 'true' },
        }),
        el('span', { class: 'row-id', text: screen.session.projectId }),
        screen.session.status === 'active' && !screen.ended
          ? el('span', { class: 'live-badge', text: this.t('session.live') })
          : null,
      ]),
    );

    const panel = el('section', {
      class: 'session-output-panel',
      attrs: { 'aria-label': this.t('session.output') },
    });
    panel.append(
      el('h2', {
        class: 'phone-section-title',
        text: this.t('session.output'),
      }),
    );
    if (screen.loading && screen.output.text.length === 0) {
      panel.append(
        el('p', {
          class: 'phone-empty',
          text: this.t('session.loading'),
          attrs: { 'aria-busy': 'true' },
        }),
      );
    } else if (screen.output.text.length === 0) {
      panel.append(
        el('p', { class: 'phone-empty', text: this.t('session.emptyOutput') }),
      );
    } else {
      panel.append(
        el('pre', {
          class: 'session-output',
          text: screen.output.text,
          attrs: { 'aria-live': 'polite' },
        }),
      );
    }
    if (screen.output.dropped) {
      panel.append(
        el('p', { class: 'session-note', text: this.t('session.dropped') }),
      );
    }
    if (screen.ended) {
      panel.append(
        el('p', { class: 'session-note', text: this.t('session.ended') }),
      );
    }
    main.append(panel);
    const demoPanel = this.renderDemoServers(screen);
    if (demoPanel) {
      main.append(demoPanel);
    }
    return main;
  }

  private renderDemoServers(
    screen: Extract<Screen, { name: 'session' }>,
  ): HTMLElement | null {
    if (screen.demoServers.length === 0) {
      return null;
    }
    const panel = el('section', {
      class: 'session-demo-panel',
      attrs: { 'aria-label': this.t('demoServer.title') },
    });
    panel.append(
      el('h2', {
        class: 'phone-section-title',
        text: this.t('demoServer.title'),
      }),
    );
    const list = el('ul', { class: 'demo-list' });
    for (const row of demoServerRows(
      screen.demoServers,
      this.forwards,
      screen.session.id,
    )) {
      const open = el('button', {
        class: 'btn demo-open',
        text: this.t('demoServer.open'),
        attrs: {
          type: 'button',
          'aria-label': this.t('demoServer.openLabel', {
            name: row.server.name,
          }),
        },
        on: {
          click: () => {
            void this.openDemoServer(row.server);
          },
        },
      });
      if (row.state === 'demoServer.starting') {
        open.disabled = true;
      }
      list.append(
        el('li', { class: 'demo-row' }, [
          el('div', { class: 'demo-text' }, [
            el('span', { class: 'demo-name', text: row.server.name }),
            el('span', {
              class: 'demo-address',
              text: row.address ?? this.t('demoServer.pending'),
            }),
          ]),
          el('span', {
            class: demoStateClass(row.state),
            text: this.t(row.state),
          }),
          open,
        ]),
      );
    }
    panel.append(list);
    return panel;
  }

  private async openDemoServer(server: DemoServer): Promise<void> {
    const screen = this.screen;
    if (
      screen.name !== 'session' ||
      screen.session.id !== server.sessionId
    ) {
      return;
    }
    const view = this.forwards.get(server.port);
    if (view?.sessionId === screen.session.id) {
      if (view.phase === 'starting') {
        return;
      }
      if (view.phase === 'ready' && view.address) {
        await this.openAddress(view.address);
        return;
      }
    }
    const address = await this.startForward(
      screen.computer,
      screen.session.id,
      server.port,
    );
    if (address) {
      await this.openAddress(address.address);
    }
  }

  private async openAddress(address: string): Promise<void> {
    try {
      await openAddress(address);
    } catch (error) {
      this.showSessionError(error);
    }
  }

  private async startForward(
    computer: ComputerRecord,
    sessionId: string,
    port: number,
  ): Promise<ForwardAddress | null> {
    this.forwards.set(port, { phase: 'starting', sessionId });
    this.scheduleRender();
    try {
      const address = await phone.forwardStart(computer.id, port);
      if (this.forwards.get(port)?.phase !== 'starting') {
        void phone.forwardStop(port).catch(() => undefined);
        return null;
      }
      this.forwards.set(port, {
        phase: 'ready',
        address: address.address,
        sessionId,
      });
      this.scheduleRender();
      return address;
    } catch (error) {
      if (this.forwards.get(port)?.phase === 'starting') {
        this.forwards.set(port, {
          phase: forwardPhaseFor(asError(error).code),
          sessionId,
        });
        this.scheduleRender();
      }
      return null;
    }
  }

  private syncDemoServers(
    computer: ComputerRecord,
    sessionId: string,
    servers: DemoServer[],
    refresh = false,
  ): void {
    if (refresh) {
      // The shell returns a live tunnel's address. The entry is dropped first
      // so a tunnel that died since the last view is opened again.
      for (const [port, view] of [...this.forwards]) {
        if (view.sessionId === sessionId) {
          this.forwards.delete(port);
        }
      }
    }
    const ports = new Set(servers.map((server) => server.port));
    for (const [port, view] of [...this.forwards]) {
      if (view.sessionId === sessionId && !ports.has(port)) {
        this.forwards.delete(port);
        void phone.forwardStop(port).catch(() => undefined);
      }
    }
    for (const server of servers) {
      const view = this.forwards.get(server.port);
      if (!view || view.sessionId !== sessionId) {
        void this.startForward(computer, sessionId, server.port);
      }
    }
    this.scheduleRender();
  }

  private showSessionError(error: unknown): void {
    const screen = this.screen;
    if (screen.name !== 'session') {
      return;
    }
    this.screen = { ...screen, error: noticeFor(asError(error)) };
    this.scheduleRender();
  }

  private connectionNotice(state: ConnectionState): Notice | null {
    if (state.state === 'connected') {
      return null;
    }
    if (state.state === 'reconnecting') {
      return { key: 'connection.reconnecting', tone: 'neutral' };
    }
    if (state.state === 'stopped' && state.error?.code === 'revoked') {
      return { key: 'error.revoked', tone: 'error' };
    }
    if (state.state === 'stopped' && state.error?.code === 'not_paired') {
      return { key: 'error.notPaired', tone: 'error' };
    }
    if (state.state === 'stopped') {
      return { key: 'connection.stopped', tone: 'error' };
    }
    return { key: 'connection.disconnected', tone: 'error' };
  }

  private async handleConnection(state: ConnectionState): Promise<void> {
    const previous = this.connectionStates.get(state.computerId);
    this.connectionStates.set(state.computerId, state);
    const screen = this.screen;
    if (
      (screen.name === 'sessions' || screen.name === 'session') &&
      screen.computer.id === state.computerId
    ) {
      this.screen = {
        ...screen,
        connection: state,
        error: state.state === 'connected' ? undefined : screen.error,
      };
      if (state.state === 'connected' && previous?.state !== 'connected') {
        // A live tunnel keeps its address. A dead one restarts when the
        // session's servers are synced again.
        this.forwards.clear();
      }
      if (state.state === 'stopped') {
        for (const port of this.forwards.keys()) {
          this.forwards.set(port, { phase: 'failed' });
        }
      }
      if (state.state === 'connected' && previous?.state !== 'connected') {
        await this.refreshBootstrap(screen.computer);
      } else {
        this.render();
      }
      return;
    }
    if (screen.name === 'computers') {
      this.computers = this.computers.map((view) =>
        view.computer.id === state.computerId
          ? { ...view, connected: state.state === 'connected' }
          : view,
      );
      this.render();
    }
  }

  private async refreshBootstrap(computer: ComputerRecord): Promise<void> {
    const token = this.sessionsToken;
    try {
      const data = (await phone.request(
        computer.id,
        'bootstrap',
      )) as BootstrapResponse;
      if (token !== this.sessionsToken) {
        return;
      }
      const sessions = data.sessions ?? [];
      const screen = this.screen;
      if (screen.name === 'sessions' && screen.computer.id === computer.id) {
        this.screen = { ...screen, sessions };
      } else if (
        screen.name === 'session' &&
        screen.computer.id === computer.id
      ) {
        const session =
          sessions.find((current) => current.id === screen.session.id) ??
          screen.session;
        const demoServers = sessionDemoServers(
          session.demoServers ?? [],
          session.id,
        );
        this.screen = { ...screen, sessions, session, demoServers };
        this.syncDemoServers(computer, session.id, demoServers, true);
      } else {
        return;
      }
      this.scheduleRender();
    } catch {
      // The connection state notice already explains a failed refresh.
    }
  }

  private async startEvents(
    computer: ComputerRecord,
    token: number,
  ): Promise<void> {
    const id = this.nextId('events');
    try {
      const stop = await openSubscription(computer.id, id, 'events', {}, {
        onEvent: (value) => this.applySessionEvent(value),
        onError: (error) => this.applyLiveError(computer.id, error),
      });
      if (token !== this.sessionsToken) {
        await stop();
        return;
      }
      this.stopEvents = stop;
    } catch {
      // The connection state notice already explains the failure.
    }
  }

  private applySessionEvent(value: unknown): void {
    const session = sessionFromEvent(value);
    if (!session) {
      return;
    }
    const screen = this.screen;
    if (screen.name === 'sessions') {
      this.screen = {
        ...screen,
        sessions: mergeSession(screen.sessions, session),
      };
      this.scheduleRender();
      return;
    }
    if (screen.name === 'session') {
      const sessions = mergeSession(screen.sessions, session);
      if (screen.session.id === session.id) {
        const demoServers = sessionDemoServers(
          session.demoServers ?? [],
          session.id,
        );
        this.screen = { ...screen, sessions, session, demoServers };
        this.syncDemoServers(screen.computer, session.id, demoServers);
        this.scheduleRender();
      }
    }
  }

  private applyLiveError(computerId: string, error: CommandError): void {
    const notice = noticeFor(error);
    const screen = this.screen;
    if (screen.name === 'sessions' && screen.computer.id === computerId) {
      this.screen = { ...screen, error: notice };
      this.scheduleRender();
    } else if (
      screen.name === 'session' &&
      screen.computer.id === computerId
    ) {
      this.screen = { ...screen, error: notice };
      this.scheduleRender();
    }
  }

  private applyOutputEvent(value: unknown): void {
    const event = outputFromEvent(value);
    if (!event) {
      return;
    }
    const screen = this.screen;
    if (screen.name !== 'session') {
      return;
    }
    this.screen = {
      ...screen,
      output: appendOutput(screen.output, event),
      loading: false,
      ended: false,
      error: undefined,
    };
    this.scheduleRender();
  }

  private failOutput(error: CommandError): void {
    const screen = this.screen;
    if (screen.name !== 'session') {
      return;
    }
    this.screen = {
      ...screen,
      loading: false,
      ended: true,
      error: noticeFor(error),
    };
    this.scheduleRender();
  }

  private endOutput(): void {
    const screen = this.screen;
    if (screen.name !== 'session') {
      return;
    }
    this.screen = { ...screen, loading: false, ended: true };
    this.scheduleRender();
  }

  private closeEvents(): void {
    const stop = this.stopEvents;
    this.stopEvents = null;
    if (stop) {
      void stop();
    }
  }

  private closeOutput(): void {
    const stop = this.stopOutput;
    this.stopOutput = null;
    if (stop) {
      void stop();
    }
  }

  private scheduleRender(): void {
    if (this.renderPending) {
      return;
    }
    this.renderPending = true;
    const run = () => {
      this.renderPending = false;
      this.render();
    };
    if (typeof requestAnimationFrame === 'function') {
      requestAnimationFrame(run);
    } else {
      setTimeout(run, 16);
    }
  }

  private nextId(prefix: string): string {
    this.idCounter += 1;
    return `${prefix}:${this.idCounter}`;
  }

  private noticeEl(notice: Notice): HTMLElement {
    const node = el('div', {
      class: `notice notice-${notice.tone}`,
      attrs: {
        role: notice.tone === 'error' ? 'alert' : 'status',
      },
    });
    node.append(
      el('p', {
        class: 'notice-text',
        text: this.t(notice.key, notice.values),
      }),
    );
    if (notice.detail) {
      node.append(el('p', { class: 'notice-detail', text: notice.detail }));
    }
    return node;
  }

  private async startPair(ticket: string): Promise<void> {
    if (!ticket) {
      return;
    }
    const platform = this.info?.platform ?? 'unknown';
    try {
      const confirm = await phone.pairStart(
        ticket,
        this.phoneName(),
        platform,
      );
      this.screen = { name: 'code', confirm };
      this.render();
      this.pairToken += 1;
      void this.finishPair(this.pairToken);
    } catch (error) {
      this.screen = {
        name: 'pair',
        error: noticeFor(asError(error)),
        ticket,
      };
      this.render();
    }
  }

  private async finishPair(token: number): Promise<void> {
    try {
      const record = await phone.pairFinish();
      if (token !== this.pairToken) {
        return;
      }
      await this.loadComputers({
        key: 'pair.saved',
        values: { name: record.name },
        tone: 'ok',
      });
    } catch (error) {
      if (token !== this.pairToken) {
        return;
      }
      this.screen = { name: 'pair', error: noticeFor(asError(error)) };
      this.render();
    }
  }

  private async cancelPair(): Promise<void> {
    this.pairToken += 1;
    try {
      await phone.pairCancel();
    } catch {
      // The pairing may already have finished; the computer answer wins.
    }
    this.screen = {
      name: 'computers',
      notice: { key: 'error.cancelledByUser', tone: 'neutral' },
    };
    this.render();
  }

  private async scanInto(input: HTMLTextAreaElement): Promise<void> {
    try {
      const ticket = await scanTicket();
      if (ticket) {
        input.value = ticket;
        input.focus();
      }
    } catch {
      this.screen = {
        name: 'pair',
        error: { key: 'pair.scanFailed', tone: 'error' },
        ticket: input.value,
      };
      this.render();
    }
  }

  private phoneName(): string {
    const suffix = this.info?.phoneId.slice(-4).toUpperCase() ?? 'PHONE';
    return `Phone · ${suffix}`;
  }

  private goComputers(): void {
    this.sessionsToken += 1;
    this.closeOutput();
    this.closeEvents();
    this.screen = { name: 'computers' };
    this.render();
    void this.loadComputers();
  }

  private goSessions(computer: ComputerRecord): void {
    const sessions =
      this.screen.name === 'session' ? this.screen.sessions : [];
    this.closeOutput();
    this.screen = {
      name: 'sessions',
      computer,
      sessions,
      filter: 'all',
      loading: false,
      connection: this.connectionStates.get(computer.id),
    };
    this.render();
    void this.refreshBootstrap(computer);
  }

  private async openSessions(computer: ComputerRecord): Promise<void> {
    this.sessionsToken += 1;
    const token = this.sessionsToken;
    this.closeOutput();
    this.closeEvents();
    this.screen = {
      name: 'sessions',
      computer,
      sessions: [],
      filter: 'all',
      loading: true,
      connection: this.connectionStates.get(computer.id),
    };
    this.render();
    try {
      await phone.connect(computer.id);
      const data = (await phone.request(
        computer.id,
        'bootstrap',
      )) as BootstrapResponse;
      if (token !== this.sessionsToken) {
        return;
      }
      this.screen = {
        name: 'sessions',
        computer,
        sessions: data.sessions ?? [],
        filter: 'all',
        loading: false,
        connection: this.connectionStates.get(computer.id),
      };
      this.render();
      await this.startEvents(computer, token);
    } catch (error) {
      if (token !== this.sessionsToken) {
        return;
      }
      this.screen = {
        name: 'sessions',
        computer,
        sessions: [],
        filter: 'all',
        loading: false,
        error: noticeFor(asError(error)),
        connection: this.connectionStates.get(computer.id),
      };
      this.render();
    }
  }

  private async openSession(
    computer: ComputerRecord,
    session: Session,
    sessions: Session[],
  ): Promise<void> {
    this.closeOutput();
    const demoServers = sessionDemoServers(
      session.demoServers ?? [],
      session.id,
    );
    this.screen = {
      name: 'session',
      computer,
      sessions,
      session,
      output: { text: '', lastSeq: 0, dropped: false },
      demoServers,
      loading: true,
      ended: false,
      connection: this.connectionStates.get(computer.id),
    };
    this.render();
    this.syncDemoServers(computer, session.id, demoServers, true);
    const id = this.nextId('output');
    try {
      const stop = await openSubscription(
        computer.id,
        id,
        'output',
        { sessionId: session.id, after: 0 },
        {
          onEvent: (value) => this.applyOutputEvent(value),
          onError: (error) => this.failOutput(error),
          onEnd: () => this.endOutput(),
        },
      );
      if (
        this.screen.name !== 'session' ||
        this.screen.session.id !== session.id
      ) {
        await stop();
        return;
      }
      this.stopOutput = stop;
    } catch (error) {
      this.failOutput(asError(error));
    }
  }

  private async forget(computerId: string): Promise<void> {
    try {
      this.sessionsToken += 1;
      this.closeOutput();
      this.closeEvents();
      this.connectionStates.delete(computerId);
      this.forwards.clear();
      await phone.forget(computerId);
      await this.loadComputers();
    } catch (error) {
      this.screen = {
        name: 'computers',
        notice: noticeFor(asError(error)),
      };
      this.render();
    }
  }
}
