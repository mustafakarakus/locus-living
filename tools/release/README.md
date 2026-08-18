# Release / signing

Vendor-side only. Never installed on a Home Core.

Signs update bundles (Ed25519) and, later, add-on license files. The Core verifies
against `/etc/homeai/update-pubkey.pem`. Keys stay in offline / HSM storage
(`docs/techstack.md` §10).
