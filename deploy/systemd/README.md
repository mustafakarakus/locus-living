# systemd units

Names are fixed in `docs/techstack.md` §5.

Core host: `homeai-core`, `homeai-llm`, `homeai-vision`, `homeai-stt`, `homeai-tts`.
Full node: `homeai-noded`.

All use `Restart=on-failure`, `RestartSec=2`, with start-limit so crash loops alert
instead of spinning forever (UC-101, UC-233).
