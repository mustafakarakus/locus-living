# Home AI

A local-first, AI-powered home intelligence platform for new-build properties.

## Vision
The user should think *"my home understands what is happening"* — not *"I need to control my smart home."*
The house notices, understands, remembers, predicts, assists, automates, and communicates.
The ideal interaction is often no interaction at all.

## Core Principles
- **Local First** — works with zero internet.
- **No Fake Intelligence** — don't pretend a manual appliance is smart.
- **Context Before Commands** — understand who/where/when before asking.
- **Silent Intelligence** — the best automation is the one you never notice.
- **User in Control** — AI suggests; asks before high-risk actions.
- **Graceful Degradation** — AI failure never breaks basic home functions.
- **Offline but Upgradeable** — zero cloud dependency; updates are signed bundles delivered by the
  owner's phone/tablet and installed only on explicit user command ("update system").
- **Hardware-Agnostic Core** — runs on any qualified Linux box (capability tiers T1–T3); the Core
  can be swapped for stronger hardware years later via backup/restore, unlocking larger models
  with no re-provisioning.
- **New-Build Only** — designed into the house during construction.
- **All Intelligence Included** — no paywall; add-ons are optional domain bundles.
- **Zero Cloud Dependency** — Home Core never connects outbound.

## Milestones (outcome-based, not count-based)
1. **Intensive Live Demo** — a real, offline, repeatable demo. Exits when the Demo Script passes.
2. **Must-Haves** — a safe, reliable, installable product. Exits at Production Release Checklist.
3. **Nice-to-Haves** — valuable, not launch-blocking.
4. **Plugins & Add-Ons** — subscription bundles and domain extensions.

## Documents
- `AGENTS.md` — contract for every coding agent (Grok, Claude, Codex, Cursor).
- `docs/usecases.md` — milestone-ordered use cases (source of feature truth).
- `docs/techstack.md` — deterministic tech stack (source of implementation truth).
- `docs/agent-architecture.md` — product agent architecture.

AI-driven development uses `AGENTS.md` as the single instruction file. Vendor-specific
files (`CLAUDE.md`, and later Copilot/Cursor pointers if needed) only point at `AGENTS.md`.

## Repository
Monorepo. The Core is the authority. Voice is the in-home input. The tablet is an
output dashboard. The phone is away/admin (notifications, confirmations, updates).
The cinematic landing site is the only public-internet surface and never talks to a home.

| Path | Role |
|---|---|
| `crates/core` | `homeai-core` — local brain (Rust) |
| `crates/noded` | Full room node daemon |
| `crates/cli` | Admin CLI |
| `workers/` | Local STT / TTS |
| `apps/tablet` | Android Compose wall display (output) |
| `apps/mobile` | Compose Multiplatform owner phone |
| `apps/web` | Cinematic Three.js landing |
| `addons/` | Optional subscribed bundles (Milestone 4) |

Full layout and stack: `docs/techstack.md` §12–§14.

## Privacy
All data generated, processed, and stored locally on the Home Core. The Home Core never initiates
an outbound internet connection. Updates are signed bundles delivered by the homeowner's
phone/tablet over LAN or VPN and installed only when the user explicitly asks; the Core never
fetches anything itself. Notifications while away travel only through the phone-initiated VPN
tunnel.