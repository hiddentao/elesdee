import {
  onConnection,
  onNotificationClick,
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
import { historySupported, mergeHistory } from '../../../app/src/phone/history';
import {
  historyItemFromEvent,
  historyStatusFromEvent,
  toggleExpanded,
  toolStateKey,
} from './messages';
import type { HistoryItem } from '../../../app/src/api/types';
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
import {
  MAX_SAVED_KEYS,
  addSavedKey,
  encodeRailKey,
  isRepeatable,
  noModifiers,
  railKeys,
  readSavedKeys,
  removeSavedKey,
  savedKeyFrom,
  savedKeyLabel,
  toggleModifier,
  writeSavedKeys,
  type KeyModifiers,
  type Modifier,
  type RailKey,
  type SavedKey,
} from '../../../app/src/phone/keys';
import {
  buildStartInput,
  canSwitch,
  closeBodyKey,
  configChoiceLabel,
  filterChoices,
  pendingQuestion,
  pendingRequest,
  switchableConfigs,
  usageSummary,
} from '../../../app/src/phone/sessionActions';
import { combo } from './combo';
import {
  noticeForEvent,
  type NotificationRequest,
  type PhoneScreenState,
} from './notifications';
import {
  changeTotals,
  changesEmpty,
  diffLineKind,
  isImageName,
  parentFilePath,
} from '../../../app/src/phone/files';
import { diffRows, imageDataUrl, uploadNoticeFor } from './files';
import type {
  AiConfig,
  BootstrapResponse,
  CommandError,
  ComputerRecord,
  ComputerView,
  ConnectionState,
  DemoServer,
  FileEntry,
  FileList,
  ForwardAddress,
  Notification as SessionNotification,
  NotificationState,
  NotificationTarget,
  PairConfirm,
  PhoneInfo,
  Project,
  Provider,
  Question,
  Session,
  SessionChanges,
  SessionRequest,
  WorkState,
} from './types';

type Filter = 'all' | 'running' | 'waiting';
type OverlayKind = 'keys' | 'switch' | 'close' | 'usage';
type SessionView = 'messages' | 'terminal';

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
      history: HistoryItem[];
      historyStructured: boolean;
      historyLoading: boolean;
      view: SessionView;
      demoServers: DemoServer[];
      loading: boolean;
      ended: boolean;
      error?: Notice;
      connection?: ConnectionState;
    }
  | {
      name: 'newSession';
      computer: ComputerRecord;
      form: StartForm;
      loading: boolean;
      busy: boolean;
      error?: Notice;
      connection?: ConnectionState;
    };

interface StartForm {
  projectId: string | null;
  projectLabel: string;
  configId: string | null;
  configLabel: string;
  sourceBranch: string | null;
  sourceBranchLabel: string;
  name: string;
  firstPrompt: string;
}

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
const KEYBOARD_ICON =
  '<svg viewBox="0 0 24 24" aria-hidden="true"><rect x="3" y="6" width="18" height="12" rx="2" fill="none" stroke="currentColor" stroke-width="1.6"/><path d="M6.5 10h1M9.5 10h1M12.5 10h1M15.5 10h1M6.5 13h1M9.5 13h1M12.5 13h1M15.5 13h1M8 16h8" stroke="currentColor" stroke-width="1.4" stroke-linecap="round"/></svg>';

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
  private stopHistory: (() => Promise<void>) | null = null;
  private activeComputerId: string | null = null;
  private notificationPermission: NotificationState = 'prompt';
  private readonly expandedTools = new Set<string>();
  private readonly connectionStates = new Map<string, ConnectionState>();
  private readonly forwards = new Map<number, ForwardView>();
  private renderPending = false;
  private idCounter = 0;
  private projects: Project[] = [];
  private providers: Provider[] = [];
  private configs: AiConfig[] = [];
  private questions: Question[] = [];
  private requests: SessionRequest[] = [];
  private savedKeys: SavedKey[] = [];
  private overlay: { kind: OverlayKind; sessionId?: string } | null = null;
  private usageLoading = false;
  private usageError: Notice | null = null;
  private switchLoading = false;
  private sessionCache: Session[] = [];
  private readonly railStates = new Map<
    string,
    { open: boolean; scroll: number }
  >();
  private readonly latches = new Map<string, KeyModifiers>();
  private readonly drafts = new Map<string, string>();
  private repeatTimer: ReturnType<typeof setInterval> | null = null;
  private repeatDelay: ReturnType<typeof setTimeout> | null = null;
  private suppressClick = false;
  private keyDraft: { name: string; modifiers: Modifier[] } = {
    name: '',
    modifiers: [],
  };
  private switchFilter = '';
  private filePanel: 'files' | 'changes' | null = null;
  private filesPath = '';
  private filesList: FileList | null = null;
  private filesLoading = false;
  private filesError: Notice | null = null;
  private filePreview:
    | { kind: 'image'; name: string; src: string }
    | { kind: 'text'; name: string; content: string }
    | null = null;
  private changes: SessionChanges | null = null;
  private changesLoading = false;
  private changesError: Notice | null = null;
  private openDiff: string | null = null;
  private uploadBusy = false;
  private uploadNotice: Notice | null = null;

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
    this.savedKeys = readSavedKeys(this.keyStorage());
    await onConnection((state) => {
      void this.handleConnection(state);
    });
    await onNotificationClick((target) => {
      void this.openNotificationTarget(target);
    });
    await this.loadComputers();
    void this.ensureNotifications();
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
    const focused = document.activeElement;
    const focusId =
      focused instanceof HTMLElement && focused.id ? focused.id : null;
    const selection =
      focused instanceof HTMLInputElement ||
      focused instanceof HTMLTextAreaElement
        ? [focused.selectionStart, focused.selectionEnd]
        : null;
    const output = this.root.querySelector<HTMLElement>('.session-output');
    const follow =
      output !== null &&
      output.scrollHeight - output.scrollTop - output.clientHeight < 24;
    const messages = this.root.querySelector<HTMLElement>('.session-messages');
    const followMessages =
      messages !== null &&
      messages.scrollHeight - messages.scrollTop - messages.clientHeight < 24;

    this.root.replaceChildren();
    const shell = el('div', { class: 'phone' });
    shell.append(this.renderHead());
    shell.append(this.renderBody());
    if (this.overlay) {
      shell.append(this.renderOverlay(this.overlay));
    }
    this.root.append(shell);

    if (focusId) {
      const next = document.getElementById(focusId);
      if (next) {
        next.focus();
        if (
          selection &&
          (next instanceof HTMLInputElement || next instanceof HTMLTextAreaElement)
        ) {
          next.setSelectionRange(selection[0], selection[1]);
        }
      }
    }
    const nextOutput = this.root.querySelector<HTMLElement>('.session-output');
    if (nextOutput && follow) {
      nextOutput.scrollTop = nextOutput.scrollHeight;
    }
    const nextMessages =
      this.root.querySelector<HTMLElement>('.session-messages');
    if (nextMessages && followMessages) {
      nextMessages.scrollTop = nextMessages.scrollHeight;
    }
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

    if (screen.name === 'newSession') {
      head.append(this.backButton(() => this.goSessions(screen.computer)));
      head.append(
        el('div', { class: 'phone-head-text' }, [
          el('span', { class: 'phone-title', text: this.t('session.new') }),
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
        iconButton(this.t('session.new'), PLUS_ICON, () => {
          void this.openNewSession(screen.computer);
        }),
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
      case 'newSession':
        return this.renderNewSession(screen);
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
    const notificationNotice = this.notificationNotice();
    if (notificationNotice) {
      main.append(this.noticeEl(notificationNotice));
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
    const notificationNotice = this.notificationNotice();
    if (notificationNotice) {
      main.append(this.noticeEl(notificationNotice));
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

    main.append(this.renderSessionActions(screen.session));
    const card = this.renderSessionCard(screen.session);
    if (card) {
      main.append(card);
    }

    main.append(this.renderSessionTools());
    main.append(this.renderContextStrip(screen));
    const filePanel = this.renderSessionFilePanel(screen);
    if (filePanel) {
      main.append(filePanel);
    }
    if (screen.view === 'messages' && screen.historyStructured) {
      main.append(this.renderMessages(screen));
    } else {
      if (
        !historySupported(
          screen.session.client ?? null,
          screen.session.kind,
        )
      ) {
        main.append(
          el('p', {
            class: 'session-note',
            text: this.t('session.terminalOnly'),
          }),
        );
      }
      main.append(this.renderOutputPanel(screen));
    }
    const demoPanel = this.renderDemoServers(screen);
    if (demoPanel) {
      main.append(demoPanel);
    }
    main.append(this.renderComposer(screen.session, screen.view));
    return main;
  }

  private renderContextStrip(
    screen: Extract<Screen, { name: 'session' }>,
  ): HTMLElement {
    const session = screen.session;
    const context =
      session.contextLeft === null || session.contextLeft === undefined
        ? this.t('session.contextUnknown')
        : this.t('session.contextLeft', {
            percent: Math.round(session.contextLeft),
          });
    const facts = el('div', { class: 'session-context-facts' }, [
      el('span', {
        class: 'session-context-state',
        text: this.t(sessionStatusKey(session)),
      }),
      el('span', { class: 'session-context-item', text: context }),
      session.branch
        ? el('span', {
            class: 'session-context-branch',
            text: session.branch,
            attrs: { title: session.branch },
          })
        : null,
    ]);
    const strip = el('div', { class: 'session-context' }, [facts]);
    if (screen.historyStructured) {
      strip.append(
        el('button', {
          class: 'btn btn-secondary session-view-toggle',
          text:
            screen.view === 'messages'
              ? this.t('session.viewTerminal')
              : this.t('session.viewMessages'),
          attrs: {
            type: 'button',
            'aria-label': this.t('session.viewLabel'),
          },
          on: {
            click: () => {
              if (this.screen.name !== 'session') {
                return;
              }
              this.screen = {
                ...this.screen,
                view:
                  this.screen.view === 'messages' ? 'terminal' : 'messages',
              };
              this.scheduleRender();
            },
          },
        }),
      );
    }
    return strip;
  }

  private renderMessages(
    screen: Extract<Screen, { name: 'session' }>,
  ): HTMLElement {
    const panel = el('section', {
      class: 'session-messages-panel',
      attrs: { 'aria-label': this.t('session.messages') },
    });
    const scroll = el('div', { class: 'session-messages' });
    if (screen.history.length === 0) {
      scroll.append(
        el('p', {
          class: 'phone-empty',
          text: screen.historyLoading
            ? this.t('session.messagesLoading')
            : this.t('session.messagesEmpty'),
          attrs: screen.historyLoading ? { 'aria-busy': 'true' } : {},
        }),
      );
    } else {
      const log = el('div', {
        class: 'session-message-list',
        attrs: {
          role: 'log',
          'aria-label': this.t('session.messages'),
        },
      });
      for (const item of screen.history) {
        log.append(this.renderHistoryItem(screen, item));
      }
      if (screen.session.workState === 'done') {
        log.append(
          el('p', {
            class: 'session-messages-done',
            text: this.t('session.finished'),
          }),
        );
      }
      scroll.append(log);
    }
    panel.append(
      el('h2', {
        class: 'phone-section-title',
        text: this.t('session.messages'),
      }),
      scroll,
    );
    if (screen.ended) {
      panel.append(
        el('p', { class: 'session-note', text: this.t('session.ended') }),
      );
    }
    return panel;
  }

  private renderHistoryItem(
    screen: Extract<Screen, { name: 'session' }>,
    item: HistoryItem,
  ): HTMLElement {
    if (item.kind === 'prompt') {
      return el('article', { class: 'session-message is-prompt' }, [
        el('span', {
          class: 'session-message-label',
          text: this.t('session.you'),
        }),
        el('p', { class: 'session-message-text', text: item.text }),
      ]);
    }
    if (item.kind === 'reply') {
      const text = el('p', { class: 'session-message-text', text: item.text });
      if (item.state === 'streaming') {
        text.append(
          el('span', {
            class: 'session-message-caret',
            text: '▍',
            attrs: { 'aria-hidden': 'true' },
          }),
        );
      }
      return el(
        'article',
        {
          class: 'session-message is-reply',
          attrs: { 'data-state': item.state },
        },
        [
          el('span', {
            class: 'session-message-label',
            text: screen.session.client ?? this.t('session.agent'),
          }),
          text,
        ],
      );
    }

    const open = this.expandedTools.has(item.id);
    const stateKey = toolStateKey(item.state);
    const row = el('button', {
      class: 'session-tool-row',
      attrs: {
        type: 'button',
        id: `session-tool-${item.id}`,
        'aria-expanded': open ? 'true' : 'false',
        'data-state': item.state,
      },
      on: {
        click: () => {
          const next = toggleExpanded(this.expandedTools, item.id);
          this.expandedTools.clear();
          for (const id of next) {
            this.expandedTools.add(id);
          }
          this.scheduleRender();
        },
      },
    });
    row.append(
      el('span', {
        class: 'session-tool-name',
        text: item.tool ?? this.t('session.tool'),
      }),
      el('span', { class: 'session-tool-summary', text: item.text }),
    );
    if (stateKey) {
      row.append(
        el('span', {
          class: 'session-tool-state',
          text: this.t(stateKey),
        }),
      );
    }
    row.append(
      el('span', {
        class: 'session-tool-marker',
        text: open ? '−' : '+',
        attrs: { 'aria-hidden': 'true' },
      }),
    );
    const tool = el('div', { class: 'session-tool' }, [row]);
    if (open) {
      tool.append(
        el('pre', { class: 'session-tool-detail', text: item.text }),
      );
    }
    return tool;
  }

  private renderOutputPanel(
    screen: Extract<Screen, { name: 'session' }>,
  ): HTMLElement {
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
    return panel;
  }

  private renderSessionActions(session: Session): HTMLElement {
    const actions = el('div', {
      class: 'session-actions',
      attrs: { role: 'group', 'aria-label': this.t('session.actions') },
    });
    if (canSwitch(session)) {
      actions.append(
        el('button', {
          class: 'btn btn-secondary',
          text: this.t('session.switch'),
          attrs: { type: 'button' },
          on: { click: () => void this.openSwitch(session.id) },
        }),
      );
    }
    actions.append(
      el('button', {
        class: 'btn btn-secondary',
        text: this.t('session.usage'),
        attrs: { type: 'button' },
        on: { click: () => void this.openUsage() },
      }),
    );
    if (session.status === 'active') {
      actions.append(
        el('button', {
          class: 'btn btn-secondary',
          text: this.t('session.close'),
          attrs: { type: 'button' },
          on: { click: () => this.openClose(session.id) },
        }),
      );
    }
    return actions;
  }

  private renderSessionTools(): HTMLElement {
    const tools = el('div', {
      class: 'session-tools',
      attrs: { role: 'group', 'aria-label': this.t('session.tools') },
    });
    const add = (panel: 'files' | 'changes', label: MessageKey): void => {
      const selected = this.filePanel === panel;
      tools.append(
        el('button', {
          class: selected ? 'btn btn-secondary is-on' : 'btn btn-secondary',
          text: this.t(label),
          attrs: {
            type: 'button',
            'aria-pressed': selected ? 'true' : 'false',
          },
          on: {
            click: () => {
              this.filePanel = selected ? null : panel;
              this.scheduleRender();
              const screen = this.screen;
              if (screen.name !== 'session') {
                return;
              }
              if (
                this.filePanel === 'files' &&
                this.filesList === null &&
                !this.filesLoading
              ) {
                void this.loadFiles(screen.computer, screen.session.id, '');
              }
              if (
                this.filePanel === 'changes' &&
                this.changes === null &&
                !this.changesLoading
              ) {
                void this.loadChanges(screen.computer, screen.session.id);
              }
            },
          },
        }),
      );
    };
    add('files', 'session.files');
    add('changes', 'session.changes');
    return tools;
  }

  private renderSessionFilePanel(
    screen: Extract<Screen, { name: 'session' }>,
  ): HTMLElement | null {
    if (this.filePanel === 'files') {
      return this.renderFilesPanel(screen);
    }
    if (this.filePanel === 'changes') {
      return this.renderChangesPanel(screen);
    }
    return null;
  }

  private renderFilesPanel(
    screen: Extract<Screen, { name: 'session' }>,
  ): HTMLElement {
    const panel = el('section', {
      class: 'session-files',
      attrs: { 'aria-label': this.t('session.files') },
    });
    const head = el('div', { class: 'panel-head' });
    head.append(
      el('h2', { class: 'phone-section-title', text: this.t('files.title') }),
    );
    const upload = el('button', {
      class: 'btn btn-secondary',
      text: this.uploadBusy
        ? this.t('files.uploading')
        : this.t('files.upload'),
      attrs: { type: 'button' },
      on: { click: () => void this.pickUpload(screen) },
    });
    if (this.uploadBusy || screen.session.status === 'closed') {
      upload.disabled = true;
    }
    head.append(upload);
    panel.append(head);
    if (this.uploadNotice) {
      panel.append(this.noticeEl(this.uploadNotice));
    }

    const pathRow = el('div', { class: 'file-path' });
    pathRow.append(
      el('span', { text: this.filesPath ? `/${this.filesPath}` : '/' }),
    );
    const actions = el('div', { class: 'file-actions' });
    if (this.filesPath) {
      actions.append(
        el('button', {
          class: 'btn btn-ghost',
          text: this.t('files.folderUp'),
          attrs: { type: 'button' },
          on: {
            click: () =>
              void this.loadFiles(
                screen.computer,
                screen.session.id,
                parentFilePath(this.filesPath),
              ),
          },
        }),
      );
    }
    actions.append(
      el('button', {
        class: 'btn btn-ghost',
        text: this.t('files.refresh'),
        attrs: { type: 'button' },
        on: {
          click: () =>
            void this.loadFiles(
              screen.computer,
              screen.session.id,
              this.filesPath,
            ),
        },
      }),
    );
    pathRow.append(actions);
    panel.append(pathRow);

    if (this.filesLoading) {
      panel.append(
        el('p', {
          class: 'session-note',
          text: this.t('files.loading'),
          attrs: { role: 'status', 'aria-busy': 'true' },
        }),
      );
    } else if (this.filesError) {
      panel.append(this.noticeEl(this.filesError));
    } else if (!this.filesList || this.filesList.entries.length === 0) {
      panel.append(
        el('p', { class: 'session-note', text: this.t('files.empty') }),
      );
    } else {
      const list = el('ul', { class: 'file-list' });
      for (const entry of this.filesList.entries) {
        const row = el('button', {
          class: 'file-row',
          attrs: { type: 'button' },
          on: { click: () => void this.openFile(screen, entry) },
        });
        row.append(el('span', { class: 'file-name', text: entry.name }));
        if (entry.directory) {
          row.append(
            el('span', { class: 'file-kind', text: this.t('files.folder') }),
          );
        } else if (isImageName(entry.name)) {
          row.append(
            el('span', { class: 'file-kind', text: this.t('files.image') }),
          );
        }
        list.append(el('li', {}, [row]));
      }
      panel.append(list);
    }
    if (this.filesList?.truncated) {
      panel.append(
        el('p', { class: 'session-note', text: this.t('files.truncated') }),
      );
    }

    if (this.filePreview) {
      const preview = el('div', { class: 'file-preview' });
      const previewHead = el('div', { class: 'panel-head' });
      previewHead.append(
        el('span', {
          class: 'file-preview-name',
          text: this.filePreview.name,
        }),
        el('button', {
          class: 'btn btn-ghost',
          text: this.t('common.close'),
          attrs: { type: 'button' },
          on: {
            click: () => {
              this.filePreview = null;
              this.scheduleRender();
            },
          },
        }),
      );
      preview.append(previewHead);
      if (this.filePreview.kind === 'image') {
        preview.append(
          el('img', {
            class: 'file-image',
            attrs: {
              src: this.filePreview.src,
              alt: this.t('files.imageAlt', { name: this.filePreview.name }),
            },
          }),
        );
      } else {
        preview.append(
          el('pre', {
            class: 'file-text',
            text: this.filePreview.content,
          }),
        );
      }
      panel.append(preview);
    }
    return panel;
  }

  private renderChangesPanel(
    screen: Extract<Screen, { name: 'session' }>,
  ): HTMLElement {
    const panel = el('section', {
      class: 'session-files',
      attrs: { 'aria-label': this.t('session.changes') },
    });
    const head = el('div', { class: 'panel-head' });
    head.append(
      el('h2', { class: 'phone-section-title', text: this.t('changes.title') }),
    );
    head.append(
      el('button', {
        class: 'btn btn-ghost',
        text: this.t('files.refresh'),
        attrs: { type: 'button' },
        on: {
          click: () =>
            void this.loadChanges(screen.computer, screen.session.id),
        },
      }),
    );
    panel.append(head);

    if (this.changesLoading) {
      panel.append(
        el('p', {
          class: 'session-note',
          text: this.t('changes.loading'),
          attrs: { role: 'status', 'aria-busy': 'true' },
        }),
      );
      return panel;
    }
    if (this.changesError) {
      panel.append(this.noticeEl(this.changesError));
      return panel;
    }
    if (!this.changes) {
      return panel;
    }
    const changes = this.changes;
    if (!changes.git) {
      panel.append(
        el('p', { class: 'session-note', text: this.t('changes.notGit') }),
      );
      return panel;
    }

    const totals = changeTotals(changes);
    panel.append(
      el('p', {
        class: 'change-summary',
        text: this.t('changes.summary', {
          files: totals.files,
          additions: totals.additions,
          deletions: totals.deletions,
        }),
      }),
    );
    if (changes.error) {
      panel.append(
        this.noticeEl({
          key: 'changes.failed',
          tone: 'error',
          detail: changes.error,
        }),
      );
    }
    if (changesEmpty(changes)) {
      panel.append(
        el('p', { class: 'session-note', text: this.t('changes.empty') }),
      );
    } else {
      const list = el('ul', { class: 'change-list' });
      for (const file of changes.files) {
        const expanded = this.openDiff === file.path;
        const row = el('button', {
          class: 'change-row',
          attrs: {
            type: 'button',
            'aria-expanded': expanded ? 'true' : 'false',
            'aria-label': this.t('changes.open', { path: file.path }),
          },
          on: {
            click: () => {
              this.openDiff = expanded ? null : file.path;
              this.scheduleRender();
            },
          },
        });
        row.append(
          el('span', { class: 'change-path', text: file.path }),
          el('span', { class: 'change-stat' }, [
            el('b', { text: `+${file.additions}` }),
            el('i', { text: `−${file.deletions}` }),
          ]),
        );
        const item = el('li', {}, [row]);
        if (expanded) {
          if (file.binary) {
            item.append(
              el('p', {
                class: 'session-note',
                text: this.t('changes.binary'),
              }),
            );
          } else {
            item.append(
              el(
                'pre',
                { class: 'diff' },
                diffRows(file.diff, diffLineKind).map((row) =>
                  el('span', {
                    class: `diff-line is-${row.kind}`,
                    text: row.text,
                  }),
                ),
              ),
            );
          }
          if (file.diffTruncated) {
            item.append(
              el('p', {
                class: 'session-note',
                text: this.t('changes.diffTruncated'),
              }),
            );
          }
        }
        list.append(item);
      }
      panel.append(list);
    }
    if (changes.truncated) {
      panel.append(
        el('p', { class: 'session-note', text: this.t('changes.truncated') }),
      );
    }
    return panel;
  }

  private async loadFiles(
    computer: ComputerRecord,
    sessionId: string,
    path: string,
  ): Promise<void> {
    this.filesLoading = true;
    this.filesError = null;
    this.scheduleRender();
    try {
      const list = (await phone.request(computer.id, 'files.list', {
        sessionId,
        path,
      })) as FileList;
      if (
        this.screen.name !== 'session' ||
        this.screen.session.id !== sessionId
      ) {
        return;
      }
      this.filesList = list;
      this.filesPath = list.path;
      this.filePreview = null;
    } catch {
      this.filesError = { key: 'files.failed', tone: 'error' };
    } finally {
      if (
        this.screen.name === 'session' &&
        this.screen.session.id === sessionId
      ) {
        this.filesLoading = false;
      }
      this.scheduleRender();
    }
  }

  private async openFile(
    screen: Extract<Screen, { name: 'session' }>,
    entry: FileEntry,
  ): Promise<void> {
    if (entry.directory) {
      await this.loadFiles(screen.computer, screen.session.id, entry.path);
      return;
    }
    this.filesError = null;
    this.filePreview = null;
    this.scheduleRender();
    try {
      if (isImageName(entry.name)) {
        const image = await phone.readImage(
          screen.computer.id,
          screen.session.id,
          entry.path,
        );
        if (
          this.screen.name !== 'session' ||
          this.screen.session.id !== screen.session.id
        ) {
          return;
        }
        this.filePreview = {
          kind: 'image',
          name: entry.path,
          src: imageDataUrl(image.contentType, image.data),
        };
      } else {
        const file = (await phone.request(screen.computer.id, 'files.read', {
          sessionId: screen.session.id,
          path: entry.path,
        })) as { content: string };
        if (
          this.screen.name !== 'session' ||
          this.screen.session.id !== screen.session.id
        ) {
          return;
        }
        this.filePreview = {
          kind: 'text',
          name: entry.path,
          content: file.content,
        };
      }
    } catch {
      if (
        this.screen.name !== 'session' ||
        this.screen.session.id !== screen.session.id
      ) {
        return;
      }
      this.filesError = {
        key: isImageName(entry.name) ? 'files.notImage' : 'files.notText',
        tone: 'error',
      };
    }
    this.scheduleRender();
  }

  private async loadChanges(
    computer: ComputerRecord,
    sessionId: string,
  ): Promise<void> {
    this.changesLoading = true;
    this.changesError = null;
    this.scheduleRender();
    try {
      const changes = (await phone.request(computer.id, 'diff.read', {
        sessionId,
      })) as SessionChanges;
      if (
        this.screen.name !== 'session' ||
        this.screen.session.id !== sessionId
      ) {
        return;
      }
      this.changes = changes;
    } catch {
      this.changesError = { key: 'changes.failed', tone: 'error' };
    } finally {
      if (
        this.screen.name === 'session' &&
        this.screen.session.id === sessionId
      ) {
        this.changesLoading = false;
      }
      this.scheduleRender();
    }
  }

  private async pickUpload(
    screen: Extract<Screen, { name: 'session' }>,
  ): Promise<void> {
    if (this.uploadBusy) {
      return;
    }
    this.uploadBusy = true;
    this.uploadNotice = null;
    this.scheduleRender();
    try {
      const uploaded = await phone.pickUpload(
        screen.computer.id,
        screen.session.id,
      );
      this.uploadNotice = {
        key: 'files.uploadDone',
        values: { name: uploaded.name },
        tone: 'ok',
      };
    } catch (error) {
      const key = uploadNoticeFor(asError(error).code);
      if (key !== null) {
        this.uploadNotice = { key, tone: 'error' };
      }
    } finally {
      this.uploadBusy = false;
      this.scheduleRender();
    }
  }

  private renderSessionCard(session: Session): HTMLElement | null {
    const permission = session.pendingPermission;
    if (permission) {
      const card = el('section', {
        class: 'phone-card session-card',
        attrs: { 'aria-label': this.t('permission.title') },
      });
      card.append(
        el('span', { class: 'phone-card-label', text: this.t('permission.title') }),
        el('p', {
          class: 'card-line',
          text: `${permission.tool} · ${permission.summary}`,
        }),
      );
      const actions = el('div', { class: 'card-actions' });
      actions.append(
        el('button', {
          class: 'btn btn-secondary',
          text: this.t('permission.deny'),
          attrs: { type: 'button' },
          on: {
            click: () => {
              void this.respondPermission(session, 'deny');
            },
          },
        }),
        el('button', {
          class: 'btn btn-primary',
          text: this.t('permission.allow'),
          attrs: { type: 'button' },
          on: {
            click: () => {
              void this.respondPermission(session, 'allow');
            },
          },
        }),
      );
      card.append(actions);
      return card;
    }

    const question = pendingQuestion(this.questions, session.id);
    if (question) {
      const card = el('section', {
        class: 'phone-card session-card',
        attrs: { 'aria-label': this.t('question.title') },
      });
      card.append(
        el('span', { class: 'phone-card-label', text: this.t('question.title') }),
        el('p', { class: 'card-line', text: question.text }),
      );
      const form = el('form', { class: 'question-form' });
      const input = el('input', {
        class: 'text-input',
        attrs: {
          type: 'text',
          id: `question-answer-${question.id}`,
          placeholder: this.t('question.placeholder'),
          'aria-label': this.t('question.answer'),
        },
      });
      if (question.options.length > 0) {
        const options = el('div', { class: 'question-options' });
        for (const option of question.options) {
          options.append(
            el('button', {
              class: 'chip',
              text: option,
              attrs: { type: 'button' },
              on: {
                click: () => {
                  void this.answerQuestion(question.id, option);
                },
              },
            }),
          );
        }
        form.append(options);
      }
      form.append(
        input,
        el('button', {
          class: 'btn btn-primary',
          text: this.t('question.send'),
          attrs: { type: 'submit' },
        }),
      );
      form.addEventListener('submit', (event) => {
        event.preventDefault();
        const answer = input.value.trim();
        if (answer) {
          void this.answerQuestion(question.id, answer);
        }
      });
      card.append(form);
      return card;
    }

    const request = pendingRequest(this.requests, session.id);
    if (request) {
      const card = el('section', {
        class: 'phone-card session-card',
        attrs: { 'aria-label': this.t('request.title') },
      });
      card.append(
        el('span', { class: 'phone-card-label', text: this.t('request.title') }),
        el('p', {
          class: 'card-line',
          text: this.t('request.body', {
            project: request.projectName,
            name: request.name ?? request.parentSessionName,
          }),
        }),
      );
      const actions = el('div', { class: 'card-actions' });
      actions.append(
        el('button', {
          class: 'btn btn-secondary',
          text: this.t('request.deny'),
          attrs: { type: 'button' },
          on: { click: () => void this.answerRequest(request.id, false) },
        }),
        el('button', {
          class: 'btn btn-primary',
          text: this.t('request.approve'),
          attrs: { type: 'button' },
          on: { click: () => void this.answerRequest(request.id, true) },
        }),
      );
      card.append(actions);
      return card;
    }
    return null;
  }

  private renderComposer(session: Session, view: SessionView): HTMLElement {
    const wrap = el('section', {
      class: 'session-composer-wrap',
      attrs: { 'aria-label': this.t('keys.title') },
    });
    const rail = this.railStates.get(session.id) ?? {
      open: true,
      scroll: 0,
    };
    if (rail.open) {
      wrap.append(this.renderKeyRail(session));
    }

    const form = el('form', { class: 'session-composer' });
    const keyboard = el('button', {
      class: 'phone-icon composer-keyboard',
      attrs: {
        type: 'button',
        id: `keyboard-${session.id}`,
        'aria-label': this.t(
          rail.open ? 'keys.hide' : 'keys.show',
        ),
        'aria-pressed': rail.open ? 'true' : 'false',
      },
      on: {
        click: () => {
          this.railStates.set(session.id, {
            open: !rail.open,
            scroll: rail.scroll,
          });
          this.scheduleRender();
        },
      },
    });
    keyboard.innerHTML = KEYBOARD_ICON;

    const input = el('input', {
      class: 'text-input session-input',
      attrs: {
        type: 'text',
        id: `session-input-${session.id}`,
        placeholder: this.t(
          view === 'messages'
            ? 'session.promptPlaceholder'
            : 'session.inputPlaceholder',
        ),
        'aria-label': this.t(
          view === 'messages' ? 'session.promptLabel' : 'session.input',
        ),
        autocomplete: 'off',
        autocapitalize: 'sentences',
        spellcheck: 'false',
        enterkeyhint: 'send',
      },
    });
    input.value = this.drafts.get(session.id) ?? '';
    input.addEventListener('input', () => {
      this.drafts.set(session.id, input.value);
    });
    form.append(keyboard, input);
    form.append(
      el('button', {
        class: 'btn btn-primary composer-send',
        text: this.t('session.send'),
        attrs: { type: 'submit' },
      }),
    );
    form.addEventListener('submit', (event) => {
      event.preventDefault();
      const value = input.value;
      this.drafts.delete(session.id);
      input.value = '';
      void this.sendInput(session, `${value}\r`);
    });
    wrap.append(form);
    return wrap;
  }

  private renderKeyRail(session: Session): HTMLElement {
    const latches = this.latches.get(session.id) ?? noModifiers();
    const state = this.railStates.get(session.id) ?? {
      open: true,
      scroll: 0,
    };
    const rail = el('div', {
      class: 'key-rail',
      attrs: {
        role: 'toolbar',
        'aria-label': this.t('keys.title'),
      },
    });
    for (const modifier of ['shift', 'ctrl', 'alt'] as const) {
      const on = latches[modifier];
      const label =
        modifier === 'shift'
          ? this.t('keys.shift')
          : modifier === 'ctrl'
            ? this.t('keys.ctrl')
            : this.t('keys.alt');
      rail.append(
        el('button', {
          class: on ? 'key-chip latch is-on' : 'key-chip latch',
          text: modifier === 'shift' ? '⇧' : modifier === 'ctrl' ? '^' : '⌥',
          attrs: {
            type: 'button',
            id: `latch-${session.id}-${modifier}`,
            'aria-label': label,
            'aria-pressed': on ? 'true' : 'false',
          },
          on: {
            click: () => {
              this.latches.set(
                session.id,
                toggleModifier(latches, modifier),
              );
              this.scheduleRender();
            },
          },
        }),
      );
    }
    rail.append(el('span', { class: 'key-separator', attrs: { 'aria-hidden': 'true' } }));

    const keys = railKeys(session.shortcuts ?? [], this.savedKeys);
    for (const entry of keys) {
      rail.append(this.keyChip(session, entry, latches));
    }
    rail.append(
      el('button', {
        class: 'key-chip add-key',
        text: '+',
        attrs: {
          type: 'button',
          id: `key-add-${session.id}`,
          'aria-label': this.t('keys.add'),
        },
        on: {
          click: () => {
            this.keyDraft = { name: '', modifiers: [] };
            this.overlay = { kind: 'keys', sessionId: session.id };
            this.scheduleRender();
          },
        },
      }),
    );
    rail.scrollLeft = state.scroll;
    rail.addEventListener('scroll', () => {
      this.railStates.set(session.id, {
        open: state.open,
        scroll: rail.scrollLeft,
      });
    });
    return rail;
  }

  private keyChip(
    session: Session,
    entry: RailKey,
    latches: KeyModifiers,
  ): HTMLElement {
    const name = entry.nameKey
      ? this.t(entry.nameKey as MessageKey)
      : entry.name;
    const label = entry.description
      ? `${name}: ${entry.description}`
      : name;
    const button = el('button', {
      class: 'key-chip',
      text: entry.label,
      attrs: {
        type: 'button',
        id: `key-${session.id}-${entry.id}`,
        'aria-label': label,
        title: entry.description ?? name,
      },
    });
    if (entry.description) {
      button.append(
        el('span', {
          class: 'key-purpose',
          text: entry.description,
          attrs: { 'aria-hidden': 'true' },
        }),
      );
    }
    button.addEventListener('pointerdown', (event) => {
      event.preventDefault();
      this.suppressClick = true;
      this.sendRailKey(session, entry, latches);
      if (isRepeatable(entry.key)) {
        // The latch belongs to the first key only; held repeats are plain.
        this.startRepeat(() =>
          this.sendRailKey(session, entry, noModifiers()),
        );
      }
    });
    for (const name of ['pointerup', 'pointerleave', 'pointercancel']) {
      button.addEventListener(name, () => this.stopRepeat());
    }
    button.addEventListener('click', () => {
      if (this.suppressClick) {
        this.suppressClick = false;
        return;
      }
      this.sendRailKey(session, entry, latches);
    });
    return button;
  }

  private keyStorage(): Storage | null {
    try {
      return window.localStorage;
    } catch {
      return null;
    }
  }

  private currentComputer(): ComputerRecord | null {
    const screen = this.screen;
    if (
      screen.name === 'sessions' ||
      screen.name === 'session' ||
      screen.name === 'newSession'
    ) {
      return screen.computer;
    }
    return null;
  }

  private sessionById(id: string): Session | null {
    const screen = this.screen;
    if (screen.name === 'session' && screen.session.id === id) {
      return screen.session;
    }
    if (screen.name === 'sessions') {
      return screen.sessions.find((session) => session.id === id) ?? null;
    }
    return this.sessionCache.find((session) => session.id === id) ?? null;
  }

  private async sendInput(session: Session, data: string): Promise<void> {
    const computer = this.currentComputer();
    if (!computer) {
      return;
    }
    try {
      await phone.request(computer.id, 'sessions.input', {
        sessionId: session.id,
        data,
      });
    } catch (error) {
      this.showSessionError(error);
    }
  }

  private sendRailKey(
    session: Session,
    entry: RailKey,
    latches: KeyModifiers,
  ): void {
    const data = encodeRailKey(entry, latches);
    this.latches.set(session.id, noModifiers());
    this.scheduleRender();
    void this.sendInput(session, data);
  }

  private startRepeat(send: () => void): void {
    this.stopRepeat();
    this.repeatDelay = setTimeout(() => {
      this.repeatTimer = setInterval(send, 80);
    }, 400);
  }

  private stopRepeat(): void {
    if (this.repeatDelay) {
      clearTimeout(this.repeatDelay);
      this.repeatDelay = null;
    }
    if (this.repeatTimer) {
      clearInterval(this.repeatTimer);
      this.repeatTimer = null;
    }
  }

  private async respondPermission(
    session: Session,
    decision: 'allow' | 'deny',
  ): Promise<void> {
    const computer = this.currentComputer();
    const pending = session.pendingPermission;
    if (!computer || !pending) {
      return;
    }
    try {
      await phone.request(computer.id, 'sessions.permission', {
        sessionId: session.id,
        requestId: pending.requestId,
        decision,
      });
      const screen = this.screen;
      if (screen.name === 'session' && screen.session.id === session.id) {
        this.screen = {
          ...screen,
          session: { ...screen.session, pendingPermission: null },
        };
        this.scheduleRender();
      }
    } catch (error) {
      this.showSessionError(error);
    }
  }

  private async answerQuestion(id: string, answer: string): Promise<void> {
    const computer = this.currentComputer();
    if (!computer) {
      return;
    }
    try {
      await phone.request(computer.id, 'questions.answer', { id, answer });
      this.questions = this.questions.filter((question) => question.id !== id);
      this.scheduleRender();
    } catch (error) {
      this.showSessionError(error);
    }
  }

  private async answerRequest(id: string, allow: boolean): Promise<void> {
    const computer = this.currentComputer();
    if (!computer) {
      return;
    }
    try {
      const result = (await phone.request(
        computer.id,
        allow ? 'sessionRequests.approve' : 'sessionRequests.deny',
        { id, allowFurther: false },
      )) as { request?: SessionRequest; session?: Session | null };
      this.requests = this.requests.filter((request) => request.id !== id);
      if (result.session) {
        const child = result.session;
        this.sessionCache = mergeSession(this.sessionCache, child);
        const screen = this.screen;
        if (screen.name === 'session') {
          this.screen = {
            ...screen,
            sessions: mergeSession(screen.sessions, child),
          };
        }
      }
      this.scheduleRender();
    } catch (error) {
      this.showSessionError(error);
    }
  }

  private openClose(sessionId: string): void {
    this.overlay = { kind: 'close', sessionId };
    this.scheduleRender();
  }

  private async confirmClose(): Promise<void> {
    const computer = this.currentComputer();
    const overlay = this.overlay;
    if (!computer || overlay?.kind !== 'close' || !overlay.sessionId) {
      return;
    }
    const session = this.sessionById(overlay.sessionId);
    if (!session) {
      this.overlay = null;
      this.scheduleRender();
      return;
    }
    try {
      await phone.request(computer.id, 'sessions.close', {
        sessionId: session.id,
      });
      this.overlay = null;
      const screen = this.screen;
      if (screen.name === 'session' && screen.session.id === session.id) {
        this.sessionCache = this.sessionCache.map((entry) =>
          entry.id === session.id
            ? { ...entry, status: 'closed' }
            : entry,
        );
        this.goSessions(computer);
      } else {
        this.scheduleRender();
      }
    } catch (error) {
      this.overlay = null;
      this.showSessionError(error);
    }
  }

  private async openSwitch(sessionId: string): Promise<void> {
    const computer = this.currentComputer();
    this.switchFilter = '';
    this.switchLoading = Boolean(computer);
    this.overlay = { kind: 'switch', sessionId };
    this.scheduleRender();
    if (!computer) {
      return;
    }
    try {
      const data = (await phone.request(
        computer.id,
        'bootstrap',
      )) as BootstrapResponse;
      this.applySnapshot(data);
      const current = this.sessionById(sessionId);
      const screen = this.screen;
      if (
        current &&
        screen.name === 'session' &&
        screen.session.id === current.id
      ) {
        this.screen = {
          ...screen,
          session: current,
          sessions: this.sessionCache,
        };
      }
    } catch {
      // The sheet still shows the configs from the last snapshot.
    } finally {
      this.switchLoading = false;
      this.scheduleRender();
    }
  }

  private async chooseConfig(configId: string): Promise<void> {
    const computer = this.currentComputer();
    const overlay = this.overlay;
    if (
      !computer ||
      overlay?.kind !== 'switch' ||
      !overlay.sessionId
    ) {
      return;
    }
    const session = this.sessionById(overlay.sessionId);
    if (!session || session.configId === configId) {
      return;
    }
    try {
      const updated = (await phone.request(
        computer.id,
        'sessions.config',
        { sessionId: session.id, configId },
      )) as Session;
      this.overlay = null;
      this.sessionCache = mergeSession(this.sessionCache, updated);
      const screen = this.screen;
      if (screen.name === 'session' && screen.session.id === session.id) {
        this.screen = {
          ...screen,
          session: updated,
          sessions: mergeSession(screen.sessions, updated),
        };
      }
      this.scheduleRender();
    } catch (error) {
      this.overlay = null;
      this.showSessionError(error);
    }
  }

  private async openUsage(): Promise<void> {
    const computer = this.currentComputer();
    this.overlay = { kind: 'usage' };
    this.usageLoading = true;
    this.usageError = null;
    this.scheduleRender();
    if (!computer) {
      this.usageLoading = false;
      this.scheduleRender();
      return;
    }
    try {
      const providers = (await phone.request(
        computer.id,
        'accounts.list',
      )) as Provider[];
      this.providers = providers;
    } catch (error) {
      this.usageError = noticeFor(asError(error));
    } finally {
      this.usageLoading = false;
      this.scheduleRender();
    }
  }

  private async openNewSession(computer: ComputerRecord): Promise<void> {
    this.screen = {
      name: 'newSession',
      computer,
      form: {
        projectId: null,
        projectLabel: '',
        configId: null,
        configLabel: '',
        sourceBranch: null,
        sourceBranchLabel: '',
        name: '',
        firstPrompt: '',
      },
      loading: true,
      busy: false,
      connection: this.connectionStates.get(computer.id),
    };
    this.render();
    try {
      const data = (await phone.request(
        computer.id,
        'bootstrap',
      )) as BootstrapResponse;
      this.applySnapshot(data);
      const screen = this.screen;
      if (screen.name !== 'newSession') {
        return;
      }
      this.screen = { ...screen, loading: false };
      this.render();
    } catch (error) {
      const screen = this.screen;
      if (screen.name !== 'newSession') {
        return;
      }
      this.screen = {
        ...screen,
        loading: false,
        error: noticeFor(asError(error)),
      };
      this.render();
    }
  }

  private async startSession(form: StartForm): Promise<void> {
    const screen = this.screen;
    if (screen.name !== 'newSession' || screen.busy) {
      return;
    }
    const input = buildStartInput(form);
    if (!input) {
      this.screen = {
        ...screen,
        error: { key: 'error.startIncomplete', tone: 'error' },
      };
      this.render();
      return;
    }
    this.screen = { ...screen, busy: true, error: undefined };
    this.render();
    try {
      const created = (await phone.request(
        screen.computer.id,
        'sessions.create',
        input,
      )) as Session;
      this.sessionCache = mergeSession(this.sessionCache, created);
      await this.openSession(screen.computer, created, this.sessionCache);
    } catch (error) {
      const current = this.screen;
      if (current.name !== 'newSession') {
        return;
      }
      this.screen = {
        ...current,
        busy: false,
        error: noticeFor(asError(error)),
      };
      this.render();
    }
  }

  private renderNewSession(
    screen: Extract<Screen, { name: 'newSession' }>,
  ): HTMLElement {
    const main = el('main', { class: 'phone-list' });
    if (screen.loading) {
      main.append(
        el('p', {
          class: 'phone-empty',
          text: this.t('app.loading'),
          attrs: { 'aria-busy': 'true' },
        }),
      );
      return main;
    }
    if (screen.error) {
      main.append(this.noticeEl(screen.error));
    }

    const form = el('form', { class: 'new-session-form' });
    const computer = screen.computer;
    const formState = screen.form;

    let configCombo: ReturnType<typeof combo> | null = null;
    let branchCombo: ReturnType<typeof combo> | null = null;

    const configFor = (id: string | null): AiConfig | null =>
      id ? this.configs.find((config) => config.id === id) ?? null : null;
    const projectFor = (id: string | null): Project | null =>
      id ? this.projects.find((project) => project.id === id) ?? null : null;

    const projectHandle = combo({
      id: 'new-session-project',
      label: this.t('newSession.project'),
      value: formState.projectId,
      selectedLabel: formState.projectLabel,
      placeholder: this.t('newSession.projectPlaceholder'),
      required: true,
      loadingLabel: this.t('combobox.loading'),
      emptyLabel: this.t('combobox.noMatches'),
      failedLabel: this.t('combobox.loadFailed'),
      load: async (query) =>
        filterChoices(this.projects, query, (project) => project.name).map(
          (project) => ({
            id: project.id,
            label: project.name,
            meta: project.folder,
          }),
        ),
      onChange: (id) => {
        const project = projectFor(id);
        formState.projectId = id;
        formState.projectLabel = project?.name ?? '';
        const config = configFor(project?.defaultConfigId ?? null);
        formState.configId = config?.id ?? null;
        formState.configLabel = config ? configChoiceLabel(config) : '';
        configCombo?.setValue(formState.configId, formState.configLabel);
        formState.sourceBranch = project?.defaultSourceBranch ?? null;
        formState.sourceBranchLabel = formState.sourceBranch ?? '';
        branchCombo?.setValue(formState.sourceBranch, formState.sourceBranchLabel);
      },
    });

    configCombo = combo({
      id: 'new-session-config',
      label: this.t('newSession.config'),
      value: formState.configId,
      selectedLabel: formState.configLabel,
      placeholder: this.t('newSession.configPlaceholder'),
      required: true,
      loadingLabel: this.t('combobox.loading'),
      emptyLabel: this.t('combobox.noMatches'),
      failedLabel: this.t('combobox.loadFailed'),
      load: async (query) =>
        filterChoices(this.configs, query, configChoiceLabel).map((config) => ({
          id: config.id,
          label: config.name,
          meta: `${config.client}${config.model ? ` · ${config.model}` : ''}`,
        })),
      onChange: (id) => {
        const config = configFor(id);
        formState.configId = id;
        formState.configLabel = config ? configChoiceLabel(config) : '';
      },
    });

    branchCombo = combo({
      id: 'new-session-branch',
      label: this.t('newSession.sourceBranch'),
      value: formState.sourceBranch,
      selectedLabel: formState.sourceBranchLabel,
      placeholder: this.t('newSession.sourceBranchPlaceholder'),
      loadingLabel: this.t('combobox.loading'),
      emptyLabel: this.t('combobox.noMatches'),
      failedLabel: this.t('combobox.loadFailed'),
      load: async (query, signal) => {
        const project = projectFor(formState.projectId);
        if (!project) {
          return [];
        }
        const branches = (await phone.request(
          computer.id,
          'branches.list',
          { folder: project.folder, q: query || undefined },
        )) as { name: string; current: boolean }[];
        signal.throwIfAborted();
        return branches.map((branch) => ({
          id: branch.name,
          label: branch.name,
          meta: branch.current ? this.t('newSession.currentTag') : undefined,
        }));
      },
      onChange: (id) => {
        formState.sourceBranch = id;
        formState.sourceBranchLabel = id ?? '';
      },
    });

    form.append(projectHandle.element, configCombo.element, branchCombo.element);

    const nameInput = el('input', {
      class: 'text-input',
      attrs: {
        type: 'text',
        id: 'new-session-name',
        placeholder: this.t('newSession.namePlaceholder'),
      },
    });
    nameInput.value = formState.name;
    nameInput.addEventListener('input', () => {
      formState.name = nameInput.value;
    });
    const nameField = el('div', { class: 'field' }, [
      el('label', {
        class: 'field-label',
        text: this.t('newSession.name'),
        attrs: { for: 'new-session-name' },
      }),
      nameInput,
    ]);

    const promptInput = el('textarea', {
      class: 'text-input prompt-input',
      attrs: {
        id: 'new-session-prompt',
        rows: '3',
        placeholder: this.t('newSession.firstPromptPlaceholder'),
      },
    });
    promptInput.value = formState.firstPrompt;
    promptInput.addEventListener('input', () => {
      formState.firstPrompt = promptInput.value;
    });
    const promptField = el('div', { class: 'field' }, [
      el('label', {
        class: 'field-label',
        text: this.t('newSession.firstPrompt'),
        attrs: { for: 'new-session-prompt' },
      }),
      promptInput,
    ]);

    form.append(nameField, promptField);

    const actions = el('div', { class: 'screen-actions' });
    actions.append(
      el('button', {
        class: 'btn btn-secondary',
        text: this.t('common.cancel'),
        attrs: { type: 'button' },
        on: { click: () => this.goSessions(computer) },
      }),
      el('button', {
        class: 'btn btn-primary',
        text: screen.busy
          ? this.t('newSession.submitting')
          : this.t('newSession.submit'),
        attrs: { type: 'submit' },
      }),
    );
    if (screen.busy) {
      actions.querySelectorAll('button').forEach((button) => {
        button.disabled = true;
      });
    }
    form.append(actions);
    form.addEventListener('submit', (event) => {
      event.preventDefault();
      void this.startSession(formState);
    });
    main.append(form);
    return main;
  }

  private renderOverlay(
    overlay: { kind: OverlayKind; sessionId?: string },
  ): HTMLElement {
    const layer = el('div', { class: 'sheet-layer' });
    layer.append(
      el('button', {
        class: 'sheet-scrim',
        attrs: { type: 'button', 'aria-label': this.t('common.close') },
        on: {
          click: () => {
            this.overlay = null;
            this.scheduleRender();
          },
        },
      }),
    );
    const sheet = el('div', {
      class: 'sheet',
      attrs: {
        role: 'dialog',
        'aria-modal': 'true',
        'aria-label': this.t(
          overlay.kind === 'keys'
            ? 'keys.title'
            : overlay.kind === 'switch'
              ? 'session.switch'
              : overlay.kind === 'close'
                ? 'close.title'
                : 'usage.title',
        ),
      },
    });
    const close = () => {
      this.overlay = null;
      this.scheduleRender();
    };
    const head = el('div', { class: 'sheet-head' }, [
      el('h2', {
        class: 'sheet-title',
        text: this.t(
          overlay.kind === 'keys'
            ? 'keys.title'
            : overlay.kind === 'switch'
              ? 'session.switch'
              : overlay.kind === 'close'
                ? 'close.title'
                : 'usage.title',
        ),
      }),
      el('button', {
        class: 'sheet-close',
        text: '×',
        attrs: { type: 'button', 'aria-label': this.t('common.close') },
        on: { click: close },
      }),
    ]);
    sheet.append(head);

    if (overlay.kind === 'keys') {
      this.renderKeysSheet(sheet, overlay.sessionId, close);
    } else if (overlay.kind === 'switch') {
      this.renderSwitchSheet(sheet, overlay.sessionId, close);
    } else if (overlay.kind === 'close') {
      this.renderCloseSheet(sheet, overlay.sessionId, close);
    } else {
      this.renderUsageSheet(sheet);
    }

    layer.append(sheet);
    return layer;
  }

  private renderKeysSheet(
    sheet: HTMLElement,
    sessionId: string | undefined,
    close: () => void,
  ): void {
    const session = sessionId ? this.sessionById(sessionId) : null;
    const body = el('div', { class: 'sheet-body' });
    const shortcuts = session?.shortcuts ?? [];
    if (shortcuts.length > 0) {
      body.append(
        el('h3', { class: 'sheet-section', text: this.t('keys.agent') }),
      );
      const list = el('ul', { class: 'key-list' });
      for (const shortcut of shortcuts) {
        list.append(
          el('li', { class: 'key-list-row' }, [
            el('span', { class: 'key-list-name', text: shortcut.label }),
            el('span', {
              class: 'key-list-purpose',
              text: shortcut.description,
            }),
          ]),
        );
      }
      body.append(list);
    }

    body.append(
      el('h3', { class: 'sheet-section', text: this.t('keys.saved') }),
    );
    if (this.savedKeys.length === 0) {
      body.append(
        el('p', { class: 'sheet-note', text: this.t('keys.none') }),
      );
    } else {
      const list = el('ul', { class: 'key-list' });
      for (const entry of this.savedKeys) {
        const remove = el('button', {
          class: 'sheet-row-action',
          text: this.t('keys.remove'),
          attrs: {
            type: 'button',
            'aria-label': this.t('keys.removeLabel', {
              name: savedKeyLabel(entry.key, entry.modifiers),
            }),
          },
          on: {
            click: () => {
              this.savedKeys = removeSavedKey(this.savedKeys, entry.id);
              writeSavedKeys(this.keyStorage(), this.savedKeys);
              this.scheduleRender();
            },
          },
        });
        list.append(
          el('li', { class: 'key-list-row' }, [
            el('span', {
              class: 'key-list-name',
              text: savedKeyLabel(entry.key, entry.modifiers),
            }),
            remove,
          ]),
        );
      }
      body.append(list);
    }

    const full = this.savedKeys.length >= MAX_SAVED_KEYS;
    const form = el('form', { class: 'key-form' });
    const modifiers = el('div', {
      class: 'modifier-row',
      attrs: { role: 'group', 'aria-label': this.t('keys.modifiers') },
    });
    for (const modifier of ['shift', 'ctrl', 'alt'] as const) {
      const id = `saved-mod-${modifier}`;
      const input = el('input', {
        attrs: { type: 'checkbox', id, value: modifier },
      });
      input.checked = this.keyDraft.modifiers.includes(modifier);
      input.addEventListener('change', () => {
        const current = new Set(this.keyDraft.modifiers);
        if (input.checked) {
          current.add(modifier);
        } else {
          current.delete(modifier);
        }
        this.keyDraft.modifiers = [...current];
      });
      modifiers.append(
        el('label', { class: 'modifier', attrs: { for: id } }, [
          input,
          el('span', {
            text:
              modifier === 'shift'
                ? this.t('keys.shift')
                : modifier === 'ctrl'
                  ? this.t('keys.ctrl')
                  : this.t('keys.alt'),
          }),
        ]),
      );
    }
    const keyInput = el('input', {
      class: 'text-input',
      attrs: {
        type: 'text',
        id: 'saved-key-name',
        placeholder: this.t('keys.namePlaceholder'),
        'aria-label': this.t('keys.name'),
      },
    });
    keyInput.value = this.keyDraft.name;
    keyInput.addEventListener('input', () => {
      this.keyDraft.name = keyInput.value;
    });
    const error = el('p', {
      class: 'field-error',
      attrs: { role: 'alert', hidden: 'hidden' },
    });
    const add = el('button', {
      class: 'btn btn-primary',
      text: this.t('keys.add'),
      attrs: { type: 'submit' },
    });
    if (full) {
      add.disabled = true;
      keyInput.disabled = true;
    }
    form.append(modifiers, keyInput, error, add);
    if (full) {
      form.append(
        el('p', { class: 'sheet-note', text: this.t('keys.full') }),
      );
    }
    form.addEventListener('submit', (event) => {
      event.preventDefault();
      const chosen: Modifier[] = [];
      for (const modifier of ['shift', 'ctrl', 'alt'] as const) {
        const input = form.querySelector<HTMLInputElement>(
          `#saved-mod-${modifier}`,
        );
        if (input?.checked) {
          chosen.push(modifier);
        }
      }
      const saved = savedKeyFrom({
        name: keyInput.value,
        modifiers: chosen,
      });
      if (!saved) {
        error.textContent = this.t('keys.invalid');
        error.hidden = false;
        keyInput.focus();
        return;
      }
      const result = addSavedKey(this.savedKeys, saved);
      if (result.error) {
        error.textContent = this.t(
          result.error === 'full' ? 'keys.full' : 'keys.duplicate',
        );
        error.hidden = false;
        return;
      }
      this.savedKeys = result.keys;
      writeSavedKeys(this.keyStorage(), this.savedKeys);
      this.keyDraft = { name: '', modifiers: [] };
      this.scheduleRender();
    });
    body.append(form);
    sheet.append(body);
    void close;
  }

  private renderSwitchSheet(
    sheet: HTMLElement,
    sessionId: string | undefined,
    close: () => void,
  ): void {
    const session = sessionId ? this.sessionById(sessionId) : null;
    const body = el('div', { class: 'sheet-body' });
    if (!session) {
      body.append(el('p', { class: 'sheet-note', text: this.t('error.generic') }));
      sheet.append(body);
      return;
    }
    if (this.switchLoading) {
      body.append(
        el('p', {
          class: 'sheet-note',
          text: this.t('combobox.loading'),
          attrs: { 'aria-busy': 'true' },
        }),
      );
    }
    const choices = switchableConfigs(this.configs, session);
    if (choices.length === 0) {
      body.append(
        el('p', { class: 'sheet-note', text: this.t('switch.none') }),
      );
      sheet.append(body);
      return;
    }
    const filter = el('input', {
      class: 'text-input',
      attrs: {
        type: 'text',
        id: 'switch-filter',
        placeholder: this.t('switch.filter'),
        'aria-label': this.t('switch.filter'),
      },
    });
    filter.value = this.switchFilter;
    const list = el('ul', { class: 'sheet-list' });
    const renderChoices = (query: string) => {
      const visible = filterChoices(choices, query, configChoiceLabel);
      list.replaceChildren();
      for (const config of visible) {
        const provider = this.providers.find(
          (entry) => entry.id === config.providerId,
        );
        const summary = provider ? usageSummary(provider) : null;
        list.append(
          el('li', {}, [
            el('button', {
              class: 'sheet-row',
              attrs: { type: 'button' },
              on: { click: () => void this.chooseConfig(config.id) },
            }, [
              el('span', { class: 'sheet-row-name', text: config.name }),
              el('span', {
                class: 'sheet-row-meta',
                text: [
                  config.client,
                  provider?.name,
                  summary,
                ]
                  .filter(Boolean)
                  .join(' · '),
              }),
            ]),
          ]),
        );
      }
      if (visible.length === 0) {
        list.append(
          el('li', {
            class: 'sheet-note',
            text: this.t('combobox.noMatches'),
          }),
        );
      }
    };
    filter.addEventListener('input', () => {
      this.switchFilter = filter.value;
      renderChoices(filter.value);
    });
    renderChoices(this.switchFilter);
    body.append(filter, list);
    sheet.append(body);
    void close;
  }

  private renderCloseSheet(
    sheet: HTMLElement,
    sessionId: string | undefined,
    close: () => void,
  ): void {
    const session = sessionId ? this.sessionById(sessionId) : null;
    const body = el('div', { class: 'sheet-body' });
    if (!session) {
      sheet.append(body);
      return;
    }
    body.append(
      el('p', { class: 'sheet-note', text: session.name }),
      el('p', {
        class: 'sheet-note',
        text: this.t(closeBodyKey(session.kind)),
      }),
    );
    const actions = el('div', { class: 'screen-actions' });
    actions.append(
      el('button', {
        class: 'btn btn-secondary',
        text: this.t('common.cancel'),
        attrs: { type: 'button' },
        on: { click: close },
      }),
      el('button', {
        class: 'btn btn-primary',
        text: this.t('session.closeConfirm'),
        attrs: { type: 'button' },
        on: { click: () => void this.confirmClose() },
      }),
    );
    body.append(actions);
    sheet.append(body);
  }

  private renderUsageSheet(sheet: HTMLElement): void {
    const body = el('div', { class: 'sheet-body' });
    if (this.usageError) {
      body.append(this.noticeEl(this.usageError));
    }
    if (this.usageLoading) {
      body.append(
        el('p', {
          class: 'sheet-note',
          text: this.t('app.loading'),
          attrs: { 'aria-busy': 'true' },
        }),
      );
    } else if (this.providers.length === 0) {
      body.append(
        el('p', { class: 'sheet-note', text: this.t('usage.none') }),
      );
    } else {
      for (const provider of this.providers) {
        const card = el('section', {
          class: 'usage-card',
          attrs: { 'aria-label': provider.name },
        });
        card.append(
          el('div', { class: 'usage-head' }, [
            el('span', { class: 'usage-name', text: provider.name }),
            provider.sample
              ? el('span', { class: 'sample-tag', text: this.t('common.sample') })
              : null,
            el('span', {
              class: 'usage-plan',
              text: `${provider.plan} · ${provider.vendor}`,
            }),
          ]),
        );
        if (provider.rows.length === 0) {
          card.append(
            el('p', { class: 'sheet-note', text: this.t('usage.none') }),
          );
        } else {
          const list = el('ul', { class: 'usage-rows' });
          for (const row of provider.rows) {
            list.append(
              el('li', { class: 'usage-row' }, [
                el('span', { class: 'usage-label', text: row.label }),
                el('span', {
                  class: row.warn ? 'usage-value is-warn' : 'usage-value',
                  text:
                    typeof row.used === 'number'
                      ? `${row.used}%`
                      : (row.value ?? ''),
                }),
              ]),
            );
          }
          card.append(list);
        }
        if (provider.foot) {
          card.append(
            el('p', { class: 'sheet-note', text: provider.foot }),
          );
        }
        body.append(card);
      }
    }
    sheet.append(body);
  }

  private applySnapshot(data: BootstrapResponse): void {
    this.projects = data.projects ?? [];
    this.providers = data.providers ?? [];
    this.configs = data.configs ?? [];
    this.sessionCache = data.sessions ?? [];
    this.questions = data.questions ?? [];
    this.requests = data.sessionRequests ?? [];
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
            ...(row.server.tailscaleAddress
              ? [
                  el('span', {
                    class: 'demo-address is-tailnet',
                    text: row.server.tailscaleAddress,
                  }),
                ]
              : []),
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
      (screen.name === 'sessions' ||
        screen.name === 'session' ||
        screen.name === 'newSession') &&
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
        this.render();
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
      this.applySnapshot(data);
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
    this.activeComputerId = computer.id;
    try {
      const stop = await openSubscription(computer.id, id, 'events', {}, {
        onEvent: (value) => this.applyLiveEvent(value),
        onError: (error) => this.applyLiveError(computer.id, error),
      });
      if (token !== this.sessionsToken) {
        await stop();
        if (this.activeComputerId === computer.id) {
          this.activeComputerId = null;
        }
        return;
      }
      this.stopEvents = stop;
    } catch {
      // The connection state notice already explains the failure.
    }
  }

  private applyLiveEvent(value: unknown): void {
    const screen = this.notificationScreenState();
    const session = sessionFromEvent(value);
    if (session) {
      const previous =
        this.sessionCache.find((entry) => entry.id === session.id) ?? null;
      this.applySessionEvent(session);
      this.raiseNotification(
        noticeForEvent({ type: 'session', session, previous }, screen),
      );
      return;
    }
    if (typeof value !== 'object' || value === null) {
      return;
    }
    const event = value as {
      type?: unknown;
      question?: Question;
      request?: SessionRequest;
      notification?: SessionNotification;
      provider?: Provider;
    };
    if (event.type === 'question.updated' && event.question) {
      const question = event.question;
      const others = this.questions.filter((entry) => entry.id !== question.id);
      this.questions =
        question.status === 'pending' ? [...others, question] : others;
      this.raiseNotification(
        noticeForEvent({ type: 'question', question }, screen),
      );
      this.scheduleRender();
    } else if (event.type === 'request.updated' && event.request) {
      const request = event.request;
      const others = this.requests.filter((entry) => entry.id !== request.id);
      this.requests =
        request.status === 'pending' ? [...others, request] : others;
      this.raiseNotification(
        noticeForEvent({ type: 'request', request }, screen),
      );
      this.scheduleRender();
    } else if (event.type === 'notification' && event.notification) {
      this.raiseNotification(
        noticeForEvent({ type: 'notification', notification: event.notification }, screen),
      );
    } else if (event.type === 'provider.updated' && event.provider) {
      const provider = event.provider;
      this.providers = this.providers.map((entry) =>
        entry.id === provider.id ? provider : entry,
      );
      if (this.overlay?.kind === 'usage') {
        this.scheduleRender();
      }
    }
  }

  private applySessionEvent(session: Session): void {
    this.sessionCache = mergeSession(this.sessionCache, session);
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
      (screen.name === 'session' || screen.name === 'newSession') &&
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

  private applyHistoryEvent(sessionId: string, value: unknown): void {
    const screen = this.screen;
    if (screen.name !== 'session' || screen.session.id !== sessionId) {
      return;
    }
    const status = historyStatusFromEvent(value);
    if (status) {
      this.screen = {
        ...screen,
        historyStructured: status.structured,
        historyLoading: false,
        view: status.structured ? screen.view : 'terminal',
        error: undefined,
      };
      this.scheduleRender();
      return;
    }
    const item = historyItemFromEvent(value);
    if (!item) {
      return;
    }
    this.screen = {
      ...screen,
      history: mergeHistory(screen.history, item),
      historyLoading: false,
      error: undefined,
    };
    this.scheduleRender();
  }

  private failHistory(error: CommandError): void {
    const screen = this.screen;
    if (screen.name !== 'session') {
      return;
    }
    this.screen = {
      ...screen,
      historyLoading: false,
      error: noticeFor(error),
    };
    this.scheduleRender();
  }

  private endHistory(): void {
    const screen = this.screen;
    if (screen.name !== 'session') {
      return;
    }
    this.screen = { ...screen, historyLoading: false, ended: true };
    this.scheduleRender();
  }

  private closeEvents(): void {
    this.activeComputerId = null;
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

  private closeHistory(): void {
    const stop = this.stopHistory;
    this.stopHistory = null;
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

  private notificationNotice(): Notice | null {
    if (this.notificationPermission === 'denied') {
      return { key: 'notifications.denied', tone: 'neutral' };
    }
    if (this.notificationPermission === 'unavailable') {
      return { key: 'notifications.unavailable', tone: 'neutral' };
    }
    return null;
  }

  private async ensureNotifications(): Promise<void> {
    try {
      let state = await phone.notificationState();
      if (state === 'prompt' || state === 'promptWithRationale') {
        state = await phone.requestNotificationPermission();
      }
      this.notificationPermission = state;
    } catch {
      this.notificationPermission = 'unavailable';
    }
    this.scheduleRender();
  }

  private async refreshNotifications(): Promise<void> {
    try {
      this.notificationPermission = await phone.notificationState();
    } catch {
      this.notificationPermission = 'unavailable';
    }
    this.scheduleRender();
  }

  private notificationScreenState(): PhoneScreenState {
    const screen = this.screen;
    return {
      sessionId: screen.name === 'session' ? screen.session.id : null,
      appVisible:
        typeof document === 'undefined' ||
        document.visibilityState !== 'hidden',
    };
  }

  private raiseNotification(request: NotificationRequest | null): void {
    const computerId = this.activeComputerId;
    if (!request || !computerId) {
      return;
    }
    const { title, body } = this.notificationText(request);
    void phone
      .showNotification(computerId, request.sessionId, title, body)
      .catch(() => {
        // A refused or failed notification never blocks the event stream.
      });
  }

  private notificationText(request: NotificationRequest): {
    title: string;
    body: string;
  } {
    if (request.kind === 'done') {
      return {
        title: this.t('notifications.doneTitle', { name: request.sessionName }),
        body: this.t('notifications.doneBody'),
      };
    }
    if (request.kind === 'message') {
      return {
        title: this.t('notifications.messageTitle', {
          name: request.sessionName,
        }),
        body: request.message ?? '',
      };
    }
    const title = this.t('notifications.waitingTitle', {
      name: request.sessionName,
    });
    switch (request.waitingFor) {
      case 'permission':
        return { title, body: this.t('notifications.permissionBody') };
      case 'question':
        return { title, body: this.t('notifications.questionBody') };
      case 'start':
        return {
          title: this.t('notifications.requestTitle'),
          body: this.t('notifications.requestBody', {
            name: request.sessionName,
          }),
        };
      default:
        return { title, body: this.t('notifications.inputBody') };
    }
  }

  private async openNotificationTarget(
    target: NotificationTarget,
  ): Promise<void> {
    const computer = this.computers.find(
      (view) => view.computer.id === target.computerId,
    )?.computer;
    if (!computer) {
      return;
    }
    const screen = this.screen;
    const sameComputer =
      (screen.name === 'sessions' ||
        screen.name === 'session' ||
        screen.name === 'newSession') &&
      screen.computer.id === computer.id;
    if (!sameComputer) {
      await this.openSessions(computer);
    }
    let session =
      this.sessionCache.find((entry) => entry.id === target.sessionId) ?? null;
    if (!session) {
      await this.refreshBootstrap(computer);
      session =
        this.sessionCache.find((entry) => entry.id === target.sessionId) ??
        null;
    }
    if (!session) {
      return;
    }
    const current = this.screen;
    const sessions =
      current.name === 'sessions' || current.name === 'session'
        ? current.sessions
        : this.sessionCache;
    await this.openSession(computer, session, sessions);
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
    this.closeHistory();
    this.closeOutput();
    // The event stream stays open, so a session that needs the user still
    // raises a notification while the computer list is on the screen.
    this.screen = { name: 'computers' };
    this.render();
    void this.loadComputers();
  }

  private goSessions(computer: ComputerRecord): void {
    const sessions =
      this.sessionCache.length > 0
        ? this.sessionCache
        : this.screen.name === 'session'
          ? this.screen.sessions
          : [];
    this.closeHistory();
    this.closeOutput();
    this.overlay = null;
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
    this.closeHistory();
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
    void this.refreshNotifications();
    try {
      await phone.connect(computer.id);
      const data = (await phone.request(
        computer.id,
        'bootstrap',
      )) as BootstrapResponse;
      if (token !== this.sessionsToken) {
        return;
      }
      this.applySnapshot(data);
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

  private resetSessionFiles(): void {
    this.filePanel = null;
    this.filesPath = '';
    this.filesList = null;
    this.filesLoading = false;
    this.filesError = null;
    this.filePreview = null;
    this.changes = null;
    this.changesLoading = false;
    this.changesError = null;
    this.openDiff = null;
    this.uploadBusy = false;
    this.uploadNotice = null;
  }

  private async openSession(
    computer: ComputerRecord,
    session: Session,
    sessions: Session[],
  ): Promise<void> {
    this.closeHistory();
    this.closeOutput();
    this.overlay = null;
    this.expandedTools.clear();
    this.resetSessionFiles();
    this.sessionCache = mergeSession(this.sessionCache, session);
    const demoServers = sessionDemoServers(
      session.demoServers ?? [],
      session.id,
    );
    const supported = historySupported(session.client ?? null, session.kind);
    this.screen = {
      name: 'session',
      computer,
      sessions,
      session,
      output: { text: '', lastSeq: 0, dropped: false },
      history: [],
      historyStructured: supported,
      historyLoading: supported,
      view: supported ? 'messages' : 'terminal',
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
    if (supported) {
      await this.startHistory(computer, session);
    }
  }

  private async startHistory(
    computer: ComputerRecord,
    session: Session,
  ): Promise<void> {
    const id = this.nextId('history');
    try {
      const stop = await openSubscription(
        computer.id,
        id,
        'history',
        { sessionId: session.id, after: 0 },
        {
          onEvent: (value) => this.applyHistoryEvent(session.id, value),
          onError: (error) => this.failHistory(error),
          onEnd: () => this.endHistory(),
        },
      );
      if (
        this.screen.name !== 'session' ||
        this.screen.session.id !== session.id
      ) {
        await stop();
        return;
      }
      this.stopHistory = stop;
    } catch (error) {
      this.failHistory(asError(error));
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
