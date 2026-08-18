# Mobile app

One Kotlin + Compose Multiplatform UI. Two store binaries.

- `composeApp/` — shared screens, viewmodels, API client
- `iosApp/` — thin shell: WireGuard Network Extension, APNs, Keychain, BLE
- `androidApp/` — thin shell: VpnService, FCM, Keystore, BLE
- Do not use React Native, Flutter, or a second SwiftUI app.
- Notifications and HIGH confirmations travel only through the phone-initiated WireGuard
  tunnel (UC-224). No FCM/APNs-to-Core callbacks. No Core outbound webhook.
- Updates: download the signed bundle on the phone, transfer over LAN/VPN, user triggers
  install (UC-234). Never auto-install.
- Photo handoff (UC-244) is a short-lived upload to the Core, not a cloud album.
- Voice remains the in-home input. This is not a full smart-home remote.
- Do not reuse these screens on the Home Tablet.
