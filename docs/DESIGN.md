# Pixel Connect — Design System (DESIGN.md)

The single reference for how the app looks, moves, and stays legible. If a
change contradicts this file, the file wins until it is deliberately updated.

Stack: Vue 3 + Tailwind (tokens, not palette classes) + CSS custom properties.
Source of truth for values: `app/main/src/tokens.ts` (themes, accents, motion,
type, shape, spacing) mirrored as `--pc-*` properties in
`app/main/src/assets/main.css`. `tailwind.config.cjs` repoints legacy
`ink/paper` classes at the same vars so old markup keeps working.

## 1. Principles

1. **Tonal hierarchy, not shadows.** Depth comes from surface-container tone
   steps plus a 1px outline-variant border where tone alone fails (AMOLED).
   No glassmorphism, neon, heavy gradients, or decorative-only animation.
2. **One truth per state.** Connection, transfer and device states each have a
   single source (the backend record); every surface reads it, none invents it.
   Features that are not implemented stay labelled, never faked.
3. **Motion means something.** Springs for space, short tweens for effects.
   Every animation has a reduced-motion path.
4. **Contrast is enforced, not hoped for.** 48 theme×accent combinations are
   machine-checked (`scripts/contrast-check.mjs`, 952 assertions, AA: 4.5:1
   text, 3:1 UI). The build fails on regression.

## 2. Typography

- **Google Sans Flex only** (bundled variable woff2, wght 1–1000, OFL in
  `app/main/licenses/`). **Never italic** — `font-synthesis: none`, and form
  controls (`button, input, textarea, select, dialog, [role=tooltip]`) set the
  family explicitly because they do not inherit it.
- Scale: Display 36/44 · Headline 28/36 · Title 20/28 (600) · Body L 16/24 ·
  Body M 14/20 · Label 12/16 (500, +0.4 tracking). Body never below 14px;
  supporting text never below 13px and always ≥4.5:1.
- Wordmark: Google Sans Flex **weight 1000**, normal style, −0.01em tracking,
  theme text colour (`LogoLockup.vue`).
- Monospace (`--pc-font-mono`) only for paths and commands (`pc-code`,
  `pc-pre`, `PathChip`).

## 3. Color: themes × accents

Selected by `data-theme` / `data-accent` on `<html>`; `:root` is the dark
default so a missing attribute still renders. Colour tokens are
`@property`-registered and cross-fade ~300ms on switch.

**Themes** (`data-theme`): `amoled` (#000 base) · `space-dark` (#0A0C16 + 6%
accent radial glow, off under reduced motion) · `simple-dark` (#131313) ·
`simple-light` (#F5F5F3) · `white-light` (#FFFFFF) · `system` (live
`prefers-color-scheme`; dark→simple-dark, light→simple-light;
`prefers-contrast: more` raises outlines/secondary). Legacy values
`dark/light/midnight/contrast-dark/contrast-light` alias to the above so
stored settings keep working.

**Accents** (`data-accent`, seeds): Purple `#9A7BFF` · Blue `#4C8DF6` · Pink
`#F0629F` · Green `#3DDC84` (default) · Yellow `#F9C440` · Brown `#A98274` ·
Gray `#9AA0A6` · White (inverse: white-on-dark, near-black-on-light). Dark
themes: primary tone 80, containers tone 30. Light: primary tone 40 (36 for
green/yellow to hold 3:1), containers tone 90. Accent colors primary buttons,
nav indicator, switches, selected chips, progress and focus rings — never
large surfaces beyond the subtle surface-tint.

**Roles** (per theme×accent): `primary / on-primary / primary-container /
on-primary-container / secondary-container / surface-tint` + state-layer
colors + `color-scheme` + matching scrollbars.

## 4. Shape, spacing, elevation

- Shape: cards 24px · inner tiles 16px · buttons/chips fully rounded · text
  fields 16px · logo tile rx 26/108.
- Spacing: 4px grid (4/8/12/16/24/32/48). Content max-width ~880px centered,
  32px side padding. List rows ≥64px. Targets ≥40px.
- Elevation: surface tones `low / default / high / highest`; 1px
  outline-variant border only where tone is insufficient.

## 5. Icons and logo

- One set: bundled SVG paths (`PcIcon.vue`), 24px optical, weight-400 style.
  Nav icons fill when selected.
- Logo: white gradient tile (`#FFFFFF→#E8E8E8`, diagonal) + monochrome ribbon
  mark (gradient `#808080→#303030→#000000`, 5-unit crossover gap, clearspace
  2× ribbon). One `#pc-logo` symbol sprite (`LogoSprite.vue`, mounted once in
  `App.vue`) referenced with `<use>` (`LogoLockup.vue`, 40px sidebar tile,
  identical in every theme; hairline ring in light themes only). Tray: flat
  `currentColor` mark (`#pc-mark-flat`), white/black at 16/22/24. Geometry is
  never redrawn; flat variants below 64px. Rasters in `data/branding/`.

## 6. Components (`components/atoms/`)

`Btn` (filled/tonal/outlined/text + error shake; ripple from pointer origin;
~0.97 press; 8/10/10% state layers) · `FAB` pattern (extended, primary — the
sidebar Share File is the only filled-primary) · `Chip` (animated check) ·
`Switch` (growing thumb, check/cross, spring; `ToggleSwitch` re-exports it) ·
`Segmented` (sliding pill, arrow keys) · `TextField` (filled, floating label,
error state) · `SelectMenu` (scales from anchor, Esc) · `Stepper` ·
`CheckBox` (incl. mixed) · `Card` (tonal, hover raises tone) · `ListItem`
(leading icon/title/supporting/trailing) · `SnackBar` (spring-up, Undo slot) ·
`Dialog` (focus trap + restore, Esc) · `Disclosure` (animated height+fade,
e.g. "Why?") · `WavyProgress` (flat offline / gentle connected / moving in
transfer / settles flat on done; determinate + speed/ETA slots) · `StatusDot`
(word + colour, pulsing ring only while searching) · `Badge` (roll/pop on
change) · `NavItem` (48px, sliding pill) · `PathChip` (monospace + copy) ·
`EmptyState` (logo tile default) · `Skeleton` (transform-only shimmer) ·
`ToastMessage` · `ProgressWave` · `LogoSprite/LogoLockup` · `BrandMark`
(legacy) · `AppIcon` (legacy).

## 7. Screens

- **App shell**: native OS frame (no custom title bar). Sidebar ≥1000px
  (280px: lockup, device status card → Devices, nav with sliding pill,
  Share-File FAB, quiet Help/Exit). <1000px: 80px rail + modal drawer. Usable
  to ~720px.
- **Files**: connection hero (offline: illustration + Reconnect primary +
  Troubleshoot secondary; connected: name + battery ring + method chip +
  Ping/Find/Mirror) → real wavy indicator → Send Text (filled area, count,
  disabled-until-content) + Send File (dropzone with animated drag-over,
  file chip, KDE send + "Share via Quick Share…" → `QuickShareSendDialog`
  with live discovery, Rescan, transport labels) → Recent transfers (state
  rows with Retry/Open/Show-in-folder/Copy-path, real empty state).
- **Devices**: rich paired cards (status pill, type/method/battery/last-seen
  chips; Reconnect only when down; Ping/Unpair/Mirror/Menu) → merged ADB/scrcpy
  card (numbered stepper, tool chips with short versions, per-row Open/Stop/
  Disconnect rules) → transports (Available/Unverified/Not-implemented chips,
  long reasons behind Why? disclosures).
- **Notifications**: title + count + Clear-all (Undo snackbar); grouped by
  device (live link dot) then app (sticky headers); cards show app initial
  fallback, title→text→appName heading, body ≤3 lines, time, real actions +
  Dismiss; reply collapsed until tapped; spring-in, 250ms slide-out + Undo;
  "You're all caught up" empty state.
- **Settings**: sticky scroll-spy chip bar (opaque, track-on-scroll, arrow
  keys) → Appearance first (live mini-app previews, 8 swatches with morphing
  check, Reduce-animations switch) → Network / Preferences / Screen mirroring
  / Integrations / About as ≥64px list rows in rounded cards. No numeral
  pills. Identity is always-editable with inline save confirmation. SSH under
  Advanced with its self-configured gate.

## 8. Motion

Tokens: emphasized `(0.2,0,0,1)` · decelerate `(0.05,0.7,0.1,1)` · accelerate
`(0.3,0,0.8,0.15)` · 100/200/300/500ms · springs (~380/30 spatial, damped
effects). Page transitions: shared-axis-Y fade-through (24–30px + fade,
~300ms), first-8 stagger 30ms. Theme switch: View-Transitions circular reveal
from the swatch, 250ms cross-fade fallback. Animate `transform`/`opacity`
(+ registered colours) only. **Reduce path**: `prefers-reduced-motion` and
in-app Reduce-animations (`data-reduce-motion`) both kill springs/glows/
translations (fades ≤100ms).

## 9. Accessibility

Full keyboard (2px accent focus rings +2px offset, tab order, arrows in nav +
segmented, Esc closes), correct ARIA roles + live regions (progress, arrivals,
toasts), ≥40px targets, ≥13px supporting text at AA, responsive to 720px and
125/150% scaling. Every screen has loading/empty/offline/error states. No
truncation hides essentials.

## 10. Verification ritual (for future changes)

1. `npm run ts-check`, `npm test`, `npm run vite:build`, `npm run contrast`.
2. Screenshots: every screen × 6 themes (one accent), Files + Settings × 2
   themes × 8 accents.
3. Keyboard-only pass; 720/1440 widths; 150% scaling.
4. No backend/IPC change may ride along with a UI change — and no UI change
   may fake a state the backend has not produced.
