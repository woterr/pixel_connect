# UI overhaul — verification report

**Status: implemented, verified as far as this machine allows.** The
presentation layer was rebuilt in Material 3 Expressive. No backend behaviour,
IPC contract, or feature logic was changed in this pass (the two functional
fixes below it were separate commits).

## 1. Audit (what was there)

- Stack: Tauri + Vue 3 + Pinia + Tailwind (`darkMode: 'class'`), no router —
  four views switched by local state in `App.vue`.
- Backend boundary: `src/api.ts` (Tauri `invoke` + event subscriptions);
  settings persist in the Tauri store via `get/update_settings`.
- Tokens: 28 `--pc-*` vars, dark default + an unreachable `light` theme (the
  selecting class was never applied — the same dead-capability shape as the
  autostart bug). No accent, no motion tokens. Tailwind `ink/paper` palette
  hardcoded, bypassing `data-theme` entirely.
- `save()` never persisted `theme` — the choice applied immediately but was
  lost on restart (fixed in commit `990b422`).

## 2. Functional fixes in this series (backend, minimal)

- **Clipboard autosend.** The frontend fallback path (only path on
  focus-only compositors) tested `if (document.hasFocus()) return` —
  inverted, so it never synced exactly when it was the only working path.
  Fixed. `pushClipboard` also dropped silently on anything but `Connected`
  while the backend watcher dials first; it now refreshes and lets the error
  surface. The backend watcher ignored `clipboardEnabled`/`clipboard_auto_share`
  entirely; it now checks both.
- **Quick Share auto-accept.** `auto_accept_shares` only affected KDE shares.
  `WaitingForUserConsent` now sends `AcceptTransfer` automatically when on.
- **Notifications crash.** The view called `n.id.split(':')` unconditionally;
  a notification without an id crashed the whole view. Null-safe now (found
  via the visual-check stub sending `key:` instead of `id:` — stub fixed too).

## 3. What was built

- **Tokens** (`tokens.ts`, single source of truth; rewritten `main.css`):
  6 themes + legacy aliases, 8 accents with per-theme tone mapping,
  `@property` animated colors, motion/type/shape/spacing scales, bundled
  Figtree, pre-paint snippet in `index.html`.
- **20 atoms** (Btn/Chip/Switch/Segmented/TextField/SelectMenu/Stepper/
  CheckBox/Card/ListItem/SnackBar/Dialog/Disclosure/WavyProgress/StatusDot/
  Badge/NavItem/PathChip/EmptyState/Skeleton). `ToggleSwitch` kept as a
  re-export wrapper.
- **4 screens** rebuilt per spec, including: connection hero with real wave
  states, numbered stepper for wireless debugging, grouped notifications with
  fallbacks, scroll-spy Settings with Appearance on top.
- **Honesty kept:** Rename/Open/Show-in-folder disabled with reasons where no
  IPC exists; Bluetooth/Wi-Fi-Direct stay "Not implemented" with Why?
  disclosures; no relative times where the type carries no timestamp.

## 4. Verification

| Check | Result |
|---|---|
| `vue-tsc --noEmit` | clean |
| `npm test` | 29/29 |
| `vite:build` | succeeds |
| `contrast-check` (48 combos, 952 assertions) | pass, thinnest +0.19 (simple-light × yellow UI 3.19:1) |
| `cargo check` / release build | clean; installed; backend ready, BLE advertising, autostart registered |
| Screenshots, 6 themes × green | all legible (captured) |
| Screenshots, amoled × 8 accents | accent visibly changes (FAB/nav/selection); "Currently Green" label correctly reflects stored (not previewed) value |
| simple-light × 8 accents | distinct computed primaries verified |
| Files/Devices/Notifications/Settings | captured; Devices rich cards + stepper, Notifications title/body/time/Dismiss/Clear-all, Files hero + send cards |
| Backend untouched by UI pass | `git status` shows no `src-tauri/` or `core/` modifications |

## 5. Not verified (stated plainly)

- **Keyboard-only walkthrough** — focus order, Esc, arrow keys, drawer focus.
  `:focus-visible` rings are in CSS but programmatic focus doesn't trigger
  them, so this needs hands on keys.
- **Responsive** at 720/900/1160/1440 and 125%/150% scaling.
- **Real-device screenshots** — all captures use harness stub data
  ("Maria's Pixel", fake battery). Layout is verified, not live content.
- **Undo-after-phone-confirmed-dismiss** divergence.
- **Component unit tests** — repo has no jsdom harness; Dialog trap,
  SelectMenu keyboard, SnackBar timer, PathChip clipboard are reviewed and
  type-checked, not executed.
- **Clipboard autosend + QS auto-accept on hardware** — need the user to copy
  text and receive a share with the setting on.
- OS `prefers-color-scheme/contrast` inside the webview; `@property`/`color-mix`
  animation on the user's WebKitGTK (both degrade gracefully).
