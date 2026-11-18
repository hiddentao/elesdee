# Reading subscription usage (5-hour and weekly limits) from a separate program

Research date: 18 November 2026. I fetched every page cited here myself. Quotes are copied exactly. Where a page could not be fetched, this file says so.

---

## Short answer

| | Claude Pro/Max + Claude Code | ChatGPT Plus/Pro + Codex CLI |
|---|---|---|
| Official interface with 5h + weekly usage? | **Yes**: the status line script input, `rate_limits.five_hour` / `rate_limits.seven_day` (`used_percentage`, `resets_at`). There are also Agent SDK messages (`rate_limit_event`, plus an experimental `usage_report` sent in reply to `/usage`). | **Yes**: app-server JSON-RPC `account/rateLimits/read` and the `account/rateLimits/updated` notification (`usedPercent`, `windowDurationMins`, `resetsAt`, per bucket `primary`/`secondary`). |
| Readable by a separate program while the agent runs in its own terminal? | **Yes, indirectly.** The app sets a `statusLine` command (in user settings or through `--settings` at launch). Claude Code pipes JSON to that command on each update, and the command can write it to a file the app reads. You only get data while a session is running and after its first API response. | **Yes.** (1) The app starts its own `codex app-server` (stdio) and polls `account/rateLimits/read`. This is an account-level read, separate from the TUI. (2) Or the app hosts the app-server and the TUI connects with `codex --remote unix://…`, so the app gets `account/rateLimits/updated` pushes. |
| Stable or experimental? | The status line fields are not labeled experimental. In the SDK, `SDKUsageReport` is "experimental: its shape may change". `rate_limit_event` has no label. | `account/rateLimits/read` is **not** behind the `experimentalApi` gate, so it is on the stable surface. The WebSocket transport is "experimental and unsupported". |
| Usage APIs for organizations | Admin API, Usage & Cost, Claude Code Analytics and Enterprise Analytics are for **organizations only**. None of them reports individual Pro/Max 5h/weekly limits. | n/a |
| Policy caveats | Developers "may not collect, store, or intermediate Claude.ai credentials or session tokens". This rules out calling the private OAuth usage endpoint with the user's token. | "App-server authentication has never been permitted for commercial or hosted services". A local or open-source app "can continue using it", but OpenAI recommends Sign in with ChatGPT. |

---

## (a) Anthropic: Claude subscription (Pro / Max) with Claude Code

### 1. Official interfaces that report the 5-hour and weekly limits

#### 1a. Status line script input: `rate_limits` (the main answer)
Source: https://code.claude.com/docs/en/statusline

- How it works: the status line "runs any shell script you configure. It receives JSON session data on stdin and displays whatever your script prints".
- Fields, quoted from the "Available data" table:
  - `rate_limits.five_hour.used_percentage`, `rate_limits.seven_day.used_percentage`: "Percentage of the 5-hour or 7-day rate limit consumed, from 0 to 100"
  - `rate_limits.five_hour.resets_at`, `rate_limits.seven_day.resets_at`: "Unix epoch seconds when the 5-hour or 7-day rate limit window resets"
  - `rate_limits.spend_limit.*`: this applies only behind a Claude apps gateway, so it does not matter for individual Pro/Max.
- Example JSON from the page:
  ```json
  "rate_limits": {
    "five_hour": { "used_percentage": 23.5, "resets_at": 1738425600 },
    "seven_day": { "used_percentage": 41.2, "resets_at": 1738857600 }
  }
  ```
- When the field is present: "`rate_limits`: appears only for claude.ai Pro, Max, and Ultra subscribers, or behind a Claude apps gateway that sets a spend limit for you. Even then, it appears only after the first API response in the session. Each window (`five_hour`, `seven_day`, `spend_limit`) may be independently absent, and Claude Code drops a window once its `resets_at` time passes."
- Section "Rate limit usage" (https://code.claude.com/docs/en/statusline#rate-limit-usage): "For subscribers, the `rate_limits` object can contain a rolling `five_hour` window and a weekly `seven_day` window. Each window provides `used_percentage`, from 0 to 100, and `resets_at`, the Unix epoch seconds when the window resets."
- When the script runs again: "A new assistant message arrives", "`/compact` finishes", "A [`refreshInterval`] timer elapses, if you set one", "A rate-limit window in the data your script last received reaches its `resets_at` time". Also: "Claude Code debounces updates at 300ms".
- Trust requirement: "Until you accept the workspace trust dialog, the status line stays blank."
- The errors page points to this as the way to watch remaining usage (https://code.claude.com/docs/en/errors#youve-hit-your-session-limit): "To watch your remaining allowance, add the `rate_limits` fields to a [custom status line](…/statusline#rate-limit-usage)".

#### 1b. `/usage` command (interactive, for a person to read)
Sources: https://code.claude.com/docs/en/commands, https://code.claude.com/docs/en/costs
- `/usage`: "Show session cost, plan usage limits, and activity stats." `/cost` and `/stats` are aliases.
- `/status`: "Open the Settings interface on the Status tab, showing version, model, account, and connectivity." This is the Claude Code docs wording.
- **Mismatch to note:** the support article (https://support.claude.com/en/articles/11145838-use-claude-code-with-your-pro-or-max-plan) still says "Monitor your remaining allocation using the /status command." The current Claude Code docs put plan limits under `/usage`.
- The costs page says `/usage` loads plan limits from a server "usage endpoint": "When the request for your plan limits fails, most often because the usage endpoint is rate limited, `/usage` shows the last usage bars it loaded on this machine within the past 60 minutes". That endpoint is not documented for third-party use (see 4 below).

#### 1c. Agent SDK / headless stream messages (only if the app drives Claude Code through the SDK, not an interactive terminal)
Sources: https://code.claude.com/docs/en/agent-sdk/typescript, https://code.claude.com/docs/en/agent-sdk/python
- `SDKRateLimitEvent` (TS): "Emitted when the session encounters a rate limit." `type: "rate_limit_event"`, `rate_limit_info: { status: "allowed" | "allowed_warning" | "rejected"; resetsAt?: number; utilization?: number; ... }`.
- Python `RateLimitInfo`: "Emitted when rate limit status changes (for example, from `"allowed"` to `"allowed_warning"`)". `rate_limit_type` is one of `"five_hour", "seven_day", "seven_day_opus", "seven_day_sonnet", "overage"`. `utilization` is "Fraction of the rate limit consumed (0.0 to 1.0)". `resets_at` is "Unix timestamp when the rate limit window resets".
- `SDKUsageReport`: "Structured form of the `/usage` report, carried as `usage_report` on the `SDKAssistantMessage` that delivers a `/usage` result. ... **The type is experimental: its shape may change.**" Each row of `rate_limits.limits` has `kind` ("such as `session`, `weekly_all`, or `weekly_scoped`"), `percent` ("Share of the window used, 0-100"), `resets_at` ("ISO 8601 timestamp"), `severity` and `is_active`. It is attached only when "The session authenticates with a claude.ai credential", "The credential shows a known plan type or carries the `user:profile` scope", and "The account isn't on usage-based billing".

#### 1d. Things that do NOT carry plan-limit data (checked)
- **Hooks** (https://code.claude.com/docs/en/hooks): no `rate_limits` or usage-percentage field in any hook input. `rate_limit` appears only as an error type for `StopFailure`. The `Notification` hook has `quota_auto_resume_*` types, which only signal a limit wait or resume.
- **OpenTelemetry** (https://code.claude.com/docs/en/monitoring-usage): the metrics are `claude_code.cost.usage`, `claude_code.token.usage`, etc. There is no plan-limit or quota metric. (I searched for "quota", "utilization", "five", "weekly" and "rate limit" and found no plan-limit metric.)
- **`claude -p --output-format stream-json`** (https://code.claude.com/docs/en/headless): the `system/api_retry` event has `error: "rate_limit"` only. The documented headless events have no usage percentages. The SDK types above cover the stream.
- **CLI reference** (https://code.claude.com/docs/en/cli-reference): there is no `claude usage` subcommand. `claude auth status` gives auth JSON (`authMethod` …), not limits.

### 2. Can a separate program read it while Claude Code runs in an interactive terminal?
**Yes, through the status line.** The docs describe no direct query a separate program can make. The supported route is:
- Set `statusLine` (`"type": "command"`, `"command": "<script>"`) in user settings (`~/.claude/settings.json`). Or pass it at launch: `--settings`: "Path to a settings JSON file or an inline JSON string. Values you set here override the same keys in your `settings.json` files for this session." (https://code.claude.com/docs/en/cli-reference)
- The script gets the JSON on stdin. It can copy `rate_limits` to a file or socket for the desktop app, and still print a line for the terminal.
- Limits, all from the docs above: the data exists only while a session runs, only after its first API response, and only for Pro/Max/Ultra. Windows can be absent. Updates are event-driven, plus `refreshInterval`. The workspace trust dialog must be accepted.
- I found no official way to read the limits when no Claude Code session is running. The experimental SDK `/usage` → `usage_report` route needs an SDK session. Note the SDK policy in section 3.

### 3. Stability and policy
- Status line `rate_limits`: documented in the field table with no "experimental" label. I found no explicit stability guarantee.
- `SDKUsageReport`: explicitly "experimental: its shape may change."
- Policy, Agent SDK overview (https://code.claude.com/docs/en/agent-sdk/overview): "Unless previously approved, Anthropic does not allow third party developers to offer claude.ai login or rate limits for their products, including agents built on the Claude Agent SDK."
- Policy, Legal and compliance (https://code.claude.com/docs/en/legal-and-compliance): "developers may not collect, store, or intermediate Claude.ai credentials or session tokens — sign-in to a Claude account must complete through Anthropic's own flow." The same page also says this does not "prevent an end user from signing in to the unmodified Claude Code binary with their own Claude subscription". Running the user's own unmodified Claude Code and reading its status line input fits that description. Calling Anthropic endpoints yourself with the user's OAuth token does not.

### Organization APIs (docs.claude.com → platform.claude.com): they do not cover individual Pro/Max limits
- docs.claude.com redirects to platform.claude.com.
- Usage and Cost API (https://platform.claude.com/docs/en/manage-claude/usage-cost-api): "The Admin API isn't available to organizations on the claude.ai Team plan or to individual Claude Console accounts." It reports "historical API usage and cost data for your organization".
- Usage and Cost Analytics API (https://platform.claude.com/docs/en/manage-claude/usage-cost-analytics-api): "reports historical token usage and cost for your organization". Same Admin API restriction.
- Claude Code Analytics API (https://platform.claude.com/docs/en/manage-claude/claude-code-analytics-api): "daily aggregated usage metrics for Claude Code users, allowing organizations to analyze developer productivity". `subscription_type` is "(`enterprise` or `team`), or `null`".
- Enterprise Analytics API (https://platform.claude.com/docs/en/manage-claude/analytics-api): "available to Claude Enterprise organizations."
- Costs page (https://code.claude.com/docs/en/costs): "On an individual Pro, Max, or Ultra plan you have no organization to manage".
- **Conclusion:** none of these report an individual Pro/Max user's 5-hour or weekly usage. They are organization-level, historical (daily) usage and cost reports.

### Support articles (support.claude.com)
- https://support.claude.com/en/articles/11145838-use-claude-code-with-your-pro-or-max-plan: "Pro and Max plans have a five-hour session limit and a weekly limit." Also: "These limits are shared across Claude and Claude Code, meaning all activity in both tools counts against the same limits." Also: "Monitor your remaining allocation using the /status command."
- https://support.claude.com/en/articles/11647753-how-do-usage-and-length-limits-work: "Paid plans have two types of usage limits: a session limit that resets every five hours, and weekly limits that reset once every week." Also: "You can see how much you've used and the time remaining under Current session in Settings > Usage". Also: "Weekly limits reset at a fixed time each week that is assigned to your account." Neither article mentions any API for reading usage.

### 4. UNOFFICIAL methods (what "official only" rules out)
- **UNOFFICIAL: private OAuth usage endpoint.** CodexBar's docs (https://github.com/steipete/CodexBar/blob/main/docs/claude.md) describe `GET https://api.anthropic.com/api/oauth/usage` with `Authorization: Bearer <access_token>` and `anthropic-beta: oauth-2025-04-20`. It returns `five_hour`, `seven_day`, `seven_day_sonnet`/`seven_day_opus`, and so on. The token comes from Claude Code's Keychain or credentials file. It "Requires `user:profile` scope". Anthropic does not document this endpoint, and the legal page forbids collecting or using the user's claude.ai tokens in third-party apps.
- **UNOFFICIAL: claude.ai web API with browser cookies.** The same CodexBar doc lists `GET https://claude.ai/api/organizations/{orgId}/usage`.
- **UNOFFICIAL: screen-scraping `/usage` or `/status`** in a PTY ("CLI PTY fallback", same source).
- **UNOFFICIAL: estimating from local logs.** ccusage (https://github.com/ryoppippi/ccusage/blob/main/apps/ccusage/README.md): "ccusage reads local usage data from coding agent CLIs". `ccusage blocks` is described as "Claude Code 5-hour billing windows". This is an estimate from transcripts, not the server's percentage.

---

## (b) OpenAI: ChatGPT subscription (Plus / Pro) with the Codex CLI

URL note: https://developers.openai.com/codex/ redirects to https://learn.chatgpt.com/docs (OpenAI's docs host). For example, https://developers.openai.com/codex/app-server → https://learn.chatgpt.com/docs/app-server.

### 1. Official interface: app-server `account/rateLimits/read` and `account/rateLimits/updated`
Source: https://learn.chatgpt.com/docs/app-server (section "Auth endpoints" → "6) Rate limits (ChatGPT)")

- Purpose: the auth/account surface lets you "determine auth state, start or cancel logins, logout, inspect ChatGPT rate limits".
- Methods:
  - "`account/rateLimits/read` - fetch ChatGPT rate limits."
  - "`account/rateLimits/updated` (notify) - emitted whenever a user's ChatGPT rate limits change."
  - Related: "`account/usage/read` - fetch ChatGPT account token-activity summaries and daily buckets." These are token counts, not limits.
- Example response (abridged from the page):
  ```json
  { "rateLimits": { "limitId": "codex", "limitName": null,
      "primary": { "usedPercent": 25, "windowDurationMins": 15, "resetsAt": 1730947200 },
      "secondary": null, "rateLimitReachedType": null },
    "rateLimitsByLimitId": { "codex": { ... }, "codex_other": { ... } },
    "rateLimitResetCredits": { "availableCount": 2, "credits": [ ... ] } }
  ```
- Field notes (quoted):
  - "`rateLimits` is the backward-compatible single-bucket view."
  - "`rateLimitsByLimitId` (when present) is the multi-bucket view keyed by metered `limit_id` (for example `codex`)."
  - "`usedPercent` is current usage within the quota window."
  - "`windowDurationMins` is the quota window length."
  - "`resetsAt` is a Unix timestamp (seconds) for the next reset."
  - "`planType` is included when the server returns the ChatGPT plan associated with a bucket."
  - "`rateLimitReachedType` identifies the server-classified limit state when one has been reached."
- **5-hour vs weekly:** the docs do **not** say which of `primary` or `secondary` is the 5-hour window and which is the weekly one. The example uses illustrative 15- and 60-minute windows. A client should identify each window by `windowDurationMins` (5 h = 300, 1 week = 10080). That mapping is my inference, not a quote. The pricing page also says: "Pro plans currently have no five-hour limit." So on Pro, expect only a weekly window.
- Source code (https://github.com/openai/codex/blob/main/codex-rs/app-server-protocol/src/protocol/v2/account.rs): `RateLimitWindow { used_percent: i32, window_duration_mins: Option<i64>, resets_at: Option<i64> }`. `GetAccountRateLimitsResponse` also has `ordinary_usage_allowed`, with the doc comment "Null means unavailable; clients must not infer recovery from percentages or reset times."
- Core protocol (https://github.com/openai/codex/blob/main/codex-rs/protocol/src/protocol.rs): `TokenCountEvent { info, rate_limits: Option<RateLimitSnapshot> }`. `RateLimitWindow` has `used_percent` ("Percentage of the allowance consumed during the window. Can exceed 100."), `window_minutes` ("Rolling window duration, in minutes.") and `resets_at`. This is the internal `token_count` event. The app-server re-exposes it as `account/rateLimits/updated`.

### 2. Can a separate program read it while Codex runs in its interactive terminal?
**Yes.** The docs support two ways:
- **Option A: a separate app-server process (simplest).** "Start the server with `codex app-server` (default stdio transport)". Then "send `initialize` followed by the `initialized` notification". Then call `account/rateLimits/read`. The method reads account-level ChatGPT limits, so it does not depend on the TUI session. The docs do not explicitly talk about running alongside a separate TUI, but the method is described as an account read, not a thread read. CodexBar does exactly this; see 4.
- **Option B: the app hosts the server and the TUI attaches.** "Remote terminal UI mode lets you run app-server on one machine and connect the Codex CLI terminal interface from another." Example: `codex app-server --listen ws://127.0.0.1:4500`, then `codex --remote ws://127.0.0.1:4500`. "The `--remote` option accepts `ws://`, `wss://`, `unix://`, and `unix://PATH` endpoints." The app can then receive `account/rateLimits/updated` pushes. Note: "WebSocket transport is experimental and unsupported." A Unix socket is listed as a transport without that label.
- **Things that do NOT expose limits to another program (checked):**
  - `codex exec --json` (https://learn.chatgpt.com/docs/non-interactive-mode): "Event types include `thread.started`, `turn.started`, `turn.completed`, `turn.failed`, `item.*`, and `error`". `turn.completed` carries only `usage` token counts. The source (`codex-rs/exec/src/exec_events.rs`) confirms there is no rate-limit field.
  - Hooks (https://learn.chatgpt.com/docs/hooks): no rate-limit or usage-percentage fields. Only `PostModelRequest` has `token_usage`. (curl got 403; I read this page through WebFetch.)
  - `notify` (https://learn.chatgpt.com/docs/config-file/config-advanced): the payload has `type`, `thread-id`, `turn-id`, `cwd`, `input-messages` and `last-assistant-message`. No limits.
  - The TUI status line (https://learn.chatgpt.com/docs/developer-commands, https://learn.chatgpt.com/docs/config-file/config-reference): `tui.status_line` is an "Ordered list of TUI footer status-line item identifiers". The built-in items include "rate limits", but it does not run a user script. So it is not a data feed for another program, unlike Claude Code's status line.
  - Slash commands: CLI `/status`: "Display session configuration and token usage." `/usage`: "View account token usage or use a rate-limit reset." The pricing page says: "If you want to see your remaining limits during an active Codex CLI session, you can use `/status`." These are for a person to read only.

### 3. Stable or experimental?
- The app-server has a stable/experimental split: "Omit `capabilities` (or set `experimentalApi` to `false`) to stay on the stable API surface, and the server rejects experimental methods/fields."
- `account/rateLimits/read` and `account/rateLimits/updated` are listed **without** an "(experimental)" tag. Other methods carry one, for example "`process/spawn` … (experimental; requires `capabilities.experimentalApi`)".
- In the source (https://github.com/openai/codex/blob/main/codex-rs/app-server-protocol/src/protocol/common.rs), `GetAccountRateLimits => "account/rateLimits/read"` has no `#[experimental(...)]` attribute. Experimental methods do have one, for example `#[experimental("process/spawn")]`. So it is on the **stable** surface.
- Schemas are tied to the Codex version: "Each output is specific to the Codex version you ran" (`codex app-server generate-ts` / `generate-json-schema`).
- **Policy (same page, "Auth endpoints"):** "If you've built a local or open-source application using Codex app-server authentication, you can continue using it, though we recommend migrating to Sign in with ChatGPT so users have greater control over and visibility into their usage. … App-server authentication has never been permitted for commercial or hosted services."

### Pricing / limits docs
- https://learn.chatgpt.com/docs/pricing:
  - "The estimates below show local messages per five-hour period for Plus and Standard Business. Pro plans currently have no five-hour limit."
  - "Weekly limits may also apply."
  - "You can find your current limits in the [usage dashboard](https://chatgpt.com/codex/settings/usage). If you want to see your remaining limits during an active Codex CLI session, you can use `/status`."
- help.openai.com: **could not fetch.** Both https://help.openai.com/en/articles/20001625-understanding-chatgpt-and-codex-usage-limits and https://help.openai.com/en/articles/11369540-using-codex-with-your-chatgpt-plan returned HTTP 403 to both WebFetch and curl. Search-engine summaries said Plus has five-hour and weekly limits and Pro tiers have no five-hour limit, and pointed to Settings → Usage. I could not verify the exact wording, so I am not quoting them.

### 4. UNOFFICIAL methods (what "official only" rules out)
- **UNOFFICIAL: private ChatGPT backend endpoint.** CodexBar's docs (https://github.com/steipete/CodexBar/blob/main/docs/codex.md): "Calls `GET https://chatgpt.com/backend-api/wham/usage` (default) with `Authorization: Bearer <token>`", using the token from `~/.codex/auth.json`. Not documented by OpenAI.
- **UNOFFICIAL: session rollout files.** Codex persists `TokenCount` events, which include `rate_limits`, into the session rollout JSONL. See `persisted_event_msg` in https://github.com/openai/codex/blob/main/codex-rs/rollout/src/policy.rs: "EventMsg::TokenCount(_) … => Some(Cow::Borrowed(ev))". Reading `~/.codex/sessions/**.jsonl` is an internal file format, not a documented interface.
- **UNOFFICIAL: screen-scraping `/status`** in a PTY. CodexBar lists this as "Codex CLI PTY diagnostics (`/status`)".
- For contrast, CodexBar's *automatic* Codex source is the official route: "Launches local RPC server: `codex -s read-only -a never app-server`" and calls "`initialize`", "`account/read`", "`account/rateLimits/read`" (same CodexBar doc).

---

## (c) DeepSeek: official balance endpoint (confirmed)
Source: https://api-docs.deepseek.com/api/get-user-balance (Markdown copy: https://api-docs.deepseek.com/api/get-user-balance.md)

- Endpoint: `GET /user/balance`, server `https://api.deepseek.com`, so the full URL is `https://api.deepseek.com/user/balance`. Description: "Get user current balance".
- Auth: `Authorization: Bearer <API_KEY>`.
- Response 200 ("OK, returns user balance info."):
  - `is_available` (boolean): "Whether the user's balance is sufficient for API calls."
  - `balance_infos` (array of objects):
    - `currency` (`CNY` | `USD`): "The currency of the balance."
    - `total_balance` (string): "The total available balance, including the granted balance and the topped-up balance."
    - `granted_balance` (string): "The total not expired granted balance."
    - `topped_up_balance` (string): "The total topped-up balance."
- Example: `{"is_available": true, "balance_infos": [{"currency": "CNY", "total_balance": "110.00", "granted_balance": "10.00", "topped_up_balance": "100.00"}]}`
- Note: this is a prepaid money balance. It has no 5-hour or weekly windows.

---

## Practical implication for the desktop app (switch provider at 90%)
- **Claude Code:** launch `claude` with `--settings '{"statusLine":{"type":"command","command":"<helper>"}}'`, or set `statusLine` in user settings. The helper reads stdin and writes `rate_limits.five_hour.used_percentage` and `rate_limits.seven_day.used_percentage` (plus `resets_at`) to a file the app watches. It treats missing windows as unknown. Setting `refreshInterval` keeps the helper running while idle, but the values only change after new API responses.
- **Codex:** start a separate `codex app-server` (stdio), `initialize`, then poll `account/rateLimits/read`. Use `rateLimitsByLimitId` or `rateLimits` and pick windows by `windowDurationMins`. Alternatively, host the app-server and attach the TUI with `codex --remote unix://PATH` to receive `account/rateLimits/updated`. Keep OpenAI's note on app-server authentication for commercial products in mind.
- **DeepSeek:** poll `GET https://api.deepseek.com/user/balance`.
