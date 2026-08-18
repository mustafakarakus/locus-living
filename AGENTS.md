# Home AI — Coding Agent Contract

Vendor-neutral instructions for every coding agent (Grok, Claude Code, Codex, Cursor).
Humans read `README.md`. Agents read this file, then the docs it names.

Do not copy those docs into this file. Do not add vendor-specific instruction files
except thin pointers that send the agent here.

## Sources of truth

| File | Role |
|---|---|
| `docs/techstack.md` | Implementation. Languages, ports, paths, models, schemas. |
| `docs/usecases.md` | Features, milestone order, acceptance, tests. |
| `docs/agent-architecture.md` | Product agents, bus, priority, isolation. |
| `README.md` | Vision and principles. |

**Precedence when documents conflict:**

1. The user's current prompt
2. This file
3. `docs/techstack.md` — wins every stack, port, path, and model dispute
4. `docs/usecases.md` — feature scope and acceptance
5. `docs/agent-architecture.md`

If a use case and the tech stack disagree, implement the tech stack and note the mismatch.

**Never read, edit, or commit `ideas.md`.** It is private scratch and is gitignored.

## Current scope

Only **Milestone 1** is in scope until UC-133 (Demo Script) passes 20 consecutive offline runs.

Do not implement Milestone 2, 3, or 4 features before that exit. Use-case IDs are stable
references, not a license to skip ahead.

## How to implement a use case

1. Search `docs/usecases.md` for `UC-NNN`. Read Problem, Solution, Acceptance, Tests, Dependencies.
2. Confirm the stack in `docs/techstack.md`. Do not substitute languages, frameworks, ports, paths, or models.
3. If the work is an agent, read `docs/agent-architecture.md`.
4. Implement the smallest change that satisfies Acceptance and Tests.
5. Update the use-case **Status** (`TODO` → `IN_PROGRESS` → `DONE`).
6. Expand compact Milestone 2–4 entries to the full UC format before implementing them.

A use case is `DONE` only when implementation, acceptance, and tests all pass, offline
behavior is verified, and earlier use cases still pass.

## Hard rules

- Local-first. Home Core never initiates outbound internet.
- Deterministic agents must work with the LLM down. The Voice fast-path (UC-121/129) is deterministic.
- Agents communicate only via the in-process bus (`homeai.HomeEvent`). No direct agent-to-agent calls.
- No fake intelligence. Do not pretend a manual appliance is smart.
- Graceful degradation. AI failure never breaks basic home functions.
- User in control. Ask before high-risk actions.
- Do not add cloud services, new ports, or new languages unless `docs/techstack.md` already names them.
- If something is unspecified, choose the most boring deterministic option and note it.

## Repository

One monorepo. Layout and client roles: `docs/techstack.md` §12–§14.

| Path | What | When |
|---|---|---|
| `crates/core` | Home Core (agents as modules) | Milestone 1 |
| `crates/noded` | Full room node | Milestone 1 |
| `crates/cli` | `homeai admin` | Milestone 1 |
| `crates/proto` + `proto/` | Wire schema | Milestone 1 (UC-102) |
| `workers/stt`, `workers/tts` | Local STT/TTS façades | Milestone 1 (UC-117/119) |
| `tools/simulator` | Virtual house | Milestone 1 (UC-134) |
| `apps/tablet` | Android Compose output kiosk | Milestone 2 (UC-223) |
| `apps/mobile` | Compose Multiplatform away/admin | Milestone 2 |
| `apps/web` | Cinematic Three.js landing | When the public site is needed |
| `firmware/satellite` | ESP32 satellite | Milestone 2 (UC-240) |
| `addons/` | Subscribed domain bundles | Milestone 4 (UC-401) |

Do not introduce React Native, Flutter, Astro, or Electron.
Mobile is Compose Multiplatform (shared UI, native shells for VPN/BLE/push).
The tablet is a separate Android Compose kiosk — do not reuse phone screens.
The landing page never talks to a Home Core.

Nested `AGENTS.md` files under `crates/`, `workers/`, and `apps/*` add surface-specific
rules. They do not replace this file.

## When the codebase exists

Follow the existing layout and test commands. Update tests with the change.
Do not introduce a second event bus, a second database, or a second LLM client.
