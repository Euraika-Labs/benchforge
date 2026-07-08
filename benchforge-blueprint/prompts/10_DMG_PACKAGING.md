# Codex prompt: DMG packaging

Prepare macOS distribution.

Scope:

- configure Tauri product metadata;
- add app icon placeholders;
- add `scripts/package-dmg-macos.sh`;
- document code-signing and notarization placeholders;
- fix GUI PATH issue for CLI discovery;
- add first-run Doctor screen.

Acceptance:

- on macOS, `npm run tauri:build:dmg` produces a DMG;
- app can find CLI tools installed through common shells or configured absolute paths.
