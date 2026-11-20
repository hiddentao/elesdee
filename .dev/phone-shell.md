# Phone shell, pairing and remote connection

Status: design. Steps 1 to 4, step 7's terminal input, session actions, permission and question cards, AI config switch, structured message view, upload, file and image reads and diffs on both phone surfaces, and step 8's demo-server forwarding on both the computer and the phone are implemented in part. The native shell shows the message view with prompts, replies, tool rows, the context strip, the composer and the key rail, with one button to the read-only terminal. It also lists the session's files, previews images and text, shows the changes since the branch point, and uploads one file with the native picker. The desktop pairing dialog in Settings > Devices shows the QR image, the phone name and the six-digit code, and the approve, deny and cancel actions. The shell has not run on a device or an emulator, and the Android and iOS projects are not generated.

Read this with `.dev/SPEC.md` (Mobile companion app, Connection technology), `.dev/api.md` and `docs/phone.md`.

This document answers four questions:

1. Which connection stack the phone shell uses.
2. How a phone pairs, stays connected and loses access.
3. How the phone gets and runs the UI package.
4. What the phone may ask the computer to do.

## 1. Decision

Use iroh. The core service and the phone shell each run an iroh endpoint. Each endpoint has a long-lived Ed25519 key. The public key is the device identity. The core service holds the desktop endpoint, so the phone stays connected when the desktop window closes.

| Crate | Version | Used by | Job |
| --- | --- | --- | --- |
| `iroh` | 1.5.0 | `link`, core, phone shell | QUIC transport, public relays, n0 discovery |
| `iroh-tickets` | 1.0.0 | `link` | `Ticket` trait and endpoint address encoding |
| `iroh-mdns-address-lookup` | 0.6.1 | `link` | Same Wi-Fi discovery; optional |
| `qrcode` | 0.15.0, `image` feature | core | Pairing QR as a PNG |
| `blake3` | 1.8.7 | `link`, core, phone shell | Package hash and pairing code |
| `subtle` | 2.6.1 | `link` | Constant-time secret compare |
| `zeroize` | 1.9.2 | `link` | Clear pairing secrets from memory |
| `postcard` | 1.1.3 | `link` | Pairing ticket body encoding |
| `zip` | 9.8 | phone shell | Safe extraction of the UI package |
| `tower` | 0.5, `util` feature | core | In-process calls to the local API router |
| `rand` | 0.10 | core | Pairing secret |

Pin exact versions with a committed `Cargo.lock`. iroh 1.5.0 requires Rust 1.91. Raise the `rust-version` of `core` and the new crates to 1.91. The desktop Tauri shell does not depend on iroh and keeps its current MSRV.

Step 1 is implemented in `link/`. The crate pins `iroh` 1.5.0, `iroh-tickets` 1.0.0, `iroh-mdns-address-lookup` 0.6.1 (optional, behind the `mdns` feature), `blake3` 1.8.7, `subtle` 2.6.1, `zeroize` 1.9.2 and `postcard` 1.1.3. Its `Cargo.lock` pins `serde` 1.0.229 and `serde_json` 1.0.154. The crate sets `rust-version = "1.91"`. The crate holds the device identity, the pairing ticket and session, request and response framing, the method allowlist and the size and count limits. It has unit tests and a loopback test with two iroh endpoints.

Step 2 is implemented in `core/src/link/`. The core raises `rust-version` to 1.91, depends on `elesdee-link` and `iroh` 1.5.0, and pins `qrcode` 0.15.0 and `image` 0.25 for the QR PNG. The desktop Tauri shell still does not depend on iroh.

Step 3 is implemented in `link/src/client.rs` and `phone-shell/`. The client loads or creates the phone identity through a `PhoneStore` trait, so the shell can keep the secret in the platform keychain. It parses the ticket, checks its expiry hint, dials the pair ALPN, reads the code, waits for the final answer, and saves the computer record on approval. It checks the computer id in the pairing reply against the endpoint in the ticket, clears the serialized pairing message after it is sent, and then dials the app ALPN and sends allowlisted requests with the shared framing. A `PairingMachine` holds the pairing state and has unit tests without a network. The shell is a Tauri v2 crate with a `mobile_entry_point`, a plain TypeScript UI and the shared design tokens. The platform keychain uses `keyring-core` with the Apple and Android native stores. The computer list is JSON in app-private storage. The generated Android and iOS projects are not in the repository; `tauri android init` and `tauri ios init` create them on a machine with the SDKs.

The iroh API changes often. Keep the surface small: `Endpoint::builder(presets::N0)`, `.alpns`, `.address_lookup`, `.bind`, `Endpoint::connect`, `Connection::{open_bi, accept_bi, remote_id, close}`, `protocol::Router`, and `iroh_tickets::Ticket`. Run the link tests on every upgrade.

API notes from the pinned crates:

- `iroh::PublicKey` `Display` produces hex. The computer id uses `PublicKey::to_z32()`, which is the z-base-32 string.
- `iroh::EndpointAddr` carries the endpoint id and a set of transport addresses, and it implements serde. The ticket encodes it with postcard.
- `iroh_tickets::Ticket` requires `KIND`, `encode_bytes` and `decode_bytes`. The default string form is the kind plus unpadded base32.
- `SecretKey::generate()` takes no argument. `SecretKey::from_bytes` takes a `&[u8; 32]`.
- `presets::Minimal` gives two loopback endpoints without a relay, which the link test uses. `presets::N0` remains the production preset.

Why not WebRTC:

- WebRTC needs a rendezvous server, STUN and a TURN relay. iroh's public relays cover all three for version 1.
- WebRTC needs an offer and answer exchange on each connect. The iroh ticket holds a stable endpoint address.
- The Tauri phone shell is Rust. iroh is a Rust library. WebRTC would need a second runtime or a foreign-function bridge.
- WebRTC remains the fallback if the test build finds an iroh blocker.

## 2. Parts and new paths

```
Desktop computer
  Tauri window ── local HTTP ── core service
                                  ├─ local axum API
                                  ├─ link server (iroh endpoint)
                                  ├─ pairing and device list
                                  └─ phone UI package

Phone
  Tauri shell
    ├─ link client (iroh endpoint)
    ├─ stored computers and UI packages
    ├─ custom protocol that serves shell screens and the package
    ├─ QR scanner, notifications, file picker
    └─ demo-server forwarding
  WebView
    ├─ shell screens: pair, computer list, update needed
    └─ UI package from the computer: the React phone layout
```

| Path | Contents |
| --- | --- |
| `link/` | Shared Rust crate: versions, ticket, framing, methods, client and server helpers. |
| `core/src/link/` | Endpoint start, pairing, device store, request dispatch, UI package service, forwarding. |
| `phone-shell/` | Tauri mobile app. Its Rust crate is `phone-shell/src-tauri`. |
| `phone-shell/ui/` | The shell's own screens. Tauri serves them from `frontendDist`. |
| `app/src/api/transport.ts` | Transport adapter: HTTP for the browser, Tauri bridge for the phone. |
| `app/vite.phone.config.ts` | Builds the phone UI package from the same React source. |
| `scripts/package-phone-ui.mjs` | Zips the phone UI, hashes it and writes the manifest. |

Step 2 files: `core/src/link/mod.rs` holds the endpoint and service, `identity.rs` the private key file, `hub.rs` the pairing state, device access and live connections, `pair.rs` the pair ALPN, `app.rs` the app ALPN and dispatch, `wire.rs` the pair messages, and `qr.rs` the PNG. The integration tests are in `core/tests/link.rs`.

Data flow:

1. Pairing: the phone scans the QR, dials the pair ALPN, proves the one-time secret, and both sides store the other key.
2. Connection: the phone dials the app ALPN, sends `hello`, receives `welcome`, then opens event and output streams.
3. UI delivery: the welcome carries the package manifest. The phone downloads the package only when its build id differs from the stored build.
4. Demo servers: the phone shell listens on a loopback port and forwards bytes over the forward ALPN.

## 3. Connection protocol

### 3.1 Endpoint identity

- The desktop key lives in `<data dir>/link.key`: 32 bytes, mode 0600, written atomically. It is not in `state.json` and is never logged.
- If the key is missing, the core creates a new one. Every paired phone must pair again.
- The phone key lives in the platform keychain through `phone-shell/src-tauri/src/store.rs`. The shell asks `keyring-core` for the identity entry and never writes the secret to a plain file. Android backup is disabled for the app, so a restored phone does not clone the identity.
- The computer id is the base32 endpoint id. The interface shows the first 8 characters. The phone stores the full endpoint address from the pairing ticket.
- A lost key means a new identity. There is no export or import, as the spec says.

### 3.2 ALPNs

| ALPN | Direction | Job |
| --- | --- | --- |
| `elesdee/pair/1` | Phone to computer | One-time pairing |
| `elesdee/app/1` | Phone to computer | Requests, events and session output |
| `elesdee/forward/1` | Phone to computer | Demo-server TCP forwarding |

The version is in the ALPN. An incompatible protocol change gets a new ALPN, so an old phone gets a clear update message instead of a parse failure.

### 3.3 Endpoint construction

```rust
let endpoint = Endpoint::builder(presets::N0)
    .secret_key(key)
    .alpns(vec![PAIR_ALPN.to_vec(), APP_ALPN.to_vec(), FORWARD_ALPN.to_vec()])
    .address_lookup(mdns) // optional
    .bind()
    .await?;

let router = Router::builder(endpoint)
    .accept(PAIR_ALPN, PairHandler { .. })
    .accept(APP_ALPN, AppHandler { .. })
    .accept(FORWARD_ALPN, ForwardHandler { .. })
    .spawn();
```

- `presets::N0` gives the public relays and n0 discovery. Version 1 uses these relays; the user cannot change them. A later release can replace the preset with a pinned relay map inside `link`; no phone setting changes.
- `iroh-mdns-address-lookup` adds same-Wi-Fi discovery. It is an optimization. The relay path must work without it.
- Tests build endpoints with `presets::Minimal` and direct loopback addresses, so CI needs no relay.
- Start the endpoint after the state loads. If the UDP socket cannot bind, log the error and keep the local API running. Pairing returns 503 until the endpoint is up.
- On shutdown, `router.shutdown().await` and `endpoint.close().await`.

Step 2 implements this in `core/src/link/mod.rs`. The identity load happens before the bind. The bind uses the production preset and registers the three ALPNs. Step 8 adds the forward handler, so a forward connection is authenticated against the paired device list and carries demo-server tunnels. A bind failure leaves the hub unavailable, and the local API still starts.

### 3.4 Framing

One bidirectional stream carries one request and its response.

```
4-byte big-endian JSON length | JSON header | optional binary body
```

- The JSON header has a maximum of 1 MiB. A reader checks the 4-byte length against the cap before it allocates a buffer, and refuses an oversized frame without reading its body.
- Request: `{"v":1,"method":"sessions.input","params":{"sessionId":"...","data":"..."}}`
- Success: `{"v":1,"ok":true,"result":{...}}`
- Failure: `{"v":1,"ok":false,"error":{"code":"conflict","message":"..."}}`
- If the header has `bytes`, exactly that many binary bytes follow. Uploads are capped at 25 MiB. The UI package is capped at 64 MiB.
- A streaming response starts with `{"v":1,"ok":true,"stream":true}`. Each later message is a framed JSON event. The stream stays open until the request ends or the connection closes.

Step 2 uses this framing for the app ALPN. The pairing handshake has its own small frame: a four-byte big-endian JSON length followed by the JSON, capped at 16 KiB. The phone sends `{ v, secret, name, platform, bridgeVersion, protocolMin, protocolMax }`, where `secret` is base32 without padding. The computer first answers `{ v, ok: true, status: "confirm", code, computerId, computerName, protocolVersion, minBridgeVersion }`, then one final `{ v, ok: true, status: "paired" }` or `{ v, ok: false, status, message }`.

On the app ALPN, step 2 reads the four-byte header length and checks it against the 1 MiB cap before it allocates the header. It reads the header, checks the declared body length against the 25 MiB cap, and only then reads the body. A frame over either cap gets `too_large` without a large allocation.

Step 3's client applies the same rule in the other direction. `read_json` checks the four-byte length against the 16 KiB pairing cap before it allocates. `read_response` checks the four-byte header length against the 1 MiB cap, reads and parses the header, checks the declared body against the UI package cap, and only then allocates the body. A declared length over a cap returns `TooLarge` before any body allocation.

Limits:

| Limit | Value |
| --- | --- |
| JSON header | 1 MiB |
| Upload body | 25 MiB |
| UI package | 64 MiB |
| Streams per phone connection | 16 |
| Streams for the link, all phones | 64 |
| Paired phones | 16 |
| Connected phones | 8 |
| `hello` timeout | 5 seconds |
| Unary request timeout | 30 seconds |
| QUIC keep-alive | 5 seconds |
| QUIC idle timeout | 30 seconds |

Error codes map to the existing local API status codes: `bad_request` (400), `forbidden` (403), `not_found` (404), `conflict` (409), `too_large` (413), `unsupported` (415), `unprocessable` (422), `rate_limited` (429), `internal` (500). Two link codes are new: `not_paired` and `revoked`.

Dispatch reuses the core code. It does not copy handlers:

- A unary method becomes an in-process request to the local router with `tower::ServiceExt::oneshot`. Set `Host: 127.0.0.1` and send no `Origin`, so the existing request guard accepts it. The router is cloned for each call. This keeps the API contract in one place.
- Events come from `state.subscribe_events()` and `state.subscribe_aux()`. One link task fans them out to the connected phones. If a phone's event queue is full, the link closes that phone's event stream. The phone reconnects and gets a fresh bootstrap snapshot.
- Session output uses `state.session(id)`, `subscribe_output()`, `output_snapshot(after)` and `subscribe_close()`, and holds the `try_acquire_output_sse()` permit. The link applies its own stream cap on top of the existing one. A slow phone gets a `gap` event, not unbounded buffering.

### 3.5 Connection life cycle

1. The phone dials the stored endpoint address on `elesdee/app/1`.
2. The desktop accept handler reads `connection.remote_id()` and checks the paired device list. An unknown key gets `close(2, "not paired")`. A removed key gets `close(3, "revoked")`.
3. The phone opens a control stream and sends `hello`:

```json
{
  "v": 1,
  "bridgeVersion": 1,
  "protocolVersion": 1,
  "platform": "android",
  "appVersion": "0.1.0",
  "lastUiBuildId": "blake3:..."
}
```

4. The desktop replies `welcome`:

```json
{
  "v": 1,
  "computerId": "base32...",
  "computerName": "Studio computer",
  "appVersion": "0.1.0",
  "protocolVersion": 1,
  "minBridgeVersion": 1,
  "ui": {
    "schema": 1,
    "buildId": "blake3:...",
    "appVersion": "0.1.0",
    "protocolVersion": 1,
    "minBridgeVersion": 1,
    "bytes": 123456,
    "files": 14
  }
}
```

5. The phone opens one `events.subscribe` stream and one `output.subscribe` stream for each session it shows.
6. QUIC keep-alive is 5 seconds. The 30-second idle timeout applies only when keep-alives stop. The phone detects the loss with `Connection::closed()`.

The desktop must authorize the connection key once, at accept. It does not re-check the key on each request. A revoked device has its live connections closed by the device store.

Step 2 implements the accept check and answers `bootstrap` on a request stream. Step 3's client dials the app ALPN on demand, sends `bootstrap` on one request stream, and closes the connection when the shell forgets the computer. Step 4 implements the control stream handshake, the unary methods with local routes, and the event and output subscriptions. A connection that has not sent `hello` gets no request handling; the handler waits five seconds for the control stream and closes the connection on an incompatible protocol version.

### 3.6 Reconnect and resume

| Event | Phone behavior |
| --- | --- |
| App opens or returns to the foreground | Connect at once. |
| Connection drops | Retry after 0.5 s, 1 s, 2 s, 4 s, 8 s, 15 s, then 30 s, with ±20% jitter. Reset after 30 s connected. |
| Network changes | Cancel the wait and connect at once. |
| Desktop restarts | The endpoint key is the same. The phone reconnects. Sessions are restored as closed by the existing persistence rule. |
| Desktop window closes | The core keeps the endpoint. The phone stays connected. |
| Core stops | The phone retries and shows "Cannot reach this computer." |
| Device removed | The desktop closes with code 3. The phone stops retrying and shows "This computer removed this phone." |

On reconnect:

1. Send `hello` and read `welcome`.
2. Call `bootstrap` for a full snapshot of projects, accounts, sessions, questions and session requests.
3. Resubscribe to events.
4. Resubscribe to each open session's output from the last sequence number seen. The output snapshot fills the gap. A `gap` field tells the phone that old text was dropped.
5. Do not replay notifications. The bootstrap snapshot shows what still needs the user.

The phone stores the full endpoint address from the ticket. Discovery by endpoint id finds new addresses when the network changes. The phone does not need a new QR code.

Step 4 implements this behavior in `link/src/reconnect.rs`, `link/src/client.rs` and the shell connection manager. A dropped connection retries after the listed delays with ±20% jitter and resets the sequence after 30 seconds connected. The manager reopens the event stream and resumes each output stream from the last sequence seen. A revoked or unknown device stops the retries. Network change and foreground controls come with step 6.

## 4. Pairing

### 4.1 Ticket

Use a custom ticket that implements `iroh_tickets::Ticket`:

```rust
struct PairTicket {
    v: u8,                  // 1
    addr: EndpointAddr,     // endpoint id, relay URL, direct addresses
    secret: [u8; 32],       // one-time pairing secret
    desktop_name: String,   // capped at 64 characters
    expires_at: u64,        // Unix seconds; a hint for the phone
}
```

- `KIND` is `elesdee-pair`. The body is postcard encoded. The string form is the kind plus base32, as the `Ticket` trait defines.
- The core generates the QR as a PNG in memory with `qrcode` and serves it at `GET /api/pairing/qr`. It is never written to disk and never logged.
- The core takes 32 bytes from the OS CSPRNG for the secret and keeps it in `Zeroizing` memory.
- The secret crosses the connection inside iroh's end-to-end encryption. The relay sees only ciphertext.
- One pairing session per computer. A new request cancels the old one. The pair handler closes every connection when no session is active.

Step 2 implements the ticket and the QR. The QR is rendered in memory in `core/src/link/qr.rs`; the ticket is never written to disk or a log.

### 4.2 Lifetime

| Item | Value |
| --- | --- |
| Pairing ticket life | 120 seconds |
| Uses | One |
| Bad secret attempts before cancel | 5 |
| Confirmation time | 60 seconds |
| Long-lived token | None |

There is no session token after pairing. The phone's endpoint key is the credential. The desktop authorizes that key against the paired device list. Revocation removes the key.

Step 2 implements these values. The confirmation code is stored only while the state is `confirm`. A used or cancelled session keeps a marker, so a replay gets `used` or `cancelled` instead of a silent close.

### 4.3 Flow

1. The user opens Settings, Devices, then "Pair a phone".
2. The core waits up to 5 seconds for `endpoint.online()`, creates `PairTicket`, and returns `{ expiresAt, relayReady }`. The dialog shows `GET /api/pairing/qr` with a cache-busting query. If the relay is not ready, the UI says "Same Wi-Fi only".
3. The phone scans the QR and parses the ticket. It rejects an expired ticket as a hint, then dials `elesdee/pair/1`.
4. The phone sends:

```json
{
  "v": 1,
  "secret": "base32...",
  "name": "Phone · 7F3A",
  "platform": "android",
  "bridgeVersion": 1,
  "protocolMin": 1,
  "protocolMax": 1
}
```

5. The core checks, in this order: an active pairing session exists, the ticket is not expired, the secret matches in constant time, the protocol ranges overlap, the name is 1 to 64 characters after cleanup, and the device cap is not reached.
6. The core computes a 6-digit code: `blake3::keyed_hash(secret, phone_id || desktop_id)`, take 4 bytes, reduce modulo 1,000,000, and pad to 6 digits. It replies:

```json
{
  "v": 1,
  "code": "428913",
  "computerId": "base32...",
  "computerName": "Studio computer",
  "protocolVersion": 1,
  "minBridgeVersion": 1,
  "ui": { ... }
}
```

7. The desktop UI polls `GET /api/pairing` each second while the dialog is open. The state moves to `confirm`. The dialog shows the phone name, the code and two actions: Approve and Cancel.
8. The phone shows the same code and the text "Check that this code matches the computer."
9. Approve stores the device, destroys the secret, and sends `paired`. Cancel stores nothing, destroys the secret, and sends `denied`.

The code confirms that the phone on the channel holds the secret. It does not prove that the phone is trusted. The short lifetime and the visible device name are the controls against a photographed code. The user must read the name before approving.

### 4.4 Device storage

`state.json` gains a `pairedDevices` array:

```json
{ "endpointId": "base32...", "name": "Phone · 7F3A", "platform": "android", "pairedAt": "2026-11-20T09:00:00Z", "lastSeen": "2026-11-20T10:00:00Z" }
```

- The file holds no secret key and no pairing secret.
- The core writes state on pair, remove and graceful shutdown. `lastSeen` updates in memory at most once a minute and is best effort.
- The local API is the only way to read the device list. A phone cannot list other phones.
- A 17th pairing returns `conflict`.

Step 2 implements this. The core test checks that `state.json` has `pairedDevices` and neither the pairing secret nor a device key.

### 4.5 Revocation

- Settings, Devices, Remove deletes the record, closes live connections with code 3, and saves state.
- The phone sees code 3, marks the computer as removed, stops retrying, and offers "Forget this computer".
- The phone can forget a computer locally. The computer still lists it until the user removes it there.
- If a phone key is copied, removal on the computer is the only control. There is no per-connection approval. State this in the user documentation.

### 4.6 Local API

These routes are local only. They are never reachable through the link.

| Method | Path | Body | Result |
| --- | --- | --- | --- |
| POST | `/api/pairing` | `{}` | `{ expiresAt, relayReady, ticket? }` |
| GET | `/api/pairing/qr` | | `image/png` for the active ticket; 404 when idle |
| GET | `/api/pairing` | | `{ state, expiresAt?, phoneName?, code? }` |
| POST | `/api/pairing/approve` | `{}` | `PairedDeviceRecord` |
| POST | `/api/pairing/deny` | `{}` | `204` |
| DELETE | `/api/pairing` | | `204` |
| GET | `/api/devices` | | `PairedDevice[]` |
| DELETE | `/api/devices/{id}` | | `204` |

- `state` is `idle`, `waiting`, `confirm`, `paired`, `denied`, `expired` or `cancelled`.
- A debug build also returns `ticket` from `POST /api/pairing` when `ELESDEE_DEV=1`. This is the emulator paste fallback. Release builds omit it.
- Approve returns 409 when no phone is waiting for confirmation. Deny and cancel return 204 in every state.
- Remove returns 404 for an unknown device.

These routes are implemented in `core/src/api.rs`. `POST /api/pairing` returns 503 until the endpoint is up. The QR route returns 404 outside `waiting`.

## 5. Phone UI delivery

**Shell project**

- `phone-shell/src-tauri` is a Tauri app with `[lib] crate-type = ["staticlib", "cdylib", "rlib"]`.
- `lib.rs` exposes `#[cfg_attr(mobile, tauri::mobile_entry_point)] pub fn run()`. `main.rs` calls `run()` for desktop development.
- `tauri android init` generates the Android project. The application id is `com.elesdee.phone`.
- `frontendDist` is `phone-shell/ui/dist`. Tauri serves the shell screens from its own asset origin: `http://tauri.localhost` on Android, `tauri://localhost` on macOS.
- Android manifest permissions: `INTERNET`, `ACCESS_NETWORK_STATE`, `CAMERA`, `POST_NOTIFICATIONS`, `CHANGE_WIFI_MULTICAST_STATE`. Set `android:allowBackup="false"`.
- Set `minSdkVersion` to 26. Allow cleartext only for `localhost`, `elesdee.localhost` and `tauri.localhost`, never for internet hosts.
- Capabilities grant only the barcode scanner, notification and dialog commands to the main window.
- Register the custom protocol before the window is created.

### 5.1 Build

- `app/vite.phone.config.ts` builds only `phone.html` with `base: './'` into `app/dist-phone`. It uses the same React source as the desktop layout.
- `npm run build:phone` runs the build.
- `scripts/package-phone-ui.mjs` writes `phone-ui.zip` with sorted entries and fixed timestamps, computes the blake3 hash, and writes `phone-ui.json`.
- The packaging script renames `phone.html` to `index.html`, so the shell loads `/ui/<buildId>/`.
- Use `fflate` 0.8 in the packaging script so the build does not depend on a platform zip tool.

Manifest:

```json
{
  "schema": 1,
  "buildId": "blake3:...",
  "appVersion": "0.1.0",
  "protocolVersion": 1,
  "minBridgeVersion": 1,
  "bytes": 123456,
  "files": 14
}
```

### 5.2 Ship and serve on the computer

Step 3 does not build or serve the phone UI package. The shell serves its own screens from `frontendDist`. The package delivery below is step 5.

- The Tauri desktop bundle includes `app/dist-phone-package` as a resource.
- The desktop shell starts the core with `--phone-ui <resource path>`. In development the core falls back to `ELESDEE_PHONE_UI`, then `app/dist-phone-package`.
- The core checks the manifest and the zip hash at startup. If they are missing or invalid, pairing still works, but `welcome` has `ui: null`. The phone shows "This computer has no phone interface. Update elesdee on this computer."
- `ui.download` returns the manifest as the response header and the zip as the binary body. Only a paired phone can call it.

### 5.3 Download and store on the phone

1. Compare the `buildId` in `welcome` with the stored build for that computer.
2. If they match, use the stored build.
3. If they differ, call `ui.download`. Write the body to a temporary file while hashing it.
4. Check the hash, the byte count and the entry count.
5. Extract into a temporary directory, then rename it to `<data>/computers/<computerId>/ui/<buildId>/`.
6. Keep the current build and one previous build. Delete older builds.

Reject a package when it has:

- a path with `..`, an absolute path, a symlink, or a name over 512 bytes;
- duplicate names, including names that differ only by case;
- more than 4096 entries;
- more than 256 MiB after extraction;
- a file over 64 MiB.

If the new build fails to load, the shell shows "Use the previous version". The previous build stays until a build loads successfully.

### 5.4 Version checks

Three numbers govern compatibility:

| Number | Owner | Meaning |
| --- | --- | --- |
| `PROTOCOL_VERSION` | `link` crate | Message format and methods |
| `BRIDGE_VERSION` | phone shell | Native commands and package requirements |
| `ui.minBridgeVersion` | UI package manifest | Oldest bridge the package needs |

On connect:

- If the shell's protocol is outside the computer's supported range, show "Update elesdee on this computer."
- If the computer's `minBridgeVersion` is greater than `BRIDGE_VERSION`, show "Update the phone app."
- If `ui.minBridgeVersion` is greater than `BRIDGE_VERSION`, show the same.
- If the package's protocol is outside the shell's range, show the same.

The shell translates its screens, including these messages, into all 12 languages. They do not depend on the package. The package uses the phone's own language setting.

Keep the package's `@tauri-apps/api` version within the same minor as the shell's Tauri version. The bridge version is the contract. The IPC protocol also has to match.

### 5.5 Serving the package

Tauri serves the shell screens from `frontendDist`. Use `tauri::Builder::register_uri_scheme_protocol("elesdee", handler)` for the UI package.

- The package origin is `http://elesdee.localhost` on Android and `elesdee://localhost` on macOS and iOS.
- The shell origin is `http://tauri.localhost` on Android and `tauri://localhost` on macOS.
- Require Tauri 2.10.3 or later for the local-URL origin fix (CVE-2026-42184). Pin the current 2.12.7.
- When a package is ready, the shell navigates the main webview to `/ui/<buildId>/`. Back returns to the shell origin.

Routes on the `elesdee` protocol:

| Path | Response |
| --- | --- |
| `/ui/<buildId>/*` | The active computer's stored build |
| Anything else | 404 |

- Serve only the active computer and the current build. Resolve the path, reject `..` before disk access, and check the canonical path is under the build root.
- Set the correct MIME type. Hashed assets can be cached for a year. `index.html` is `no-store`.
- The package cannot load scripts, styles, fonts or images from the internet. Session images arrive through the bridge as blobs or data URLs.
- The shell blocks navigation away from the shell origin and the package origin, and denies new windows.
- Content security policy: `default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data: blob:; connect-src ipc: http://ipc.localhost; object-src 'none'; base-uri 'none'; frame-ancestors 'none'`.

### 5.6 Bridge

The shell exposes a small command set to the package:

| Command | Job |
| --- | --- |
| `phone_bridge_info` | Return the bridge version and the active computer. |
| `phone_rpc` | Send a unary request to the computer. |
| `phone_subscribe` | Open an event or output subscription. |
| `phone_unsubscribe` | Close a subscription. |
| `phone_pick_upload` | Open the file picker, check the size and stream one file to a session. |

The package cannot use a shell, filesystem, HTTP or process plugin. No command accepts a path and reads it. The package gets a result, not a file handle.

The shell posts notifications itself, from the event stream. The package cannot ask for an arbitrary notification. The shell checks every method against the same allowlist before it sends a request to the computer.

Step 3 implements the shell side of the bridge as these commands in `phone-shell/src-tauri/src/commands.rs`:

| Command | Job |
| --- | --- |
| `phone_info` | Return the phone id, platform, bridge version and protocol version. |
| `phone_computers` | List the stored computers and whether each has an open connection. |
| `phone_pair_start` | Parse the ticket, dial the pair ALPN and return the six-digit code. |
| `phone_pair_finish` | Wait for the computer's answer and save the record on approval. |
| `phone_pair_cancel` | Close the pairing connection. |
| `phone_forget` | Close any connection and remove the local record. |
| `phone_connect` | Open the app connection to one computer. |
| `phone_disconnect` | Close the app connection. |
| `phone_request` | Parse the method name against `Method::parse`, then send the request. |
| `phone_pick_upload` | Open the native file picker, check the size and send one file to a session. |
| `phone_read_image` | Send `files.image` and return the image type and base64 bytes. |

`phone_request` is the only path that sends a method. It rejects a name outside the allowlist before it touches a connection, so the screens cannot reach a method that the link crate does not list. No command holds the shell state lock across a network wait: pairing, connect and request take what they need, drop the lock, then write the result back. A pairing call has its own gate, so two pairings cannot overlap while other commands still run. One timeout covers the whole request, including the send, so a computer that stops reading cannot hold a caller forever. The package bridge (`phone_rpc`, subscriptions and upload) comes with steps 4 and 5.

The shell injects `window.__ELESDEE_PHONE__` with `{ computerId, computerName, bridgeVersion, protocolVersion }` before page scripts run. The package uses it to select the phone transport. In a normal browser the marker is absent, so the same bundle uses the local HTTP API. This keeps the Playwright tests working.

### 5.7 UI transport refactor

Add `app/src/api/transport.ts`:

```ts
interface Transport {
  request<T>(method: string, params?: unknown): Promise<T>;
  subscribe(
    channel: 'events' | 'aux' | 'output',
    params: unknown,
    handlers: { onEvent: (value: unknown) => void; onError?: () => void },
  ): () => void;
}
```

- The browser transport uses `fetch` and `EventSource`, as today.
- The phone transport uses `phone_rpc`, `phone_subscribe` and `phone_unsubscribe`.
- `client.ts` and `events.ts` call the transport. Components do not change.
- Method names map to existing routes where possible.

## 6. Security boundaries

### 6.1 Trust model

- iroh encrypts and authenticates every connection. The desktop checks the phone key at accept against the paired list.
- The desktop trusts the phone less than a local process. The phone gets an allowlist, not the full local API.
- The local API still trusts the local user. The link does not change that.
- A compromised computer can send arbitrary JavaScript to the phone as a UI package. The bridge is the containment boundary. The package gets RPC and the listed native features, not filesystem or process access.
- A compromised phone has the user's session access until the user removes the device.
- There is no app lock. The phone's screen lock is the control, as the spec says.

Step 2 implements the key check at accept, the method allowlist and the frame limits. The phone cannot read the device list, write a file or send session input.

### 6.2 What the phone may call

| Method | Maps to | Notes |
| --- | --- | --- |
| `bootstrap` | `GET /api/bootstrap` | Snapshot for the list and the reconnect. |
| `health` | `GET /api/health` | Agent versions, read only. |
| `projects.list` | `GET /api/projects` | For the new-session form. |
| `branches.list` | `GET /api/branches` | For the new-session form. |
| `providers.list` | `GET /api/providers` | For the new-session form. |
| `accounts.list` | `GET /api/accounts` | Metadata and usage. Never keys. |
| `sessions.create` | `POST /api/sessions` | The desktop ignores a folder from a paired device and uses the project folder. |
| `sessions.close` | `POST /api/sessions/{id}/close` | Same confirmation as the desktop. |
| `sessions.input` | `POST /api/sessions/{id}/input` | Text and key bytes. No resize. |
| `sessions.permission` | `POST /api/sessions/{id}/permission` | Allow or deny one request. |
| `sessions.account` | `POST /api/sessions/{id}/account` | The older provider switch. |
| `sessions.config` | `POST /api/sessions/{id}/config` | The one-button AI config switch. |
| `sessions.resume` | `POST /api/sessions/{id}/resume` | Resume a paused session. |
| `sessions.upload` | New local route | Writes one file to the session upload folder. |
| `questions.list` | `GET /api/questions` | Pending questions. |
| `questions.answer` | `POST /api/questions/{id}/answer` | One answer. |
| `sessionRequests.approve` | `POST /api/session-requests/{id}/approve` | Approves an agent-started session. |
| `sessionRequests.deny` | `POST /api/session-requests/{id}/deny` | Denies it. |
| `files.list`, `files.read`, `files.image` | New local routes | Confined to the session folder and worktree. |
| `diff.read` | New local route | The session's changes since it branched. |
| `events.subscribe` | `GET /api/events` | Server to phone. |
| `output.subscribe` | `GET /api/sessions/{id}/output` | Server to phone, replay by sequence. |
| `history.subscribe` | `GET /api/sessions/{id}/history` | Server to phone, structured items, replay by sequence. |
| `ui.manifest`, `ui.download` | New link handlers | UI package only. |
| `demoServers.list` | `GET /api/demo-servers` | Active registered ports only. |

The phone cannot call:

- `POST /api/projects` or `POST /api/accounts`
- `POST /api/accounts/{id}/usage`
- `DELETE /api/sessions/{id}`
- `PATCH /api/sessions/{id}` (rename)
- `POST /api/sessions/{id}/reopen`
- `POST /api/sessions/{id}/resize`
- `POST /api/questions/{id}/withdraw`
- Any `/internal/*` route

The link server rejects an unknown method with `forbidden`. A test compares the allowlist table with the local router, so a new route does not become reachable by accident.

Step 4 dispatches every allowlisted method that has a local route: `bootstrap`, `health`, the project, branch, provider and account lists, session create, close, input, permission, account, config and resume, question list and answer, session request approve and deny, the file list and text read, the diff read, `demoServers.list`, and the event, output and history subscriptions. `sessions.create` drops a phone-supplied folder. `sessions.upload` forwards its binary body and file name to the upload route, and `files.image` forwards its path to the image route and returns the bytes. The UI package methods return `unsupported` until their step. A method outside the allowlist is rejected while the request frame decodes, so it returns `forbidden`.

A phone never chooses a job folder. When remote session creation is added, it starts in the project folder, and the link drops any `folder` parameter from a `sessions.create` request before a handler sees it.

Account keys never cross the link. The existing account response omits them.

### 6.3 Uploads, files and images

- `sessions.upload` writes to `<data dir>/uploads/<session id>/<name>`, outside the repository.
- The route is `POST /api/sessions/{id}/upload`. It accepts `application/octet-stream` with an `X-El-Esdee-Filename` header that carries the percent-encoded UTF-8 name. The request guard has an explicit exception for this path; every other POST and PATCH keeps the JSON rule. The body limit is 25 MiB.
- The core reduces the name to a single path component and removes control, bidi and zero-width characters. It rejects `.` and `..`, and an empty result becomes `file`. A name over 120 characters is shortened and keeps its extension.
- The core does not overwrite an existing file. A repeated name gets a numeric suffix. A file is created with `create_new`, so a lost race never replaces a file.
- Mode is 0600 and the folder is 0700. The size cap is 25 MiB per file.
- The core writes the file, then sends one line to the session input with the absolute path. The file name is cleaned first, so the line cannot carry terminal control sequences. A closed or stopped session returns 409 and stores nothing.
- The phone cannot read arbitrary files. `files.read` and `files.image` resolve a relative path under the session folder: the worktree for an agent, the chosen folder for a terminal. Symlinks are not followed, and a hard link, a special file and a `.git` path are refused. A session created from a paired device always uses its project folder, so the readable root is chosen by the desktop, never by the phone.
- `files.image` returns PNG, JPEG, GIF, WebP, AVIF or BMP detected from the file signature, never from the name. The cap is 10 MiB. A text file or any other type returns `unsupported` (415), a larger file returns `too_large` (413), and the phone shows a clear message instead of a broken image.
- The phone never sends a path for an upload. It sends one file name, and the desktop chooses the folder.

### 6.4 Phone files, images and changes

Both phone surfaces put **Files** and **Changes** beside the session actions.

- **Files** lists the session folder one folder at a time. A folder row opens that folder and an Up action returns. An image row opens a preview through `files.image`; the native shell receives the bytes as base64 over the bridge and shows a data URL, and the browser layout uses an object URL. A text row opens the text from `files.read`. An unsupported image, a binary file and a file over a cap each show a clear message.
- **Upload** opens the native file picker in the shell and a hidden file input in the browser layout. The picker checks the size before it reads the file. A completed upload shows the stored name; a cancelled picker shows nothing.
- **Changes** lists every file changed since the branch point, with the added and removed line counts. A tap opens that file's unified diff, with added and removed lines marked. A binary file says it has no text diff. A folder outside Git says so, and the truncated flag and a Git error are shown.
- The shell bridge has `phone_pick_upload`, which picks and reads one file and sends `sessions.upload`, and `phone_read_image`, which sends `files.image` and returns `{ contentType, data }` with base64 data. The screens never pass a path to the bridge for an upload.

### 6.5 Demo-server forwarding

- The phone sends a port from the registered demo-server list. The desktop checks the active list; any other port is refused. The forward handler checks the paired device list like the app handler.
- The handshake is two big-endian port bytes from the phone, then one status byte from the computer: `0` open, `1` refused, `2` approved but nothing listens. The bytes are defined in `link/src/forward.rs` and the statuses are shared.
- The phone shell asks the computer for `demoServers.list` before it binds, and it refuses a port that no active session registered. It then binds `127.0.0.1` only. It uses the same port when free, otherwise a free port, and shows the address on the session screen.
- A row also shows the server's tailnet address when the computer recorded one. That address is for another device on the tailnet without the app; the phone still opens its own loopback address. The row does not open the tailnet address itself.
- The computer side connects to `127.0.0.1:<port>` only. The port request carries no address, so the phone cannot ask for another host.
- At most 8 tunnels per phone, one per port. A duplicate port or a ninth tunnel is refused. Closing a tunnel frees its slot.
- The session screen starts a listener for each active server and shows its address. The listener stays while the tunnel is healthy, even after another screen opens, until its server stops, its session closes, the phone disconnects or the app exits. The session's refresh and update events start a tunnel for a new server and stop the tunnel of a server that leaves the active list.
- The tunnel closes when the session stops the server, closes or pauses, when the phone connection drops, when the pairing is revoked, or when the app exits. The desktop closes a revoked phone's forward connection; the phone listener then stops and frees its port.
- Android may suspend the app after the user switches to the browser. This build does not start a foreground service and does not stop forwarding on background by itself: the platform decides whether the process keeps running. A foreground service is added only if a device test shows that Android suspends the tunnels during a normal review. iOS applies its own background limits. Neither behavior is tested here.
- The computer side is covered by a core loopback test with a real local HTTP server. The link crate has a loopback test for the phone-side forward connection and the status byte. The phone shell has unit tests for port choice, refusal of an unregistered port and a socket bridged over a fake forward ALPN server. No device or emulator has run the listener.

**Implementation files**

| Path | Contents |
| --- | --- |
| `link/src/forward.rs` | Port encoding, port range, status bytes, the local addresses, and the phone-side `ForwardConnection` and `ForwardTunnel`. |
| `link/src/limits.rs` | `MAX_FORWARD_TUNNELS`. |
| `link/src/client.rs` | The forward ALPN on the phone endpoint and `LinkClient::forward`. |
| `core/src/link/forward.rs` | Forward ALPN handler, registry check, loopback connect and teardown. |
| `core/src/link/hub.rs` | One permit per port and tunnel cancellation. |
| `core/src/api.rs` | `GET /api/demo-servers` and the stop route. |
| `core/src/demo.rs` | Demo-server records and validation. |
| `core/src/tailscale.rs` | Tailscale client discovery, the cached status read, and the tailnet address. |
| `core/src/mcp.rs` | The `register_demo_server` tool. |
| `phone-shell/src-tauri/src/forward.rs` | Loopback listener, same-or-free port choice, the 8-tunnel registry, and the socket-to-stream bridge. |
| `phone-shell/src-tauri/src/commands.rs` | `phone_forward_start` and `phone_forward_stop`, the registered-port check, and teardown on disconnect, forget and exit. |
| `phone-shell/ui/src/demo.ts` | Session server selection and the listener state of each row. |
| `phone-shell/ui/src/app.ts` | The demo-server list on the session screen, with the local and tailnet addresses, state and Open action. |

### 6.5 Logging and storage

Never log:

- endpoint secret keys, pairing secrets, or the pairing ticket;
- account keys;
- session output bodies or uploaded file contents.

Log:

- pairing start, approve, deny, expiry and cancel;
- device add and remove;
- connection open and close with the endpoint id prefix;
- protocol errors, without payloads.

Pairing state lives in memory only. The device list in `state.json` has no secrets. The phone stores computer addresses and UI packages in app-private storage, excluded from Android backup.

## 7. Work breakdown

Each step ends with an end-to-end test.

| Step | Scope | End-to-end test |
| --- | --- | --- |
| 1. Link crate | Versions, `PairTicket`, framing, method enum, allowlist, limits, error codes. Files: `link/`. | Unit tests: ticket round trip, tampered ticket rejected, frame over the limit rejected, unknown method rejected, golden strings. |
| 2. Core endpoint and pairing | **Implemented.** Persistent key, endpoint start, pairing state machine, QR PNG, device store, local pairing and device routes. Files: `core/src/link/`, `core/src/state.rs`, `core/src/api.rs`. | **Implemented.** Rust integration with two real loopback endpoints: pair once and read the session snapshot, reject a second use of the ticket, reject an expired ticket, reject a removed device. Local API: QR returns, polling reaches `confirm`, approve stores the device, state reloads with no secret. Unit tests cover five wrong secrets, cancel, deny and the 16-device limit. |
| 3. Phone shell and pairing screen | **Implemented in part.** Tauri v2 skeleton, `lib.rs` entry point, keychain identity, link client, QR scan and paste, code screen, refusal/expiry/cancel, computer list, session list from `bootstrap`, 12 shell catalogs. Files: `link/src/client.rs`, `link/tests/client.rs`, `phone-shell/`. | **Run:** 58 link tests (52 unit, 5 client loopback, 1 framing loopback), 37 shell catalog tests, UI typecheck and build, shell format, clippy and tests (0 Rust tests). Chromium render at 390 width in both themes with the native bridge stubbed, no console errors and no horizontal scroll. **Not run:** Android and iOS builds, emulator or device install, camera scan on a real phone. These need the SDKs and a device. |
| 4. App connection and session RPC | **Implemented.** `hello` and `welcome`, unary dispatch through the local router, event, output and history streams with sequence replay, phone link client, shell connection manager and live session screens. Files: `link/src/app.rs`, `link/src/reconnect.rs`, `link/src/client.rs`, `core/src/link/app.rs`, `phone-shell/src-tauri/src/commands.rs`, `phone-shell/ui/src/app.ts`, `live.ts`, `bridge.ts`, `messages.ts`. | **Implemented.** Rust integration with two real loopback endpoints: welcome, a unary call, a denied method, an event, output replay and live output, history replay and live versions, live revocation with close code 3, and reconnect delays on a fake clock. The link client test opens a history stream, reads the status, the streamed version and the final version. The phone shell has UI tests for the live list, output merge, history merge, replay after reconnect, item parsing and tool row expansion, and Rust tests for channels and terminal errors. The browser phone layout still runs against the core; Playwright cannot drive the native bridge without a device. |
| 5. UI package build and delivery | Vite phone config, packaging script, manifest, download, hash check, safe unzip, custom protocol serving, version gates. Files: `app/vite.phone.config.ts`, `scripts/package-phone-ui.mjs`, `core/src/link/ui.rs`, `phone-shell/src/`. | Node test: deterministic zip and matching hash. Rust test: hash mismatch rejected, traversal zip rejected, oversize rejected, atomic swap. Android: first download, desktop upgrade then phone update on connect, offline restart uses the stored build. |
| 6. Reconnect and revocation | **Implemented in part.** Backoff, output resume and the revocation close code exist. Network change and foreground reconnect controls remain. Files: `link/src/reconnect.rs`, `phone-shell/src-tauri/src/commands.rs`. | **Implemented in part.** Rust tests cover the backoff sequence on a fake clock, output replay from a requested sequence and close code 3. The shell subscription stores the last sequence and reopens from it after a reconnect. A full reconnect test on a device remains. |
| 7. Mobile actions | **Implemented in part:** terminal input with the key rail, latches, held-key repeat and saved keys; start, close, AI config switch and usage panel; permission, question and session-request cards; the one-line input in the browser phone layout; and the structured message view with the history stream on both phone surfaces. **Remaining:** upload, files, images and diffs. Files: `core/src/link/`, `link/src/method.rs`, `app/src/phone/`, `phone-shell/ui/`. | **Run:** link tests for the allowlist and the config route, a core loopback test for input and config switch, a core loopback test for history replay and live versions, phone shell UI tests for key encoding, latches, saved keys, action rules, history merge, replay after reconnect and tool row expansion, app tests for the message view states, and a Playwright test for the browser phone layout with recorded hook events. **Not run:** a device or emulator. Upload, file and image tests remain. |
| 8. Demo forwarding | **Implemented on the computer and the phone shell.** Forward ALPN, registered-port check, loopback connect, one tunnel per port, teardown. On the phone: loopback listener, same-or-free port choice, 8-tunnel registry, socket bridge, session list with local address, state and Open. Files: `link/src/forward.rs`, `link/src/client.rs`, `core/src/link/forward.rs`, `core/src/link/hub.rs`, `core/src/api.rs`, `core/src/demo.rs`, `core/src/mcp.rs`, `phone-shell/src-tauri/src/forward.rs`, `phone-shell/src-tauri/src/commands.rs`, `phone-shell/ui/src/demo.ts`, `phone-shell/ui/src/app.ts`, `phone-shell/ui/src/bridge.ts`. | **Run:** the link-crate forward tests, the core loopback test that registers a port, fetches a local HTTP server through the tunnel, refuses an unregistered port and refuses an unpaired phone, and the phone shell tests for port choice, refusal and a bridged socket. **Not run:** a device or emulator build, the phone's browser handoff, and any background behavior. |
| 9. CI and release | Android job with JDK, SDK, NDK and Rust targets. Shell version and APK artifact. Signing secrets. Path filters. | CI builds the APK on a phone-shell change, runs the Rust and JS tests, and uploads the artifact. Install the artifact on a device and check the version screen. |

First three steps in order: link crate, core endpoint and pairing, phone shell and pairing screen. They prove the riskiest parts first: the ticket format, the pairing handshake, and Tauri mobile on a real phone.

### Step 3 files and checks

| Path | Contents |
| --- | --- |
| `link/src/client.rs` | `PhoneStore`, `LinkClient`, `PairingMachine`, `PairingStart`, `PairingCancel`, `AppConnection`, `ComputerRecord`, frame readers and the client error type. |
| `link/tests/client.rs` | Five tests on loopback: pair and read `bootstrap`, a refused pairing, an oversized response header refused before allocation, an expired ticket refused before the dial, and the allowlist parser. |
| `phone-shell/src-tauri/src/store.rs` | Platform keychain identity and the JSON computer list. |
| `phone-shell/src-tauri/src/commands.rs` | The nine shell commands, the allowlist check and error codes for the screens. |
| `phone-shell/src-tauri/src/lib.rs` | The mobile entry point, the scanner plugin on mobile, and command registration. |
| `phone-shell/ui/src/` | Plain TypeScript screens: computers, pair, code, sessions, with the shared design tokens and 12 catalogs. |
| `phone-shell/test/catalogs.test.mjs` | Key parity, placeholders and empty-string checks across the 12 catalogs. |

Checks run on macOS with no Android or iOS SDK:

- `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings` and `cargo test` in `link/`: 58 tests pass.
- The same three in `phone-shell/src-tauri/`: 0 Rust tests, because the shell's logic sits in the link client and the screens. The crate builds for macOS.
- `npm --prefix phone-shell run test`: 37 tests pass.
- `npm --prefix phone-shell run typecheck` and `npm --prefix phone-shell run build`: both pass.
- The screens render in Chromium at 390 width in light and dark with a stubbed native bridge: no console errors and no horizontal scroll. This checks layout only, not the native bridge.

Not run: `tauri android init`, `tauri ios init`, an Android or iOS build, an emulator install, a real device, the camera, the keychain on a phone, and a connection over Wi-Fi or mobile data. The Android and iOS keychain stores compile only for their own targets and are not compiled here.

### Step 4 files and checks

| Path | Contents |
| --- | --- |
| `link/src/app.rs` | `AppHello`, `Welcome`, the welcome checks and the capped handshake reader and writer. |
| `link/src/reconnect.rs` | `ReconnectPolicy`, the delay table, jitter, the 30-second reset and the terminal error check. |
| `link/src/client.rs` | Handshake on connect, `EventStream`, `OutputStream`, `wait_closed` and the remote error. |
| `core/src/link/app.rs` | The app handler: handshake, method routes to the local router, event fan-out and output replay. |
| `phone-shell/src-tauri/src/commands.rs` | The connection manager, subscriptions, reconnect loop, `phone_subscribe` and `phone_unsubscribe`. |
| `phone-shell/ui/src/live.ts` | Session merge and output append, shared with the UI tests. |
| `phone-shell/ui/src/app.ts` | Live session list, session view and connection notices. |
| `phone-shell/test/live.test.mjs` | Seven tests for the live merge and output behavior. |

Checks run on macOS with no Android or iOS SDK:

- `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings` and `cargo test` in `link/`: 71 tests pass.
- The same three in `core/`: 259 tests pass, including nine link integration tests.
- The same three in `phone-shell/src-tauri/`: 2 Rust tests pass.
- `npm --prefix phone-shell run test`: 44 tests pass. `typecheck` and `build` pass.
- Playwright is not used for the link. The native bridge needs the Tauri shell and a paired phone key, so a browser test would not exercise it. The existing browser phone layout tests still run against the local HTTP API.

### Step 7 files and checks

| Path | Contents |
| --- | --- |
| `link/src/method.rs` | The `sessions.config` allowlist entry. |
| `core/src/link/app.rs` | The config route and its unit test. |
| `core/tests/link.rs` | A paired phone sends terminal input, reads bootstrap configs and reaches the config route. |
| `app/src/phone/keys.ts` | Base keys, agent key parsing, key encoding, latches, rail merge, saved-key validation and storage. Shared by both phone surfaces. |
| `app/src/phone/sessionActions.ts` | Switch choices, picker filtering, start request, close copy and pending-card selection. |
| `app/src/phone/PhoneApp.tsx` | One-line terminal input, key rail and saved-key sheet, start, close, switch and usage actions, and the permission, question and request cards. |
| `phone-shell/ui/src/app.ts` | The same actions and terminal input over the link methods. |
| `phone-shell/ui/src/combo.ts` | A filterable asynchronous combo box with keyboard and screen-reader support. |
| `phone-shell/test/keys.test.mjs` | Key encoding, agent merge, latches, repeat rules and saved keys. |
| `phone-shell/test/actions.test.mjs` | Switch choices, start request, usage text and pending cards. |
| `e2e/26-phone-actions.spec.ts` | The browser phone layout: input, rail toggle, latches, usage, switch, start and close. |

Checks run on macOS with no Android or iOS SDK:

- `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings` and `cargo test` in `link/`: 79 tests pass.
- The same three in `core/`: all tests pass, including the phone input and config checks in `core/tests/link.rs`.
- The same three in `phone-shell/src-tauri/`: 7 Rust tests pass.
- `npm --prefix phone-shell run test`: 69 tests pass. `typecheck` and `build` pass.
- `npm test`, `npm run typecheck` and `npm run build` at the root pass.
- Playwright runs the browser phone layout at 390 width.

**Message view.** The core publishes a bounded structured history for each agent session. Claude Code items come from its hooks (`UserPromptSubmit`, `MessageDisplay`, `PreToolUse`, `PostToolUse`, `Stop`), Codex items from the app-server `item/*` notifications (`turn/completed` ends a streamed reply), and OpenCode 2 items from the local server's message records, polled with the permission poll. Antigravity has no structured source and stays terminal-only. An item update is a new version with a higher sequence and the same opaque id. The log holds at most 400 versions and 128 KiB per session, persisted in `state.json`, with session secrets removed. The link method `history.subscribe` replays from `after` and follows live versions.

The browser phone layout and the native shell both show the message view for Claude Code, Codex and OpenCode 2: prompts and full replies, one-line tool rows that open on tap, a context strip with state, context left and branch, and one button to the read-only terminal. The composer sends a whole prompt. Terminal sessions and Antigravity open on the terminal with a short note, because they have no structured source. The shell keeps its own plain TypeScript renderer; it reuses the shared item merge and the history support test from `app/src/phone/history.ts` and parses stream values in `phone-shell/ui/src/messages.ts`. The shell subscribes to `history.subscribe` for the active session and keeps the output subscription, so the terminal view is ready at once. The shell reopens the history stream from the last sequence after a reconnect, and the merge ignores a replayed version that is not newer. The message view has not run in the native shell on a device.

### Message view files and checks

| Path | Contents |
| --- | --- |
| `core/src/history.rs` | Item model, bounded log with sequence replay, redaction hook and the Claude, Codex and OpenCode parsers, with recorded-payload tests. |
| `core/src/api.rs` | `GET /api/sessions/{id}/history` and the hook wiring. |
| `core/src/codex.rs`, `core/src/opencode.rs` | The app-server notification wiring and the server message poll. |
| `core/src/link/app.rs` | `history.subscribe` with replay, live versions and the `history.state` notice. |
| `link/src/method.rs`, `link/src/client.rs` | The allowlist entry, `HistoryStream` and the client call. |
| `link/tests/client.rs` | A loopback test for history replay from a sequence, the status event, a streamed version and the final version. |
| `phone-shell/src-tauri/src/commands.rs` | The `history` subscription channel. |
| `phone-shell/ui/src/app.ts`, `messages.ts` | Message view, context strip, view toggle, tool row expansion, history stream merge, the terminal-first note and the composer labels. The shell reuses `mergeHistory` and `historySupported` from `app/src/phone/history.ts`. |
| `phone-shell/test/history.test.mjs`, `messages.test.mjs` | The merge, replay after reconnect, item and status parsing, tool row expansion and agent support. |
| `app/src/phone/history.ts`, `MessageView.tsx`, `PhoneApp.tsx` | Item merge, message view, context strip, view toggle and composer for the browser layout. |
| `app/src/phone/history.test.ts`, `MessageView.test.tsx` | The merge and message view states. |
| `e2e/29-phone-messages.spec.ts` | The browser phone layout with recorded Claude hook events. |

Checks run on macOS with no Android or iOS SDK:

- `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings` and `cargo test` in `core/`: all tests pass, including the history parsers, redaction, bounded replay and the link history stream.
- The same three in `link/`: 80 tests pass, including the history stream loopback test.
- The same three in `phone-shell/src-tauri/`: 7 Rust tests pass, including the history channel parse.
- `npm --prefix app run test`: 211 tests pass, including the stream, merge and message view states.
- `npm --prefix phone-shell run test`: 79 tests pass, including the history merge, replay, parsing and tool row tests. `typecheck` and `build` pass.
- `npm run typecheck`, `npm run build` and the message-view Playwright test pass.

Not run: the native shell on a device or emulator. The message view, the terminal toggle and a live reconnect are checked on the host only.

### Step 8 files and checks

| Path | Contents |
| --- | --- |
| `link/src/forward.rs` | `ForwardConnection`, `ForwardTunnel`, `ForwardError`, `phone_address`, and the port handshake helpers. |
| `link/tests/client.rs` | A loopback test that opens a tunnel over the forward ALPN, echoes bytes and reads a refusal. |
| `phone-shell/src-tauri/src/forward.rs` | `ForwardRegistry`, `bind_local`, `check_port`, the accept loop and the bridge. |
| `phone-shell/src-tauri/src/commands.rs` | `registered_ports` and the two forward commands. |
| `phone-shell/ui/src/demo.ts` | `sessionDemoServers`, `demoServerRows`, `demoStateClass` and `forwardPhaseFor`. |
| `phone-shell/test/demo.test.mjs` | Four tests for session filtering, the 8-server cap, row addresses and error mapping. |

Checks run on macOS with no Android or iOS SDK:

- `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings` and `cargo test` in `link/`: 78 tests pass, including the forward loopback test.
- The same three in `phone-shell/src-tauri/`: 7 Rust tests pass, including port choice, refusal of an unregistered port and a socket bridged over a fake forward server.
- `npm --prefix phone-shell run test`: 48 tests pass. `typecheck` and `build` pass.
- The screens render in Chromium at 390 width in both themes with a stubbed native bridge. This checks layout only.

Not run: an Android or iOS build, an emulator, a real device, the camera, the platform keychain, the system browser handoff, and background or lock-screen behavior. The Open action uses the official opener plugin; its permission scope is `http://127.0.0.1:*` and needs a device check.

### Test harness

- Rust link tests use `presets::Minimal` and direct addresses on loopback. They do not need the public relay or the internet.
- The core test starts on a random port with a temporary data directory, as the existing API tests do.
- The fake phone uses the same `link` client code as the real shell. This tests the protocol, not a mock.
- The Android emulator cannot use a camera. Use the paste fallback in debug builds. The real camera test runs on a device.
- Keep the existing browser tests. The phone layout must still work at `/phone.html` against the local core.

## 8. Risks and unknowns

| Risk | Why it matters | Test |
| --- | --- | --- |
| iroh on Android | QUIC over UDP, battery use, relay reconnect and app backgrounding may behave differently from the desktop. | Run the test build on a real Android phone on Wi-Fi and mobile data for a full day. Check relay fallback and reconnect. |
| Phone shell has no device run yet | Steps 3, 4 and 8 prove the client, the listener and the screens on the host. The native keychain, the scanner, the mobile build, the browser handoff and background behavior are untested. | Generate the Android project, install on an emulator, pair with a pasted ticket against a core on the host, register and open a demo server, then repeat on a phone with the camera. |
| Tauri mobile maturity | The build toolchain is heavy, plugins may lag, and the mobile developer experience is young. | Build the shell in CI early. If a blocker appears, use the spec's fallback: React Native with a web view and a uniffi bridge. |
| Barcode scanner and ML Kit | The Tauri plugin uses the Play Services model, which downloads on first use. Pairing must work on a fresh or offline device. | Override Gradle to use the bundled `com.google.mlkit:barcode-scanning` artifact. Test on a fresh device without Play Services. |
| UI package is executable code | The phone runs JavaScript authored by the computer. A compromised computer can use the bridge. | Review the bridge command set. Test that the package cannot reach the fs, shell or HTTP plugins. |
| Version skew | A new package can meet an old shell, or the reverse. | Test all four combinations: old and new shell, old and new computer. Check the two update messages. |
| Public relays | Rate limits, no uptime guarantee, and n0 can drop old iroh versions. | Test a long session over mobile data. Keep direct LAN connections working without a relay. |
| mDNS on Android | Android drops multicast frames unless the app holds a multicast lock and has `CHANGE_WIFI_MULTICAST_STATE`. | Hold the lock on resume and release it on pause. Test same-Wi-Fi discovery with the relay blocked. |
| Pairing code photographed | Someone can pair within the 120-second window if they see the QR. | Keep the window short. Show the phone name and code on the computer. Test approve and deny. |
| Android background limits | Android may suspend the app when the user switches to the browser, so a demo tunnel can stop. This build does not request a foreground service and does not stop forwarding on pause by itself. | Test with Chrome in front on a real device. Add a foreground service only if the test proves it is needed. |
| Phone listener and browser handoff | The loopback listener and the Open action use mobile APIs that no CI job runs. | Install the shell on a device, pair, register a demo server, open its local address, and check that the page loads and live reload works. |
| Package size and startup | A large bundle slows the first connect and uses phone storage. | Measure the phone build. Cap the zip at 64 MiB and keep one previous build. |
| iroh API churn and MSRV | iroh 1.5.0 is new and needs Rust 1.91. The core and link now require it; the desktop shell keeps its own MSRV. Upgrades may break the build. | Pin exact versions, commit the lock file, and run the core and link tests on every upgrade. |
| iPhone and store rules | Apple's rules on downloaded code are a risk for the iPhone app. | Out of scope for version 1. Decide the iPhone route later, as the spec says. |

The three biggest risks are iroh on Android, the Tauri mobile and barcode scanner toolchain, and the UI package's bridge access. The test build must settle all three before the rest of the phone work starts.

## 9. Open decisions

| Decision | Recommendation |
| --- | --- |
| Show a confirmation code on the computer during pairing | Yes. It confirms the channel and blocks accidental pairings. |
| mDNS on by default | Yes, with the relay as the fallback. |
| Keep one previous UI build | Yes. It gives a fallback when a new package fails to load. |
| Allow rename, delete or reopen from the phone | No in version 1. The spec lists close and start, not these. |
| Foreground service for demo forwarding | Test first. Add it only if Android suspends the app during a normal review. |
| Android minimum version | API 26 (Android 8). It gives modern Keystore and WebView behavior. |
| Shell screens framework | Decided and implemented: plain TypeScript and the shared design tokens. The screens are small and do not depend on the package. |

## Sources

- iroh overview and relay limits: https://iroh.computer/docs/overview and https://docs.iroh.computer/iroh-services/relays/public.md
- iroh protocol guide: https://docs.iroh.computer/protocols/writing-a-protocol.md
- iroh connect two endpoints: https://docs.iroh.computer/connect-two-endpoints.md
- iroh tickets: https://docs.iroh.computer/concepts/tickets.md
- iroh local address lookup: https://docs.iroh.computer/connecting/local-address-lookup.md
- iroh 1.5.0 crate and features: https://docs.rs/iroh/1.5.0/iroh/ and https://docs.rs/crate/iroh/1.5.0/features
- iroh releases: https://github.com/n0-computer/iroh/releases
- iroh Android tests: https://github.com/n0-computer/iroh/blob/main/.github/workflows/test-android.yml
- Tauri 2 mobile prerequisites: https://v2.tauri.app/start/prerequisites/
- Tauri custom protocol and origin fix: https://v2.tauri.app/reference/webview-versions/ and https://osv.dev/vulnerability/GHSA-7gmj-67g7-phm9
- Tauri barcode scanner: https://v2.tauri.app/plugin/barcode-scanner/
- Tauri project structure and mobile entry point: https://v2.tauri.app/start/project-structure/
- Google Play downloaded code policy: https://support.google.com/googleplay/android-developer/answer/9888379
- Apple App Review guidelines 2.5.2 and 4.2.7: https://developer.apple.com/app-store/review/guidelines/
- Android multicast lock requirement: https://github.com/outlmd/outl/blob/main/docs/android-platform.md
