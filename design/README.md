# Design reference

`reference/` holds the supplied Pixel Connect design files that this project's
user interface implements. They are the acceptance reference for the front end:
layout, typography, colour, spacing, component shapes and the responsive
behaviour were taken from them.

Where a design showed a control that has no working backend in V1 (the Tailscale
network card, the dark-mode and transfer-alert switches, transfer pause/cancel),
the control is present but **disabled with an explicit explanation**, rather than
silently dropped or faked. Decisions are listed in
`docs/architecture/v1-implementation-plan.md` §3.6.

The product name shown throughout is **Pixel Connect**.
