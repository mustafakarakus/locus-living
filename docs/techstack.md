# Home AI — Technology Stack (Source of Truth)

Single source of truth for all implementation decisions. Every use case in `docs/usecases.md` and every
agent in `docs/agent-architecture.md` MUST conform to this stack. If a use case and this document conflict, THIS
DOCUMENT WINS.

**Rule for AI agents:** Do not substitute languages, frameworks, ports, paths, or models. If
something is unspecified here, choose the most boring, deterministic option and note it.

## 1. Languages & Runtimes
| Component | Language | Version |
|---|---|---|
| Home Core runtime | Rust | stable 1.80+ |
| Room Node daemon | Rust | stable 1.80+ |
| Admin CLI | Rust | stable 1.80+ |
| AI workers (STT/TTS servers) | Python | 3.11 |
| Home Tablet UI | Kotlin + Jetpack Compose | Android 14+, tablet/PoE panel |
| Mobile app | Kotlin + Compose Multiplatform | CMP 1.11+, iOS 17+, Android 14+ |
| Cinematic landing | TypeScript + Vite + Three.js | Three.js r170+, GSAP 3, TS 5 |

Rust async: **tokio**. HTTP: **axum**. gRPC: **tonic**. Protobuf: **prost**.
DB: **rusqlite** over **SQLite (WAL)**. Config: **toml + serde**. Logging: **tracing** (JSON).

**Process model:** `homeai-core` is a single process; each agent (see `docs/agent-architecture.md`) runs as an
independent supervised tokio task with panic isolation. The event bus is in-process.
**DB concurrency:** one dedicated writer thread owns the rusqlite write connection; async tasks
reach it via a channel; reads use a small WAL read pool.
**Device interop:** the `Device` trait (UC-127) is implemented by protocol adapters. Supported
protocols: **Matter** (incl. Thread — the cabinet 802.15.4 dongle acts as border router),
**Zigbee** (same dongle; unlocks cheap battery sensors: door/window ~$8, leak ~$10), **KNX**
(IP gateway, new-build wiring), and native node GPIO/relay. New device families are added as
adapters, never as one-off code paths.

## 2. Hardware Profiles (Hardware-Agnostic Core)
The Home Core is **hardware-agnostic**: any Linux box (x86_64 or arm64) that meets a capability
tier below can run it. The Core must be replaceable years later (e.g. mini-PC → DGX Spark →
Mac Studio class) via backup/restore migration (UC-238) with zero re-provisioning of nodes.
The active tier is declared in `/etc/homeai/models.toml`; model choices bind to the tier, not to
the physical device (UC-237).

| Tier | Example hardware | Capability |
|---|---|---|
| T1 Minimum | 32GB mini-PC / SBC with NPU | Fast-path + smaller LLM (Qwen3-4B class), relaxed latency |
| T2 Standard | 48–64GB (Apple Mx / RTX box) | Full reference model stack at target latency |
| T3 Max | DGX Spark 128GB class | Larger models, bigger context, headroom for add-ons |

| Profile | Hardware | Role |
|---|---|---|
| Dev | Apple M4 Pro Max (48GB) | Development, latency tuning |
| Prod Home Core | Any T1–T3 qualified box | Runs all core + AI services |
| Room Node (full) | Linux SBC (RPi CM4/CM5 class) | Main rooms: mic, speaker, sensors, BLE, wake word |
| Room Node (satellite) | ESP32-S3 class ($15–40 BOM) | Minor rooms: mic, speaker, on-chip wake word (UC-240) |
| Home Tablet | Android tablet / PoE panel | Ambient output kiosk (UC-223). Not an iPad, not a webview. |

OS for Core and full Nodes: **Ubuntu 24.04 LTS** (satellites run dedicated firmware). Core disk
uses **LUKS full-disk encryption**.
Node hardware requirements: hardware mic-mute switch (cuts mic power electrically) + status LED
(listening/thinking/muted) on every node class; mmWave = LD2410/LD2450-class FMCW modules (UART);
full-node carriers reserve a UWB (DW3000-class) footprint for the identity upgrade path (UC-112).
Cabinet hardware (UC-230): 802.15.4 dongle (Thread/Zigbee), battery-backed RTC (UC-241), UPS for
Core + PoE switch (UC-242).
Dev (Apple Silicon/Metal) vs Prod (CUDA/other) parity: CI must run the acceptance suite on
prod-class hardware before release; latency numbers tuned on dev hardware are not authoritative.

## 3. Network & Ports (FIXED)
All traffic LAN-only. Home Core NEVER initiates outbound internet. The only remote path is an
inbound-initiated WireGuard VPN from the owner's phone (UC-224); notifications and update
transfers travel only through already-established tunnels.
| Port | Protocol | Service | Bound to |
|---|---|---|---|
| 8443 | HTTPS + WSS | Local API | Core |
| 50051 | gRPC (tonic, mTLS) | Room Node ↔ Core | Core |
| 8200 | HTTP | LLM server (llama-server, OpenAI-compatible) | Core, localhost |
| 8250 | HTTP | Vision-language server (llama-server, OpenAI-compatible) | Core, localhost |
| 8100 | HTTP | STT façade (primary + fallback) | Core, localhost |
| 8300 | HTTP | TTS façade (primary + fallback) | Core, localhost |
| 8500 | HTTP | Prometheus metrics | Core, localhost |
| 3000 | HTTP | Grafana dashboards (UC-233) | Core, localhost |
| 8883 | MQTTS | Satellite nodes ↔ Core, embedded broker (UC-240) | Core |
| 22 | SSH | Admin only — key-only auth, no passwords | Core/Nodes |

TLS on 8443: self-signed cert from provisioning (`/etc/homeai/tls/`). Provisioning installs the
cert on every client (tablet, phone, CLI); clients pin it and hard-fail on mismatch.
gRPC 50051 uses mTLS: per-node client certificates issued at provisioning; unauthenticated
`Register` or streams are rejected.
API auth: per-client bearer tokens with scopes (`read`, `control`, `admin`), rotatable and
revocable via `homeai admin token`; failed-auth attempts are rate-limited with lockout.

## 4. Filesystem Layout (FIXED)
```
/etc/homeai/config.toml
/etc/homeai/house.toml
/etc/homeai/models.toml          # model manifest: tier, model versions, resolved paths
/etc/homeai/tls/                 # API cert + per-node mTLS certs + scoped client tokens
/etc/homeai/update-pubkey.pem    # update-bundle verification key (public only)
/var/lib/homeai/home.db              # includes append-only event_log (UC-102)
/var/lib/homeai/memory/
/var/lib/homeai/attachments/         # short-lived photo handoffs; session retention (UC-244)
/var/lib/homeai/diagnostics/         # encrypted, expiring support exports (UC-245)
/var/lib/homeai/updates/         # staged signed update bundles (A/B, rollback)
/opt/homeai/models/current/      # symlinks resolved via models.toml (upgradeable)
/opt/homeai/models/llm/qwen3-30b-a3b-q4_k_m.gguf   # T2/T3 reference (MoE)
/opt/homeai/models/llm/qwen3-4b-q4_k_m.gguf        # T1
/opt/homeai/models/llm/qwen3-0.6b-draft.gguf       # speculative-decoding draft
/opt/homeai/models/vision/                         # per-tier local VLM (UC-244)
/opt/homeai/models/stt/qwen3-asr-0.6b/
/opt/homeai/models/tts/chatterbox-multilingual/
/opt/homeai/models/tts/piper/               # tr_TR, en_US, nl_NL, ar_JO voices
/opt/homeai/models/wakeword/hey-home.onnx
/var/log/homeai/
```

**Dev prefix:** production uses the paths above with no override. On a developer machine set
`HOMEAI_PREFIX` (e.g. `./.run`) and the same layout is rooted under that directory:
`$HOMEAI_PREFIX/etc/homeai/config.toml`, `$HOMEAI_PREFIX/var/lib/homeai/home.db`,
`$HOMEAI_PREFIX/var/log/homeai/core.log`. Do not invent a second config format. If the
prefix is set and TLS material is missing, `homeai-core` may write a self-signed dev cert
into `$HOMEAI_PREFIX/etc/homeai/tls/`. Without a prefix, missing certs are fatal.

## 5. Systemd Units (FIXED)
Core: `homeai-core.service`, `homeai-llm.service`, `homeai-vision.service`,
`homeai-stt.service`, `homeai-tts.service`.
Node: `homeai-noded.service`. All `Restart=on-failure`, `RestartSec=2` with restart limits
(`StartLimitIntervalSec`/`StartLimitBurst`); repeated crash loops raise a health alert (UC-233).

## 6. AI Model Stack (FIXED per release, upgradeable via manifest)
Models are referenced through `/etc/homeai/models.toml` + `/opt/homeai/models/current/` symlinks
and swapped only by signed updates (UC-234), gated by the acceptance suite (UC-132). "FIXED"
means fixed per release — never hand-substituted, but upgradeable as hardware and models improve.
Per-tier defaults (§2): T1 uses smaller variants (Qwen3-4B class LLM); T2/T3 use the reference
models below.

| Stage | Reference model (T2) | Serving | Port |
|---|---|---|---|
| Wake word | "hey home" — engine by bake-off (openWakeWord reference) | on-node | — |
| STT primary | Qwen3-ASR-0.6B | STT façade: Python FastAPI streaming | 8100 |
| STT fallback | whisper.cpp large-v3-turbo | same façade, engine swap on failure | 8100 |
| LLM | Qwen3-30B-A3B Q4_K_M (MoE) + Qwen3-0.6B draft (speculative decoding) | llama-server (OpenAI API) | 8200 |
| Vision-language | Qwen2.5-VL-7B-class quantized (smaller per-tier variant on T1) | llama-server multimodal OpenAI API | 8250 |
| TTS primary | Chatterbox Multilingual (MIT; TR/EN/NL/AR among 23 langs) | TTS façade: Python FastAPI streaming PCM | 8300 |
| TTS fallback | Piper (tr_TR, en_US, nl_NL, ar_JO) | same façade invokes piper CLI on failure | 8300 |

LLM rationale: ~3B active params gives 8B-class decode speed with better reasoning/tool-calling;
Q4 (~20GB) fits T2. T1 runs Qwen3-4B dense. Speculative decoding with the 0.6B draft targets
1.5–2.5× decode speedup. MoE quantization quality is gated by the UC-132 suite before any
manifest swap; Qwen3-8B dense remains the sanctioned fallback reference.

One façade service per port (`homeai-stt.service`, `homeai-tts.service`) wraps primary + fallback;
fallback engines are not separate network services.
**Language priority (binding for all STT/TTS/model choices): Turkish = required, English =
required, Dutch = big plus, Arabic = future plus.** The façade may route per language to whichever
engine scores best (per the model manifest).
Pre-build verification (before any UC depends on them): STT language matrix — if Qwen3-ASR lacks
or is weak in Turkish/Dutch, whisper.cpp large-v3-turbo becomes STT primary; TTS — use the
Chatterbox **Multilingual** variant (covers TR/NL/AR), verified in priority order, with Piper
(tr_TR, nl_NL, ar_JO) as fallback; Qwen3-ASR true streaming-partial support; wake-word engine
bake-off — prototype openWakeWord first (synthetic-TTS training data, days), sherpa-onnx if its
node footprint wins, Porcupine only if on-prem licensing is acceptable; a custom "hey home" model
incl. Turkish-accent data is a sub-project either way.

Latency targets, measured wake → first audio (UC-123), on T2 hardware (T1 uses a documented
relaxation factor): deterministic fast-path < 400ms; LLM-answered simple query < 800ms;
complex < 1500ms.

## 7. Protobuf Schemas (FIXED)
```proto
syntax = "proto3";
package homeai;

message HomeEvent {
  string event_id = 1;
  string event_type = 2;
  string source_id = 3;
  string room_id = 4;
  string person_id = 5;
  int64  timestamp_ms = 6;
  double confidence = 7;
  bytes  payload = 8;
  uint32 schema_version = 9;  // event schema compat across Core/Node update skew
}

// Every payload is schema-validated before bus publish; invalid events are dropped and logged.
// Events persist in the home.db append-only event_log table (UC-102); schema_version applies there too.
// Satellite nodes (UC-240) use a slim MQTT-over-TLS profile on 8883 instead of NodeService;
// full nodes use the gRPC service below.

service NodeService {
  rpc Register(NodeInfo) returns (RegisterAck);
  rpc StreamEvents(stream HomeEvent) returns (stream NodeCommand);
  rpc Health(NodeHealthRequest) returns (NodeHealth);
}

message NodeInfo { string node_id = 1; string room_id = 2; repeated string capabilities = 3; }
message RegisterAck { bool ok = 1; string assigned_room_id = 2; }
message NodeCommand { string command_id = 1; string target_device_id = 2; string action = 3; bytes params = 4; }
message NodeHealthRequest { string node_id = 1; }
message NodeHealth { string status = 1; int64 uptime_s = 2; }
```

## 8. Local API Contract (FIXED)
Base `https://<core>:8443`. Auth: bearer token.
| Method | Path | Purpose |
|---|---|---|
| GET | `/api/v1/house` | Full house model |
| GET | `/api/v1/rooms` | List rooms |
| GET | `/api/v1/rooms/{id}` | Room detail + devices + presence |
| GET | `/api/v1/devices` | List devices |
| POST | `/api/v1/devices/{id}/command` | Send device command |
| GET | `/api/v1/presence` | Current presence + identity |
| POST | `/api/v1/voice/say` | Speak text in a room |
| GET | `/api/v1/status` | Home status summary |
| GET | `/api/v1/health` | System health |
| POST | `/api/v1/conversations/{id}/attachments` | Upload photo to a short-lived request (UC-244) |
| POST | `/api/v1/system/diagnostics/export` | Create consented, PII-stripped support bundle (admin) |
| POST | `/api/v1/system/ownership-transfer` | Start reviewed ownership transfer (admin + HIGH) |
| POST | `/api/v1/system/factory-reset` | Wipe everything and re-enter provisioning (admin + physical) |
| POST | `/api/v1/system/update` | Stage/install signed update bundle (admin scope) |
| WS | `/ws/events` | Live event stream (bearer auth enforced at upgrade) |

`POST /api/v1/voice/say` requires `control` scope and every use is logged (abuse amplifier if a
token leaks).

## 9. Database Schema (SQLite)
Tables: `property, floor, room, device, sensor, device_state, person, identity_signal,
presence_event, automation, automation_rule, automation_execution, event_log, home_memory,
user_preference, conversation_session, conversation_attachment, system_health`.

## 10. Updates & Hardware Migration (Offline-First)
The Core stays fully offline; it never fetches updates itself. Turning WiFi/WAN off never degrades
any feature.
- **Delivery:** the owner's phone/tablet app downloads a signed update bundle over its own
  internet, then transfers it to the Core over LAN or the phone-initiated VPN, into
  `/var/lib/homeai/updates/`.
- **Trigger:** the user initiates installation explicitly ("update system") from the Home Tablet,
  mobile app, or voice — a HIGH-risk action requiring confirmation. Never automatic, never
  outbound. The trigger surface is device-agnostic: anything holding an `admin`-scoped token may
  call `POST /api/v1/system/update`.
- **Integrity:** bundles are Ed25519-signed; Core verifies against `/etc/homeai/update-pubkey.pem`.
  Two-tier key hierarchy: an offline ROOT key signs only key-rotation bundles; a subordinate
  update-signing key signs releases. Update-key compromise → root-signed successor-key bundle;
  root compromise → documented physical re-provisioning (break-glass). Node mTLS certs are
  medium-lived and auto-renewed by the Core (local CA) over the existing authenticated channel
  before expiry (UC-107).
- **Safety:** A/B staged install with automatic rollback on failed post-install health check.
- **Architecture-aware:** bundles carry x86_64 and arm64 binaries plus per-tier model manifests,
  so the same update serves a T1 mini-PC, a DGX Spark, or a future device (UC-237).
- **Hardware migration:** `homeai admin backup` produces a portable archive (DB, config, certs,
  memory, house model). Restore on any qualified box re-establishes the same home; nodes reconnect
  without re-provisioning; stronger hardware adopts a higher tier's models via manifest only
  (UC-238).

## 11. Development Order
Follow `docs/usecases.md` milestones strictly. Complete Milestone 1 (Demo Script passes) before starting
Milestone 2. Do not build Milestone 3/4 features before Milestone 2 exits.

## 12. Client Surfaces
The house is not a tablet-managed smart home. Voice is the in-home input. The Core is the
authority. Clients are thin. Household UIs are compiled native — no React Native, no Flutter,
no Electron, no webview kiosk.

| Surface | Lives | Role |
|---|---|---|
| Voice (nodes) | In-home | Primary input. Wake → command or conversation. |
| Home Tablet (`apps/tablet`) | In-home, wall-mounted Android / PoE panel | **Output.** Android Jetpack Compose kiosk: status, presence, calendar, notifications, energy (UC-223). Touch is for HIGH confirmations and rare exceptions, not day-to-day management. Instant. **Not** the mobile Compose app in a larger window. |
| Mobile (`apps/mobile`) | Owner's phone | **Away + admin.** One Kotlin + Compose Multiplatform UI, two store binaries. Notifications, HIGH confirmations, photo handoff (UC-244), WireGuard VPN (UC-224), signed update delivery (UC-234), time correction (UC-241), diagnostics export. |
| Cinematic landing (`apps/web`) | Public internet | Company site and product film. The only cloud-hosted surface. Never talks to a Home Core. See §13. |
| Admin CLI (`crates/cli`) | Installer / owner laptop | Provisioning, tokens, backup/restore. LAN only. |

Grafana on `:3000` is an installer diagnostic, not a household UI.

**Why these client stacks (binding):**
- **Tablet = Android Compose, its own app.** UC-231 puts displays on PoE. Commercial wall panels are Android, not iPad. Ambient output is a different product from the phone — do not reuse phone screens.
- **Phone = Kotlin + Compose Multiplatform (one UI).** Compose for iOS is stable (1.8+, current 1.11+). This phone is a utility (notify, confirm, VPN, deliver updates), not the luxury cinematic surface — that is `apps/web`. Sharing UI is the right trade. You still ship an `.ipa` and an `.apk`. You still write **thin native shells** for WireGuard (Network Extension / VpnService), APNs/FCM, Keychain/Keystore, and BLE. Those stay `expect`/`actual` or platform modules. Do not pretend CMP wraps Network Extension.
- **Not React Native, not Flutter, not Swift-on-Android.** CMP is the one cross-UI we allow, because the tablet and the Android phone are already Compose, and iOS text/scroll are production-ready enough for this surface.
- **iOS feel:** use CMP's native text-input opt-in. Do not fake iOS navigation with Material-only widgets. Platform theme (Cupertino-adjacent on iOS, Material 3 on Android) is allowed; two fully separate SwiftUI/Compose UIs are not.
- **Landing ≠ Astro.** Astro is for content sites. This landing page is a continuous WebGL film. See §13.

## 13. Cinematic landing (`apps/web`)
A single-page WebGL experience that *demonstrates* Locus Living. It is not a brochure and it is
not connected to anyone's house.

**Story (binding product intent, not yet implemented):**
Angled bird's-eye of a luxury villa — garden, facade, approach. Interaction dollys into the
garden and plays a Locus garden vignette, then walks through the entrance, living room,
kitchen ("I have tomatoes, basil, vegetables — what can I cook?" → an immediate dish + offer
to share the recipe), bedroom, and further rooms. Seamless camera. The visitor should feel
*intelligence*, not a smart-home control panel.

**Stack (binding):** TypeScript + **Vite** + **Three.js** + **GSAP** (camera/timeline). Villa
authored in Blender, shipped as streamed glTF with LODs. HUD (dialogue, recipe card) is DOM
overlay — not a second 3D world. React is allowed only as a HUD layer; the scene graph is
Three.js, not React Three Fiber.

**Hard rules:**
- Never call a Home Core, never ship home TLS material, never show a real customer's house.
- Vignettes are **authored** to real use cases (garden, entrance, kitchen, bedroom). The tour
  must play without a live model. An optional hosted/demo LLM may colour a line; if it is down,
  the scripted line still plays.
- First visit must stay light: one hero villa, streamed LODs, compressed textures. Phones get a
  reduced scene or a cinematic video fallback — do not ship a 200MB WebGL download.
- Do not use Unreal Pixel Streaming or Unity WebGL for v1 (GPU servers, huge payloads, bounce).
  A later "film quality" mode may be considered; it is not the default landing.
- Milestone 4 add-on *purchase* may be a route on this same origin. Activation remains a signed
  offline file the phone delivers (UC-401). Purchase must never become a runtime check inside
  `homeai-core`.

## 14. Repository Layout (monorepo)
One repo. Cargo workspace for Rust. Vite app for the cinematic site only. Native apps are
Gradle/Xcode projects. Python workers are standalone packages. Do not split the Core into
multiple processes or add a second API.

```
crates/                 # Rust (Cargo workspace)
  common/               # shared config, errors, paths
  proto/                # prost/tonic types from proto/homeai.proto
  core/                 # homeai-core — single process, agents as tokio tasks
  noded/                # homeai-noded — full room node
  cli/                  # homeai admin
proto/                  # canonical .proto (source of truth for codegen)
workers/                # Python 3.11 façades (localhost only)
  stt/                  # :8100
  tts/                  # :8300
apps/
  tablet/               # Android Compose kiosk — output dashboard (not the phone UI)
  mobile/               # one CMP product, two binaries
    composeApp/         # shared Kotlin UI + viewmodels + API client
    iosApp/             # thin Xcode shell: Network Extension, APNs, BLE
    androidApp/         # thin Gradle shell: VpnService, FCM, BLE
  web/                  # Cinematic Three.js landing — never talks to a Core
firmware/
  satellite/            # ESP32-S3 satellite (UC-240, Milestone 2)
addons/                 # signed domain bundles (Milestone 4). Empty until UC-401.
tools/
  simulator/            # virtual house (UC-134)
  release/              # vendor-side update signing. Never runs on a Core.
deploy/
  systemd/              # unit files
  config/               # example config.toml, house.toml, models.toml
docs/                   # product + agent contract sources
```

LLM and vision servers are **llama-server** (not first-party crates). They are packaged in
`deploy/` and referenced by `models.toml`.

**Add-ons** compile to signed bundles the phone delivers. They must not be compiled into
`homeai-core` by default. Core features are never gated on a subscription (UC-235).