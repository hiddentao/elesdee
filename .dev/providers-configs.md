# Providers and AI configs

Status: decided. This replaces the account and preset model. Accounts become
providers; presets become AI configs.

## Why

One subscription or API key may drive more than one agent. DeepSeek, for
example, may run in OpenCode and in Claude Code with different models, effort
levels and command-line settings. The credential must be entered once, while
the agent setup stays separate.

## Two records

**Provider** holds the credential and where it connects.

```ts
interface Provider {
  id: string;
  name: string;              // editable, defaults to the vendor label
  vendor: string;            // anthropic, openai, google, deepseek, custom, ...
  auth: "subscription" | "key" | "local";
  server: string | null;     // base URL when the vendor or custom entry needs one
  apiStyle: "openai" | "anthropic" | null;
  sample: boolean;
  connected: boolean;         // a subscription sign-in was checked; key and local providers hold their credential
  minimumBalance: number | null; // the prepaid floor for a vendor with a balance source
  supportedClients: Client[]; // returned by the API, not stored
}
```

The key or subscription token belongs to the provider. It lives in the system
keychain under the provider id and never appears in a response, state, log or
session output. A provider does not name a client.

A vendor with a prepaid balance source, currently DeepSeek, also stores an
optional `minimumBalance`. It is provider state, not a secret. The provider
form shows the field with the currency the balance row reports, and hides it for
a vendor without a balance source. The default is 5. A balance at or below the
minimum counts as no room for auto-switch; a balance above it counts as room.
Without a minimum, a balance cannot show room, so auto-switch skips the
provider.

**AI config** is what a session runs and what the user switches between.

```ts
interface AiConfig {
  id: string;
  name: string;              // defaults to "<Provider> (<Client>)"
  providerId: string;
  client: Client;
  model: string | null;      // initial model
  effort: string | null;     // initial effort level
  args: string[];            // extra command-line arguments, one per entry
  env: EnvOverride[];        // environment overrides
  config: Record<string, string>; // agent config overrides
}
```

Several configs may share one provider. A config selects one client that the
provider supports. Model, effort, extra arguments, environment and agent config
apply to that config only and never write into a repository or the user's
global agent files.

The config name defaults to the provider name plus the client in brackets, for
example `DeepSeek (OpenCode 2)`, and the user may rename it.

## Models

The config form loads the provider's model list in real time through the
provider API, as a filterable combo box with a visible loading state. The user
types to filter and may also enter a model name that the provider does not
return. A custom name is accepted.

- OpenAI-style providers: `GET /models` with the provider key.
- Anthropic-style providers: the documented model-list endpoint when the
  vendor offers one; otherwise the known list for the vendor.
- Local servers: `GET /models` on the server URL.
- A failed read returns the known fallback list and lets the user type a name.
  It never blocks saving.

The endpoint is `GET /api/providers/{id}/models?client=...`. The result has the
model ids, a display label, and the source of the list. The core caches a
successful read briefly and coalesces concurrent reads. A key never appears in
the response or a log.

## Effort

The effort choices come from what the vendor and client support. The client
adapter owns the list, so the form shows only valid levels:

- Claude Code: `low`, `medium`, `high`, `xhigh`, `max`, plus the provider default.
- Codex: the levels its app-server accepts for `model_reasoning_effort`
  (`minimal`, `low`, `medium`, `high`), plus the provider default.
- OpenCode 2 and Antigravity: the levels their launch config accepts, or no
  effort choice when the client has none.

`GET /api/provider-options` returns these choices per client. The config stores
one of them or `null`. The core translates the level for the client:

- Claude Code: `--effort <level>`.
- Codex: `-c model_reasoning_effort="<level>"`.
- OpenCode 2 and Antigravity: pass it in their launch config or arguments when
  the client supports it; otherwise the value stays visible but does not reach
  the process, and the UI says so.

Extra command-line arguments are appended to the client command after the
generated model, effort and tool arguments. They are an argv list, never a shell
string, so no shell parsing or quoting is involved.

## Supported clients

A provider reports the clients it can run. The config form shows those clients
only.

| Vendor | Clients |
| --- | --- |
| Anthropic | Claude Code, OpenCode 2 |
| OpenAI | Codex, OpenCode 2 |
| Google | Antigravity |
| DeepSeek | Claude Code, Codex, OpenCode 2 |
| Kimi, Z.ai | Claude Code, OpenCode 2 |
| OpenRouter | OpenCode 2, Codex |
| Ollama, LM Studio | Claude Code, OpenCode 2 |
| Custom, OpenAI style | Codex, OpenCode 2 |
| Custom, Anthropic style | Claude Code, OpenCode 2 |

A vendor with two API styles uses the endpoint for the chosen client. The
provider's `server` overrides the default endpoint when set.

## Sessions

- A session runs one AI config. `configId` replaces the old account id.
- A session stores the configs it may use in `allowedConfigIds`; the default is
  the configs of the starting client plus the starting config.
- Switching, automatic switching, the switch chooser and the usage pill work on
  configs. Usage belongs to the provider and is shown for the config's provider.
- A same-client switch continues the conversation. A cross-client switch uses
  the handoff note.
- The project default is `defaultConfigId`.
- Removing a provider is refused while a config uses it. Removing a config is
  refused while an active session uses it.

## Routes

- `GET/POST /api/providers`, `PATCH/DELETE /api/providers/{id}`: configured
  providers. Search with `q`.
- `GET /api/provider-options`: the vendor catalog with auth modes, supported
  clients and each client's effort levels. This is the old `/api/providers`
  catalog under a new name.
- `GET /api/providers/{id}/models?client={client}`: the provider's model list,
  read in real time. A failed read returns a fallback list and a flag.
- `POST /api/providers/{id}/usage/refresh`, `GET /api/providers/{id}/usage`.
- `POST /api/sign-in`, `GET /api/sign-in/{id}`, `POST /api/sign-in/{id}/code`,
  `DELETE /api/sign-in/{id}`: the subscription sign-in. It starts the installed
  Claude Code or Codex login, follows the address and one-time code, and adds or
  reconnects the provider only after the client confirms the credential. The
  client keeps the token; the provider records `connected`, never a secret.
- `GET/POST /api/configs`, `PATCH/DELETE /api/configs/{id}`: AI configs. Search
  with `q`.
- Session creation takes `configId`; `POST /api/sessions/{id}/config` switches.
- `GET /api/sessions/{id}/switch-options` returns allowed configs and the next
  automatic choice.

## Settings

Settings has two related pages:

- **Providers**: add, edit and remove credentials. Adding a provider can happen
  from this page or from the AI config form. The form shows the minimum balance
  only for a vendor with a balance source.
- **AI configs**: list, add, edit and remove configs. The form chooses an
  existing provider or creates one in place, shows that provider's supported
  clients, then the name, model and effort. Environment and config overrides,
  extra command-line arguments and server overrides sit under Additional
  settings, collapsed at first.

The launch form and the session switch chooser list AI configs, not providers.
