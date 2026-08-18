# Home AI — Agent Architecture

Agents are the intelligence layer. Deterministic agents must work even if the LLM is unavailable.
Conforms to `docs/techstack.md`.

## Agent Categories
- **Deterministic (no AI):** Presence, Automation, Security, Climate, Room Manager, Memory, Notification.
- **AI (require LLM):** Voice, Conversation, Proactive, Reasoning, Routine Learning.

Notes:
- The fast-path command router (UC-121/129) is a deterministic component *inside* the Voice agent;
  it keeps working when the LLM is down.
- Conversation owns short-lived dialogue sessions (UC-125, Milestone 1); the Memory agent owns the
  persistent home memory store (UC-204, Milestone 2). Milestone 1 must not build the persistent store.

## Agents
| Agent | Milestone | Type | Responsibility |
|---|---|---|---|
| Room Manager | 1 | Deterministic | Node lifecycle, device discovery (UC-107..109) |
| Presence | 1 | Deterministic | Occupancy + identity (UC-111..113) |
| Voice | 1 | AI | Wake→STT→LLM→TTS pipeline (UC-115..123) |
| Conversation | 1 | AI | Session, routing, personality (UC-124..126) |
| Automation | 2 | Deterministic | Rule engine, permissions (UC-209..211, UC-225) |
| Security | 2 | Deterministic | Modes, anomaly detection (UC-216..217) |
| Climate | 2 | Deterministic | HVAC per room (UC-213) |
| Memory | 2 | Deterministic | Home memory store (UC-204) |
| Notification | 2 | Deterministic | Prioritized alerts (UC-220) |
| Proactive | 4 | AI | Surface useful info (UC-409) |
| Reasoning | 4 | AI | Multi-signal analysis (UC-410) |
| Routine Learning | 2 | AI | Pattern detection → suggested automations (UC-211) |

## Event Bus
All agents communicate via the central bus (`homeai.HomeEvent`). No direct agent-to-agent calls.

## Agent Priority
1. Security  2. Automation  3. Climate  4. Voice  5. Proactive  6. Reasoning  7. Routine Learning

## Failure Isolation
All agents run inside the single `homeai-core` process as independent supervised tokio tasks with
panic isolation (techstack §1). If one task panics, others continue; the supervisor restarts it
with exponential backoff and alerts on repeated crash loops. Deterministic agents have highest
restart priority.

## LLM Context Window (assembled by Voice Agent)
System prompt (~2K) + room (~500) + person (~200) + home state (~500) + recent events (~1K) +
conversation history (~4K) + memories (~500) ≈ 8K tokens. Pre-warmed KV cache per UC-122.