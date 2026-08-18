# Rust crates

Cargo workspace. Agents in `homeai-core` are modules + supervised tokio tasks, not
separate processes and not separate crates.

- `common` — paths, errors, config types. No I/O policy that belongs in core.
- `proto` — generated from `proto/homeai.proto`. Do not hand-write a parallel schema.
- `core` — `homeai-core`. Single process. Event bus in-process. See `docs/agent-architecture.md`.
- `noded` — full room node. Discovers Core via mDNS, speaks gRPC/mTLS on 50051.
- `cli` — `homeai admin`. LAN only.

Do not add cloud crates, a second database, or a second event bus.
Build Milestone 1 (`core` supervisor, bus, API, noded, cli stubs) before anything else.
