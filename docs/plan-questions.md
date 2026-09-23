# Plan questions

Ambiguities, contradictions or impossibilities found in PLAN.md, with the
provisional choice made. Code affected is marked `// PQ-nnn`.

## PQ-001 — Tailwind `darkMode: 'class'` under Tailwind v4 (open)

- **Plan:** Tech Stack (PLAN.md 1604): "shadcn-svelte, dark mode only (Tailwind
  `darkMode: 'class'` with `dark` always on the root)".
- **Issue:** `darkMode: 'class'` is Tailwind v3 config syntax. Current
  shadcn-svelte (1.x) requires Tailwind v4, which has no `darkMode` key; the v4
  equivalent is `@custom-variant dark (&:where(.dark, .dark *));` in CSS.
- **Options:** (a) Tailwind v4 + current shadcn-svelte with the class-based
  custom variant; (b) Tailwind v3 + the legacy shadcn-svelte 0.x line.
- **Provisional choice:** (a). Behaviour is identical (class-based dark mode,
  `class="dark"` always on `<html>`). Marked in `frontend/src/app.css`.
