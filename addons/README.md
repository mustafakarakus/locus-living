# Add-ons

Milestone 4. Optional domain bundles (pet, elderly, child safety, …).

- Purchase may happen on `apps/web`
- Delivery is a signed offline file, same path as software updates (UC-401, UC-234)
- Activation works with WAN disconnected
- Removing an add-on must leave Core features intact
- Core intelligence is never paywalled (UC-235)

Do not add add-on crates under `crates/core`. Each add-on will live here as its own
package and compile into a bundle `tools/release` signs.
