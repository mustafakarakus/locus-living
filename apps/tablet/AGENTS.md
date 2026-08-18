# Home Tablet

Native Android kiosk. Jetpack Compose. Wall-mounted / PoE panel.

- Output: status, presence, calendar, notifications, energy (UC-223).
- Touch is for HIGH confirmations and rare exceptions.
- Do not build settings, room editors, or device managers here.
- No webview, no React, no Flutter.
- Do not reuse `apps/mobile` screens. This is an ambient kiosk, not a large phone.
- No public internet. Pin the Core TLS cert. Hard-fail on mismatch.
- Scaffold the Android project when UC-223 is in scope (Milestone 2).
