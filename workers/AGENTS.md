# AI workers

Python 3.11 façades on localhost. They wrap a primary engine and a fallback.
They are not a second brain.

- `stt/` binds `127.0.0.1:8100`
- `tts/` binds `127.0.0.1:8300`
- LLM (`:8200`) and vision (`:8250`) are llama-server, not these packages
- No outbound network. No extra ports. Language priority: TR + EN required, NL plus, AR later
- Implement with the Voice pipeline (UC-117, UC-119), not before the Core can call them
