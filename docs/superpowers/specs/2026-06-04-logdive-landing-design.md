# logdive Landing Page — Design Spec

**Date:** 2026-06-04  
**Status:** Approved  
**Author:** Arya Gorjipour (via Claude Code brainstorming)

---

## 1. Goal

Build a production-quality SPA landing page for the logdive project, deployable to `https://aryagorjipour.github.io/logdive/`. Pixel-faithful to the Claude Design handoff (`LogDive Landing-handoff.zip`), with all content updated to v0.2.1 and docs strictly accurate to the real README/CHANGELOG.

---

## 2. Repository Location

**`landing/` subdirectory** inside the existing `Aryagorjipour/logdive` repo.

- Astro `base: '/logdive'` config handles the path prefix.
- GitHub Actions workflow at `.github/workflows/landing.yml` triggers on `push` to `main` when `landing/**` changes.
- Deploys `landing/dist/` to the `gh-pages` branch.

---

## 3. Tech Stack

| Concern | Choice | Rationale |
|---|---|---|
| Framework | Astro 5 | Static output, island architecture, no framework overhead |
| Language | TypeScript | Type-safe data, component props |
| Styling | Vanilla CSS (custom properties) | Preserve handoff design system exactly |
| Interactive bits | Astro `<script>` islands | ThemeToggle, CodeBlock, Tabs, mobile nav — no React/Vue |
| Content | Inline Astro + `src/data/*.ts` | Docs in page files; roadmap/version as typed TS data |
| Fonts | Google Fonts (IBM Plex Sans + JetBrains Mono) | Matches handoff exactly |
| Deployment | `withastro/action@v3` + `gh-pages` branch | Standard Astro GH Pages pattern |

---

## 4. Directory Structure

```
landing/
  astro.config.ts
  package.json
  tsconfig.json
  src/
    data/
      roadmap.ts          # typed roadmap data + CURRENT_VERSION constant
    layouts/
      Base.astro           # <html>, <head>, theme pre-paint script, font import
    components/
      Header.astro         # sticky nav, theme toggle, mobile menu
      Footer.astro         # logo, license, version badge, links
      CodeBlock.astro      # <pre> + copy-to-clipboard island
      Tabs.astro           # installation method tabs island
      ThemeToggle.astro    # localStorage theme toggle island
      RoadmapStatus.astro  # renders Now/Next/Later lanes from data/roadmap.ts
      TerminalPreview.astro # query language demo terminal
    pages/
      index.astro          # → /logdive/
      docs.astro           # → /logdive/docs
      about.astro          # → /logdive/about
    styles/
      tokens.css           # CSS custom properties (design tokens)
      global.css           # reset, base, typography
      components.css       # header, footer, buttons, code, terminal, stat grid, etc.
  public/
    (no binary assets; favicon is inline SVG data URI)

# GitHub Actions workflow lives at REPO ROOT (not inside landing/):
# .github/workflows/landing.yml
```

---

## 5. Design System

Preserved exactly from handoff. No deviations.

**Palette (light mode):**
- `--bg: #FBF9F6`
- `--surface: #FFFFFF`
- `--border: #E8E3DC`
- `--text: #0E1419`
- `--text-muted: #5B5A57`
- `--brand: #0A2540`
- `--accent: #B85D44` (terracotta — CTAs, logo dot, active states, accent highlights)

**Dark mode** via `[data-theme="dark"]` and `@media (prefers-color-scheme: dark)`:
- `--bg: #0B1014`, `--surface: #141A20`, `--accent: #D9714E`

**Typography:**
- `--font-sans: 'IBM Plex Sans'` — UI/body
- `--font-mono: 'JetBrains Mono'` — code, eyebrows, tabs

**Spacing:** 4px base unit scale (`--space-1` through `--space-32`).

---

## 6. Pages

### 6.1 `index.astro` — Home (`/logdive/`)

Seven sections in order:

1. **Hero** — Tagline "jq with memory.", sub-copy, `cargo install logdive` install bar with copy button, GitHub ghost button. Eyebrow: `v0.2.1 · MIT OR Apache-2.0`. Horizontal rule background lines decoration.

2. **Query language** — Terminal preview component showing 3 example queries with inline notes. Queries from README.

3. **Performance** — 4-card stat grid: `210k lines/s`, `166k lines/s`, `17µs`, `3.6ms`. Numbers from criterion benchmarks in README.

4. **Pillars** — 3-card grid: Local-first, Fast queries, Multi-format ingestion. SVG icons from handoff.

5. **Architecture** — Code block showing 3-crate workspace tree.

6. **Comparison** — Prose card: when to use logdive vs. Loki/Datadog/Elastic. "Honest limit" callout.

7. **Project status** — `RoadmapStatus` component: Now/Next/Later lanes rendered from `data/roadmap.ts` at build time. "Recently shipped" `<details>` list with v0.2.1, v0.2.0, v0.1.0.

8. **Installation** — `Tabs` component: cargo / docker / source tabs with `CodeBlock` in each panel.

### 6.2 `docs.astro` — Documentation (`/logdive/docs`)

Sticky sidebar nav + main content. Sidebar scroll-spy highlights active section via `IntersectionObserver`.

**Sidebar sections:**
- Get started: Quick start, Installation
- The CLI: ingest, query, stats, prune
- The HTTP API: /query, /stats, /version
- Reference: Query language, Configuration, Docker, Architecture

**Content accuracy:** Strictly README v0.2.1. Only real flags/env vars documented. Incorrect handoff content (--batch, --dedupe, --source, --reverse, --explain, logdive.toml) is replaced with accurate content.

Key corrections:
- `logdive ingest` flags: `--file`, `--format json|logfmt|plain`, `--tag`, `--timestamp-now`, `--follow`, `--db`
- `logdive query` flags: `--format pretty|json`, `--limit`, `--db`
- `logdive prune` flags: `--older-than`, `--before`, `--yes`, `--db`
- `logdive-api` flags: `--db`, `--port`, `--host`, `--cors-origins` (not `--bind`)
- Configuration: `LOGDIVE_DB`, `LOGDIVE_LOG`, `LOGDIVE_API_PORT`, `LOGDIVE_API_HOST`, `LOGDIVE_API_CORS_ORIGINS`, `NO_COLOR`, `HOME`
- Query language grammar: AND/OR, no parens (parens are v0.3 non-goal)
- Docker: real image tag `ghcr.io/aryagorjipour/logdive:0.2.1`

### 6.3 `about.astro` — About (`/logdive/about`)

Two-column layout: prose (why logdive exists) + non-goals aside. "Built by" section. Content matches handoff exactly — it's accurate.

---

## 7. Data Files

### `src/data/roadmap.ts`

```typescript
// Type definitions — actual data values defined in implementation
export const CURRENT_VERSION = '0.2.1';
export const IN_PROGRESS_VERSION = '0.3.0';

export interface RoadmapItem {
  title: string;
  kind: 'feature' | 'perf' | 'infra' | 'docs';
  issue?: number;
  description?: string;
  note?: string;
  versionTarget?: string;
}

export interface ShippedRelease {
  version: string;
  date: string;
  highlights: string[];
}

export const roadmap: {
  updated: string;
  lanes: { now: RoadmapItem[]; next: RoadmapItem[]; later: RoadmapItem[] };
  shipped: ShippedRelease[];
} = { /* populated during implementation */ };
```

**Now (v0.3.0):**
- Parenthesised query expressions (`(level=error OR level=warn) AND service=payments`)
- Distroless Docker image
- Generated columns

**Next:**
- Windows `--follow` support (NTFS rotation detection)
- yaml/csv output formats
- Per-source configurable retention (`prune --older-than` per source)

**Later (no timeline):**
- HTTP API authentication
- Multi-file glob ingest
- Aggregations: count, distinct, group-by
- Browser-based query UI (explicit v1 non-goal, listed for completeness)

**Shipped:**
- v0.2.1 (2026-06-01): Security tests, functional tests (proptest, concurrent ingest, boundary prune, API integration), supply-chain hardening (cargo-deny, SBOM, audit CI), allocation improvements
- v0.2.0 (2026-05-15): OR query language, logfmt/plain ingestion, --follow mode, prune subcommand, /version + CORS on API, multi-arch Docker
- v0.1.0 (2026-04-19): Initial release — ingest, query, stats, SQLite, blake3 dedup, HTTP API

---

## 8. Interactive Components

| Component | Island type | Behavior |
|---|---|---|
| `ThemeToggle` | `<script>` | Reads `localStorage['logdive-theme']`, toggles `[data-theme]` on `<html>`, persists |
| `CodeBlock` | `<script>` | `navigator.clipboard.writeText` + `execCommand` fallback; "COPIED" feedback |
| `Tabs` | `<script>` | `.is-active` class on button + panel pairs |
| Mobile nav | `<script>` | `.is-open` toggle on `.nav` |
| Docs scroll-spy | `<script>` | `IntersectionObserver` on section headings → highlight active sidebar link |
| `RoadmapStatus` | **None** | Rendered at build time from `data/roadmap.ts`; no client JS |

All scripts use Astro's native `<script>` tag (auto-bundled, deferred). No external JS libraries.

---

## 9. GitHub Actions Workflow

**File:** `.github/workflows/landing.yml`

```yaml
name: Deploy landing page
on:
  push:
    branches: [main]
    paths: ['landing/**']
permissions:
  contents: write
  pages: write
  id-token: write
jobs:
  deploy:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: withastro/action@v3
        with:
          path: landing
          node-version: 22
```

Pushes built `dist/` to `gh-pages` branch. Repository GitHub Pages setting: source = `gh-pages` branch, root `/`.

---

## 10. Accessibility + Performance Targets

- Semantic HTML: `<header>`, `<nav>`, `<main>`, `<footer>`, `<article>`, `<aside>`, `aria-label` on nav regions
- `prefers-reduced-motion`: all animations/transitions disabled
- `NO_COLOR` pattern honoured in code examples
- Target: Lighthouse Performance ≥ 95, Accessibility ≥ 95
- Fonts loaded with `display=swap`; theme pre-paint inline script prevents flash

---

## 11. Out of Scope

- Contact page (exists in handoff but not wired in nav; omit unless user requests)
- Search functionality
- i18n
- RSS feed
- Analytics
