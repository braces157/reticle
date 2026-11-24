# Reticle Website Design System (`DESIGN.md`)

Aesthetic benchmark: **Linear · Raycast · CleanShot X · Leica optical hardware**.

## 1. Color & Surface Tokens
- **Canvas (`--color-void`):** `#050507` (deep obsidian void)
- **Surface 1 (`--color-surface`):** `#0B0B0F` (elevated section / card plane)
- **Surface 2 (`--color-surface2`):** `#111117` (interactive control wells)
- **Hairline Rim (`--color-rim`):** `rgba(255, 255, 255, 0.08)` (1px crisp edge)
- **Primary Ink:** `#F4F4F6` (body >= 15:1 contrast on `#050507`)
- **Secondary Ink:** `rgba(244, 244, 246, 0.62)` (>= 6.5:1 on `#050507`)
- **Muted Ink:** `rgba(244, 244, 246, 0.44)` (labels/metadata only)
- **Optical Accent (`--color-accent`):** `#FF4F30` (Leica vermilion — reserved for focal indicators, beams, and primary highlights)
- **Signal Emerald (`--color-emerald`):** `#34D399` (verified local/offline status)
- **Annotation Ink Palette (1:1 with `src/snip/annotate.rs`):** `#EF4444`, `#F59E0B`, `#22C55E`, `#38BDF8`, `#FAFAFA`

## 2. Typography Pair
- **Display & UI (`--font-sans`):** `Plus Jakarta Sans`, `-apple-system`, `Segoe UI`, sans-serif (`tracking-[-0.045em]` on display)
- **Telemetry & Shortcuts (`--font-mono`):** `JetBrains Mono`, `Cascadia Mono`, monospace (`font-variant-numeric: tabular-nums`)

## 3. Motion & Spring Budget (Emil Kowalski / Rauno Freiberg)
- **Frame-rate independent physics:** All interactive motion uses a damped harmonic `Spring` (`dt`-clamped) rather than frame-count `lerp` so 60Hz, 144Hz, and 240Hz Windows displays feel identical.
- **Tactile press:** Interactive buttons scale to `0.97` on `:active` (`150ms cubic-bezier(0.16, 1, 0.3, 1)`).
- **Never scale from 0:** Modals & popovers enter from `scale(0.95) translateY(8px)` with `180ms` out-expo easing.
- **Visibility gating & reduced motion:** Every RAF/canvas loop pauses when offscreen via `IntersectionObserver`, and `@media (prefers-reduced-motion: reduce)` disables ambient beams, 3D tilt, and particle effects.

## 4. Six Edge States
1. **Empty:** Playground initializes with an active preset snip so the stage is immediately demonstrable; clearing selection shows crosshair + coordinate loupe guidance.
2. **Loading:** Compiled local CSS eliminates CDN Tailwind FOUC; hero blur-in resolves cleanly with `clearProps`.
3. **Overflow & Density:** Tested across `1280px` desktop and `375px` mobile; long monospace paths use `truncate` / `overflow-x-auto`.
4. **Error & Offline:** Clipboard write falls back to `execCommand('copy')`; Web Audio fails silently if autoplay-blocked.
5. **Keyboard & Focus:** Visible `:focus-visible` titanium ring, `?` opens shortcuts sheet, `Escape` closes modals with focus return to trigger, `ArrowLeft`/`ArrowRight` operates compare slider.
6. **Contrast:** Adaptive `mix-blend-mode: difference` on the Pin Opacity capsule slider guarantees legibility at every value (`20%–100%`).
