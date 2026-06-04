# logdive Landing Page Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a production-quality Astro 5 SPA landing page in `landing/` subdirectory, deployable to `https://aryagorjipour.github.io/logdive/`, pixel-faithful to the Claude Design handoff with all content updated to v0.2.1.

**Architecture:** Astro 5 static site in `landing/` with `base: '/logdive'`. Pages (`index`, `docs`, `about`) share a `Base.astro` layout. Interactive bits (theme toggle, copy buttons, tabs, mobile nav, docs scroll-spy) are vanilla TypeScript Astro `<script>` islands. Roadmap and version data live in `src/data/roadmap.ts` — rendered at build time, zero client JS for the status section.

**Tech Stack:** Astro 5, TypeScript, Bun (package manager + runtime), Vanilla CSS (custom properties), IBM Plex Sans + JetBrains Mono (Google Fonts), GitHub Actions (`oven-sh/setup-bun@v2`) → `gh-pages` branch deploy.

**Branch:** `feat/landing-page`

**Spec:** `docs/superpowers/specs/2026-06-04-logdive-landing-design.md`

---

## File Map

```
landing/
  astro.config.ts
  package.json
  tsconfig.json
  bun.lockb                          (auto-generated)
  src/
    data/
      roadmap.ts                     version constants + typed roadmap data
    layouts/
      Base.astro                     <html>, <head>, theme pre-paint, fonts, global CSS
    components/
      Header.astro                   sticky nav, logo, mobile toggle, theme toggle
      Footer.astro                   logo, license, version badge, footer links
      CodeBlock.astro                <pre> wrapper + copy-to-clipboard <script>
      Tabs.astro                     tab buttons + panels + <script>
      TerminalPreview.astro          query demo terminal block (static)
      RoadmapStatus.astro            Now/Next/Later lanes rendered at build time
    pages/
      index.astro                    home page (7 sections)
      docs.astro                     docs page (sidebar + main content)
      about.astro                    about page
    styles/
      tokens.css                     CSS custom properties, spacing scale
      global.css                     reset, base, typography, layout helpers
      components.css                 header, footer, buttons, code, terminal, stat grid, pillars, tabs, compare, status, docs
.github/
  workflows/
    landing.yml                      build + deploy on push to main, paths: landing/**
```

---

## Task 1: Scaffold Astro project with Bun

**Files:**
- Create: `landing/package.json`
- Create: `landing/astro.config.ts`
- Create: `landing/tsconfig.json`

- [ ] **Step 1: Initialise Astro with Bun**

```bash
cd /home/arysmart/Projects/Rust/logdive
mkdir landing && cd landing
bun create astro@latest . --template minimal --typescript strict --no-git --install
```

Expected output: Astro scaffold in `landing/`, `bun.lockb` created.

- [ ] **Step 2: Replace `astro.config.ts` with correct config**

```typescript
// landing/astro.config.ts
import { defineConfig } from 'astro/config';

export default defineConfig({
  site: 'https://aryagorjipour.github.io',
  base: '/logdive',
  output: 'static',
  build: {
    assets: '_assets',
  },
});
```

- [ ] **Step 3: Replace `tsconfig.json`**

```json
{
  "extends": "astro/tsconfigs/strict",
  "compilerOptions": {
    "baseUrl": ".",
    "paths": {
      "@data/*": ["src/data/*"],
      "@components/*": ["src/components/*"],
      "@layouts/*": ["src/layouts/*"]
    }
  }
}
```

- [ ] **Step 4: Verify build runs clean**

```bash
cd landing
bun run build
```

Expected: `dist/` created, no errors.

- [ ] **Step 5: Commit scaffold**

```bash
cd /home/arysmart/Projects/Rust/logdive
git add landing/
git commit -m "feat(landing): scaffold Astro 5 project with Bun"
```

---

## Task 2: CSS design system

**Files:**
- Create: `landing/src/styles/tokens.css`
- Create: `landing/src/styles/global.css`
- Create: `landing/src/styles/components.css`

- [ ] **Step 1: Create `tokens.css`**

```css
/* landing/src/styles/tokens.css */
@import url('https://fonts.googleapis.com/css2?family=IBM+Plex+Sans:wght@400;500;600;700&family=JetBrains+Mono:wght@400;500;600&display=swap');

:root {
  --bg: #FBF9F6;
  --surface: #FFFFFF;
  --surface-hover: #F4F1EA;
  --border: #E8E3DC;
  --text: #0E1419;
  --text-muted: #5B5A57;
  --brand: #0A2540;
  --accent: #B85D44;
  --success: #1F7A3A;
  --warn: #A0660A;
  --error: #A02818;

  --radius-sm: 6px;
  --radius-md: 8px;
  --radius-lg: 12px;

  --space-1: 4px;
  --space-2: 8px;
  --space-3: 12px;
  --space-4: 16px;
  --space-6: 24px;
  --space-8: 32px;
  --space-12: 48px;
  --space-16: 64px;
  --space-24: 96px;
  --space-32: 128px;

  --shadow-card: 0 1px 2px rgba(14,20,25,0.04), 0 4px 12px rgba(14,20,25,0.04);
  --ease: cubic-bezier(0.32, 0.72, 0, 1);

  --font-sans: 'IBM Plex Sans', sans-serif;
  --font-mono: 'JetBrains Mono', monospace;

  --content-width: 1120px;
}

@media (prefers-color-scheme: dark) {
  :root:not([data-theme="light"]) {
    --bg: #0B1014;
    --surface: #141A20;
    --surface-hover: #1B232B;
    --border: #1F2730;
    --text: #E8E4DC;
    --text-muted: #8B8D90;
    --brand: #E8E4DC;
    --accent: #D9714E;
    --success: #3FC771;
    --warn: #D49B36;
    --error: #E66956;
    --shadow-card: 0 1px 2px rgba(0,0,0,0.4), 0 4px 12px rgba(0,0,0,0.3);
  }
}

[data-theme="dark"] {
  --bg: #0B1014;
  --surface: #141A20;
  --surface-hover: #1B232B;
  --border: #1F2730;
  --text: #E8E4DC;
  --text-muted: #8B8D90;
  --brand: #E8E4DC;
  --accent: #D9714E;
  --success: #3FC771;
  --warn: #D49B36;
  --error: #E66956;
  --shadow-card: 0 1px 2px rgba(0,0,0,0.4), 0 4px 12px rgba(0,0,0,0.3);
}
```

- [ ] **Step 2: Create `global.css`**

```css
/* landing/src/styles/global.css */
*, *::before, *::after { box-sizing: border-box; }

html { scroll-behavior: smooth; }

body {
  margin: 0;
  background: var(--bg);
  color: var(--text);
  font-family: var(--font-sans);
  font-weight: 400;
  font-size: 16px;
  line-height: 1.5;
  -webkit-font-smoothing: antialiased;
  -moz-osx-font-smoothing: grayscale;
  text-rendering: optimizeLegibility;
}

a {
  color: var(--text);
  text-decoration: none;
  transition: opacity 200ms var(--ease);
}
a:hover { opacity: 0.7; }
a.link-underline { border-bottom: 1px solid var(--border); }
a.link-underline:hover { border-bottom-color: var(--accent); opacity: 1; }

p { margin: 0 0 var(--space-4) 0; text-wrap: pretty; }
p:last-child { margin-bottom: 0; }

h1, h2, h3, h4 {
  font-family: var(--font-sans);
  font-weight: 600;
  letter-spacing: -0.02em;
  margin: 0;
  color: var(--text);
  text-wrap: balance;
}

h1 { font-size: 64px; line-height: 1.05; }
h2 { font-size: 36px; line-height: 1.15; }
h3 { font-size: 20px; line-height: 1.3; letter-spacing: -0.01em; }

@media (max-width: 720px) {
  h1 { font-size: 44px; }
  h2 { font-size: 28px; }
}

code, pre, .mono { font-family: var(--font-mono); font-feature-settings: "calt" 1; }

hr { border: 0; border-top: 1px solid var(--border); margin: 0; }
button { font-family: inherit; }

.wrap {
  max-width: var(--content-width);
  margin: 0 auto;
  padding: 0 var(--space-6);
}

section { padding: var(--space-24) 0; }
section.tight { padding: var(--space-16) 0; }

@media (max-width: 720px) {
  section { padding: var(--space-16) 0; }
  section.tight { padding: var(--space-12) 0; }
}

.eyebrow {
  font-family: var(--font-mono);
  font-size: 12px;
  font-weight: 500;
  text-transform: uppercase;
  letter-spacing: 0.08em;
  color: var(--text-muted);
  margin-bottom: var(--space-4);
  display: block;
}

.muted { color: var(--text-muted); }
.accent { color: var(--accent); }

.section-head { margin-bottom: var(--space-12); max-width: 56ch; }
.section-head h2 { margin-bottom: var(--space-3); }
.section-head p { color: var(--text-muted); font-size: 16px; }

@keyframes om-rise {
  from { opacity: 0; transform: translateY(8px); }
  to   { opacity: 1; transform: translateY(0); }
}
main > section,
.docs > * {
  animation: om-rise 400ms var(--ease) both;
}
main > section:nth-child(1), .docs > *:nth-child(1) { animation-delay: 0ms; }
main > section:nth-child(2), .docs > *:nth-child(2) { animation-delay: 80ms; }
main > section:nth-child(3) { animation-delay: 160ms; }
main > section:nth-child(4) { animation-delay: 240ms; }
main > section:nth-child(5) { animation-delay: 320ms; }
main > section:nth-child(n+6) { animation-delay: 320ms; }

@media (prefers-reduced-motion: reduce) {
  *, *::before, *::after {
    animation: none !important;
    transition: none !important;
    scroll-behavior: auto !important;
  }
}
```

- [ ] **Step 3: Create `components.css`**

```css
/* landing/src/styles/components.css */

/* --- header --- */
.site-header {
  position: sticky; top: 0; z-index: 50;
  height: 64px;
  background: var(--surface);
  border-bottom: 1px solid var(--border);
  backdrop-filter: saturate(140%) blur(8px);
}
.site-header .wrap {
  height: 100%; display: flex; align-items: center; gap: var(--space-8);
}
.logo { display: flex; align-items: center; gap: var(--space-3); color: var(--text); }
.logo svg { display: block; flex: none; }
.wordmark {
  font-family: var(--font-sans); font-weight: 600;
  font-size: 20px; letter-spacing: -0.02em; color: var(--text);
}
.nav { margin-left: auto; display: flex; align-items: center; gap: var(--space-6); }
.nav a { font-size: 14px; font-weight: 500; color: var(--text-muted); }
.nav a:hover { color: var(--text); opacity: 1; }
.nav a.is-active { color: var(--text); }
.icon-btn {
  appearance: none; background: transparent;
  border: 1px solid var(--border); color: var(--text-muted);
  width: 36px; height: 36px; border-radius: var(--radius-sm);
  display: inline-flex; align-items: center; justify-content: center;
  cursor: pointer;
  transition: opacity 200ms var(--ease), border-color 200ms var(--ease);
}
.icon-btn:hover { color: var(--text); border-color: var(--text-muted); }
.icon-btn svg { width: 16px; height: 16px; }
.theme-toggle .sun { display: none; }
.theme-toggle .moon { display: block; }
[data-theme="dark"] .theme-toggle .sun { display: block; }
[data-theme="dark"] .theme-toggle .moon { display: none; }
@media (prefers-color-scheme: dark) {
  :root:not([data-theme="light"]) .theme-toggle .sun { display: block; }
  :root:not([data-theme="light"]) .theme-toggle .moon { display: none; }
}
.mobile-toggle { display: none; }
@media (max-width: 720px) {
  .nav {
    display: none; position: absolute; top: 64px; left: 0; right: 0;
    background: var(--surface); border-bottom: 1px solid var(--border);
    flex-direction: column; align-items: stretch; gap: 0; padding: var(--space-2) 0;
  }
  .nav.is-open { display: flex; }
  .nav a { padding: var(--space-3) var(--space-6); border-bottom: 1px solid var(--border); }
  .nav a:last-child { border-bottom: 0; }
  .mobile-toggle { display: inline-flex; margin-left: auto; }
  .theme-toggle { margin-left: var(--space-2); }
}

/* --- buttons --- */
.btn {
  display: inline-flex; align-items: center; justify-content: center;
  gap: var(--space-2); height: 44px; padding: 0 var(--space-6);
  border-radius: var(--radius-sm); font-family: var(--font-sans);
  font-size: 14px; font-weight: 500; letter-spacing: -0.005em;
  cursor: pointer; border: 1px solid transparent;
  transition: opacity 200ms var(--ease), border-color 200ms var(--ease);
  white-space: nowrap;
}
.btn-primary { background: var(--accent); color: #FFFFFF; }
.btn-primary:hover { opacity: 0.9; }
.btn-ghost { background: transparent; border-color: var(--border); color: var(--text); }
.btn-ghost:hover { border-color: var(--text-muted); opacity: 1; }

/* --- hero --- */
.hero { position: relative; padding-top: var(--space-24); padding-bottom: var(--space-24); overflow: hidden; }
.hero-lines { position: absolute; inset: 0; pointer-events: none; z-index: 0; }
.hero-lines::before {
  content: "";
  position: absolute; inset: 0;
  background-image: repeating-linear-gradient(to bottom, transparent 0, transparent 79px, var(--text) 79px, var(--text) 80px);
  opacity: 0.06;
}
.hero .wrap { position: relative; z-index: 1; }
.hero h1 { max-width: 14ch; margin-bottom: var(--space-6); }
.hero-sub {
  font-size: 20px; line-height: 1.5; color: var(--text-muted);
  max-width: 56ch; margin-bottom: var(--space-8);
}
.hero-cta { display: flex; flex-wrap: wrap; align-items: center; gap: var(--space-3); }
.install-code {
  display: inline-flex; align-items: center; height: 44px;
  padding: 0 var(--space-3) 0 var(--space-4); gap: var(--space-3);
  background: var(--surface); border: 1px solid var(--border);
  border-radius: var(--radius-sm); font-family: var(--font-mono);
  font-size: 14px; color: var(--text); position: relative;
}
.install-code .prompt { color: var(--text-muted); margin-right: var(--space-2); }
.install-code .copy-btn { position: static; }

/* --- code blocks --- */
.code-block {
  position: relative; background: var(--surface);
  border: 1px solid var(--border); border-radius: var(--radius-md);
  padding: var(--space-4); font-family: var(--font-mono);
  font-size: 14px; line-height: 1.6; color: var(--text); overflow-x: auto;
}
.code-block pre {
  margin: 0; font-family: inherit; font-size: inherit;
  line-height: inherit; color: inherit; white-space: pre;
}
.code-block .prompt { color: var(--text-muted); user-select: none; }
.code-block .kw { color: var(--brand); font-weight: 500; }
[data-theme="dark"] .code-block .kw,
:root:not([data-theme="light"]) .code-block .kw { color: var(--text); }
.code-block .cm { color: var(--text-muted); }
.code-block .str { color: var(--success); }
.code-block .accent-tok { color: var(--accent); }
.copy-btn {
  position: absolute; top: var(--space-2); right: var(--space-2);
  appearance: none; background: var(--bg); border: 1px solid var(--border);
  border-radius: var(--radius-sm); color: var(--text-muted);
  font-family: var(--font-mono); font-size: 11px; letter-spacing: 0.04em;
  padding: 4px 8px; cursor: pointer;
  transition: opacity 200ms var(--ease), color 200ms var(--ease), border-color 200ms var(--ease);
  text-transform: uppercase;
}
.copy-btn:hover { color: var(--text); border-color: var(--text-muted); }
.copy-btn.is-copied { color: var(--success); border-color: var(--success); }
.code-inline {
  font-family: var(--font-mono); font-size: 0.9em;
  background: var(--surface); border: 1px solid var(--border);
  border-radius: 4px; padding: 1px 6px; color: var(--text);
}

/* --- terminal preview --- */
.terminal {
  background: var(--surface); border: 1px solid var(--border);
  border-radius: var(--radius-md); overflow: hidden; box-shadow: var(--shadow-card);
}
.terminal-head {
  padding: var(--space-3) var(--space-4); border-bottom: 1px solid var(--border);
  display: flex; align-items: center; gap: var(--space-2);
  font-family: var(--font-mono); font-size: 12px; color: var(--text-muted);
}
.terminal-dots { display: flex; gap: 6px; margin-right: var(--space-3); }
.terminal-dots span { width: 10px; height: 10px; border-radius: 50%; background: var(--border); }
.terminal-body {
  padding: var(--space-6); font-family: var(--font-mono);
  font-size: 14px; line-height: 1.7;
}
.terminal-body .note {
  color: var(--text-muted); font-size: 13px; margin-top: 2px;
  margin-bottom: var(--space-4); padding-left: 18px;
}
.terminal-body .note:last-child { margin-bottom: 0; }
.terminal-body .cmd { color: var(--text); }
.terminal-body .cmd .prompt { color: var(--text-muted); margin-right: 8px; }
.terminal-body .cmd .accent-tok { color: var(--accent); }

/* --- stat grid --- */
.stat-grid {
  display: grid; grid-template-columns: repeat(4, 1fr); gap: var(--space-4);
}
@media (max-width: 960px) { .stat-grid { grid-template-columns: repeat(2, 1fr); } }
@media (max-width: 520px) { .stat-grid { grid-template-columns: 1fr; } }
.stat-card {
  background: var(--surface); border: 1px solid var(--border);
  border-radius: var(--radius-md); padding: var(--space-6);
}
.stat-num {
  font-family: var(--font-sans); font-weight: 600; font-size: 36px;
  letter-spacing: -0.03em; line-height: 1; color: var(--text);
  margin-bottom: var(--space-3); display: flex; align-items: baseline; gap: 4px;
}
.stat-num .unit { font-size: 18px; font-weight: 500; letter-spacing: -0.01em; color: var(--text-muted); }
.stat-desc { font-size: 13px; line-height: 1.45; color: var(--text-muted); }

/* --- feature pillars --- */
.pillar-grid {
  display: grid; grid-template-columns: repeat(3, 1fr); gap: var(--space-4);
}
@media (max-width: 880px) { .pillar-grid { grid-template-columns: 1fr; } }
.pillar {
  background: var(--surface); border: 1px solid var(--border);
  border-radius: var(--radius-md); padding: var(--space-6);
}
.pillar svg { color: var(--text); margin-bottom: var(--space-4); display: block; }
.pillar h3 { margin-bottom: var(--space-2); }
.pillar p { color: var(--text-muted); font-size: 14px; line-height: 1.55; }

/* --- tabs --- */
.tabs {
  display: flex; gap: 0; border-bottom: 1px solid var(--border); margin-bottom: var(--space-4);
}
.tab-btn {
  appearance: none; background: transparent; border: 0;
  border-bottom: 1px solid transparent; margin-bottom: -1px;
  padding: var(--space-3) var(--space-4); font-family: var(--font-mono);
  font-size: 13px; color: var(--text-muted); cursor: pointer;
  transition: color 200ms var(--ease), border-color 200ms var(--ease);
}
.tab-btn:hover { color: var(--text); }
.tab-btn.is-active { color: var(--text); border-bottom-color: var(--accent); }
.tab-panel { display: none; }
.tab-panel.is-active { display: block; }

/* --- comparison --- */
.compare {
  background: var(--surface); border: 1px solid var(--border);
  border-radius: var(--radius-md); padding: var(--space-8);
}
.compare p { font-size: 16px; line-height: 1.65; }
.compare p + p { margin-top: var(--space-4); }
.compare strong { color: var(--text); font-weight: 600; }
.compare .honest {
  margin-top: var(--space-6); padding-top: var(--space-6);
  border-top: 1px solid var(--border); color: var(--text-muted); font-size: 14px;
}
.compare .honest strong { color: var(--accent); font-weight: 500; }

/* --- project status section --- */
#status > header { margin-bottom: var(--space-12); max-width: 56ch; }
#status > header h2 { margin-bottom: var(--space-3); }
#status .meta {
  font-size: 14px; color: var(--text-muted);
  display: flex; flex-wrap: wrap; align-items: center; gap: var(--space-2);
}
#status .meta .sep { opacity: 0.6; }
#status .meta a { color: var(--text-muted); border-bottom: 1px solid var(--border); padding-bottom: 1px; }
#status .meta a:hover { color: var(--text); border-bottom-color: var(--accent); opacity: 1; }

.status-lanes { display: grid; grid-template-columns: repeat(3, 1fr); gap: var(--space-8); }
@media (max-width: 900px) { .status-lanes { grid-template-columns: 1fr; } }

.status-lane {
  background: var(--surface); border: 1px solid var(--border);
  border-radius: var(--radius-md); padding: var(--space-6);
}
@media (max-width: 600px) { .status-lane { padding: var(--space-4); } }
.status-lane > h3 {
  font-size: 22px; font-weight: 600; letter-spacing: -0.02em;
  margin: 0 0 var(--space-2) 0; color: var(--text);
}
.status-lane > p.lane-desc {
  font-size: 13px; color: var(--text-muted); margin: 0 0 var(--space-6) 0; line-height: 1.5;
}
.status-lane > ul {
  list-style: none; margin: 0; padding: 0;
  display: flex; flex-direction: column; gap: var(--space-4);
}
.status-lane li {
  padding: var(--space-3); margin: calc(var(--space-3) * -1);
  border-radius: var(--radius-sm); transition: background-color 200ms var(--ease);
}
.status-lane li + li { margin-top: var(--space-1); }
.status-lane li:hover { background: var(--surface-hover); }
.status-lane li > a,
.status-lane li > .item-title {
  display: inline-block; font-size: 16px; font-weight: 500; color: var(--text);
  line-height: 1.35; letter-spacing: -0.005em;
  text-decoration: underline; text-decoration-color: transparent;
  text-decoration-thickness: 2px; text-underline-offset: 4px;
  transition: text-decoration-color 200ms var(--ease), opacity 200ms var(--ease);
}
.status-lane li > .item-title { text-decoration: none; }
.status-lane li > a:hover { text-decoration-color: var(--accent); opacity: 1; }
.status-lane li .item-meta {
  font-family: var(--font-mono); font-size: 13px; line-height: 1.4; color: var(--text-muted); margin-top: 4px;
}
.status-lane li .item-desc {
  font-size: 14px; line-height: 1.5; color: var(--text); margin: var(--space-2) 0 0 0;
  display: -webkit-box; -webkit-line-clamp: 2; -webkit-box-orient: vertical; overflow: hidden;
}
.status-lane li.lane-empty {
  font-style: italic; color: var(--text-muted); font-size: 14px; margin: 0; padding: 0;
}
.status-lane li.lane-empty:hover { background: transparent; }

.status-shipped { margin-top: var(--space-16); }
.status-shipped > h3 {
  font-size: 22px; font-weight: 600; letter-spacing: -0.02em; margin: 0 0 var(--space-2) 0; color: var(--text);
}
.status-shipped .shipped-meta { font-size: 13px; color: var(--text-muted); margin: 0 0 var(--space-6) 0; }
.status-shipped .shipped-meta a { color: var(--text-muted); border-bottom: 1px solid var(--border); }
.status-shipped .shipped-meta a:hover { color: var(--text); border-bottom-color: var(--accent); opacity: 1; }
#shipped-list { list-style: none; margin: 0; padding: 0; }
.status-shipped details { padding: var(--space-6) 0; border-bottom: 1px solid var(--border); }
#shipped-list > li:last-child details { border-bottom: 0; }
#shipped-list > li:first-child details { padding-top: 0; }
.status-shipped summary {
  list-style: none; cursor: pointer; display: flex; align-items: baseline;
  gap: var(--space-2); padding-right: 28px; position: relative; user-select: none;
  transition: opacity 200ms var(--ease);
}
.status-shipped summary::-webkit-details-marker { display: none; }
.status-shipped summary::marker { display: none; content: ""; }
.status-shipped summary:hover { opacity: 0.8; }
.status-shipped .release-version {
  font-size: 18px; font-weight: 600; letter-spacing: -0.02em; color: var(--text);
}
.status-shipped .release-date { font-size: 14px; color: var(--text-muted); }
.status-shipped summary::after {
  content: "›"; position: absolute; right: 4px; top: 50%; transform: translateY(-50%);
  font-size: 22px; line-height: 1; color: var(--text-muted); transition: transform 200ms var(--ease);
}
.status-shipped details[open] > summary::after { transform: translateY(-50%) rotate(90deg); }
.status-shipped .release-highlights {
  list-style: none; margin: var(--space-4) 0 0 0; padding: 0;
  display: flex; flex-direction: column; gap: var(--space-2);
}
.status-shipped .release-highlights li {
  position: relative; padding-left: var(--space-6);
  font-size: 15px; line-height: 1.55; color: var(--text);
}
.status-shipped .release-highlights li::before {
  content: ""; position: absolute; left: 0; top: 11px;
  width: 12px; height: 1px; background: var(--text-muted); opacity: 0.6;
}
.status-oos {
  margin-top: var(--space-12); font-size: 14px; color: var(--text-muted); max-width: 60ch;
}
.status-oos a { color: var(--text-muted); border-bottom: 1px solid var(--border); }
.status-oos a:hover { color: var(--text); border-bottom-color: var(--accent); opacity: 1; }

/* --- footer --- */
.site-footer {
  border-top: 1px solid var(--border); padding: var(--space-12) 0; background: var(--bg);
}
.site-footer .wrap {
  display: flex; flex-wrap: wrap; align-items: center; gap: var(--space-6);
  font-size: 13px; color: var(--text-muted);
}
.site-footer .logo .wordmark { font-size: 16px; }
.site-footer .footer-links { margin-left: auto; display: flex; gap: var(--space-6); }
.site-footer .footer-links a { color: var(--text-muted); }
.site-footer .footer-links a:hover { color: var(--text); opacity: 1; }
.site-footer .version {
  font-family: var(--font-mono); font-size: 12px; padding: 2px 8px;
  border: 1px solid var(--border); border-radius: 999px; color: var(--text-muted);
}

/* --- about page --- */
.two-col { display: grid; grid-template-columns: 3fr 2fr; gap: var(--space-16); align-items: start; }
@media (max-width: 900px) { .two-col { grid-template-columns: 1fr; gap: var(--space-12); } }
.prose p { font-size: 17px; line-height: 1.65; color: var(--text); }
.prose p + p { margin-top: var(--space-4); }
.nongoals {
  background: var(--surface); border: 1px solid var(--border);
  border-radius: var(--radius-md); padding: var(--space-6);
}
.nongoals h3 { margin-bottom: var(--space-4); }
.nongoals ul { list-style: none; padding: 0; margin: 0; display: flex; flex-direction: column; gap: var(--space-3); }
.nongoals li {
  font-family: var(--font-mono); font-size: 13px; line-height: 1.5; color: var(--text);
  padding-left: 22px; position: relative;
}
.nongoals li::before {
  content: "×"; position: absolute; left: 0; color: var(--accent);
  font-family: var(--font-sans); font-weight: 500; font-size: 15px; top: -1px;
}

/* --- docs layout --- */
.docs {
  display: grid; grid-template-columns: 240px 1fr; gap: var(--space-12);
  padding: var(--space-12) var(--space-6); max-width: 1280px;
  margin: 0 auto; align-items: start;
}
@media (max-width: 900px) {
  .docs { grid-template-columns: 1fr; gap: var(--space-8); }
  .docs-nav { position: static !important; }
}
.docs-nav { position: sticky; top: 88px; align-self: start; font-size: 14px; }
.docs-nav h4 {
  font-family: var(--font-mono); font-size: 11px; text-transform: uppercase;
  letter-spacing: 0.08em; color: var(--text-muted); font-weight: 500;
  margin: var(--space-6) 0 var(--space-2) 0;
}
.docs-nav h4:first-child { margin-top: 0; }
.docs-nav ul { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; }
.docs-nav a {
  display: block; padding: 6px 12px; margin-left: -12px; color: var(--text-muted);
  border-left: 2px solid transparent; font-size: 14px; line-height: 1.4;
  transition: color 200ms var(--ease), border-color 200ms var(--ease);
}
.docs-nav a:hover { color: var(--text); opacity: 1; }
.docs-nav a.is-active { color: var(--text); border-left-color: var(--accent); }

.docs-main { min-width: 0; max-width: 960px; }
.docs-main h2 {
  font-size: 28px; margin-top: var(--space-16); margin-bottom: var(--space-4); scroll-margin-top: 80px;
}
.docs-main > section:first-child h2 { margin-top: 0; }
.docs-main h3 { font-size: 17px; margin-top: var(--space-8); margin-bottom: var(--space-3); letter-spacing: -0.01em; }
.docs-main p { font-size: 15px; line-height: 1.65; color: var(--text); max-width: 70ch; }
.docs-main p.lede { color: var(--text-muted); font-size: 16px; }
.docs-main .code-block { margin: var(--space-4) 0; }
.docs-main ul { padding-left: var(--space-6); margin: 0 0 var(--space-4) 0; }
.docs-main ul li { margin-bottom: var(--space-1); font-size: 15px; color: var(--text); }

.kv-list {
  display: grid; grid-template-columns: 200px 1fr;
  gap: var(--space-2) var(--space-6); font-size: 14px; margin: var(--space-4) 0; max-width: 720px;
}
.kv-list dt {
  font-family: var(--font-mono); color: var(--text); padding: 6px 0; border-top: 1px solid var(--border);
}
.kv-list dd { margin: 0; color: var(--text-muted); padding: 6px 0; border-top: 1px solid var(--border); line-height: 1.5; }
.kv-list dt:first-of-type, .kv-list dd:nth-of-type(1) { border-top: 0; }
@media (max-width: 640px) {
  .kv-list { grid-template-columns: 1fr; gap: 0; }
  .kv-list dd { border-top: 0; padding-top: 0; padding-bottom: var(--space-3); }
}

.grammar {
  background: var(--surface); border: 1px solid var(--border); border-left: 2px solid var(--accent);
  border-radius: var(--radius-sm); padding: var(--space-4) var(--space-6);
  font-family: var(--font-mono); font-size: 13px; line-height: 1.7; color: var(--text);
  margin: var(--space-4) 0; white-space: pre; overflow-x: auto;
}
.grammar .nt { color: var(--accent); }
.grammar .tok { color: var(--text-muted); }
```

- [ ] **Step 4: Verify CSS files exist**

```bash
ls landing/src/styles/
```

Expected: `tokens.css  global.css  components.css`

- [ ] **Step 5: Commit CSS**

```bash
cd /home/arysmart/Projects/Rust/logdive
git add landing/src/styles/
git commit -m "feat(landing): add design system CSS (tokens, global, components)"
```

---

## Task 3: Data file — roadmap and version

**Files:**
- Create: `landing/src/data/roadmap.ts`

- [ ] **Step 1: Create `landing/src/data/roadmap.ts`**

```typescript
// landing/src/data/roadmap.ts

export const CURRENT_VERSION = '0.2.1';
export const IN_PROGRESS_VERSION = '0.3.0';
export const ROADMAP_UPDATED = '2026-06-01';

export type ItemKind = 'feature' | 'perf' | 'infra' | 'docs';

export interface RoadmapItem {
  title: string;
  kind: ItemKind;
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

export const roadmapNow: RoadmapItem[] = [
  {
    title: 'Parenthesised query expressions',
    kind: 'feature',
    issue: 51,
    description: 'Grouping with ( ) so NOT and OR compose cleanly without precedence surprises.',
  },
  {
    title: 'Distroless Docker image',
    kind: 'infra',
    description: 'Replace debian:bookworm-slim runtime with distroless/cc for a smaller attack surface.',
  },
  {
    title: 'Generated columns',
    kind: 'feature',
    description: 'Index derived fields at ingest time to accelerate repeated queries on computed values.',
  },
];

export const roadmapNext: RoadmapItem[] = [
  {
    title: 'Windows support for --follow mode',
    kind: 'infra',
    versionTarget: '0.4.0',
    description: 'Rotation and truncation detection on NTFS using ReadDirectoryChangesW.',
  },
  {
    title: 'Structured output formats: yaml, csv',
    kind: 'feature',
    issue: 39,
    versionTarget: '0.3.0',
  },
  {
    title: 'Configurable retention by source',
    kind: 'feature',
    issue: 44,
    versionTarget: '0.4.0',
    description: 'Let prune --older-than vary per source tag instead of one global cutoff.',
  },
];

export const roadmapLater: RoadmapItem[] = [
  {
    title: 'Authentication for the HTTP API',
    kind: 'infra',
    note: 'waiting on feedback',
    description: 'Currently a non-goal. Reconsidering only if the localhost-only stance is causing real pain.',
  },
  {
    title: 'Multi-file ingest with glob patterns',
    kind: 'feature',
    issue: 33,
    note: 'considering',
  },
  {
    title: 'Aggregations: count, distinct, group-by',
    kind: 'feature',
    issue: 21,
    note: 'considering',
  },
  {
    title: 'Browser-based query UI',
    kind: 'feature',
    note: 'needs spec',
    description: 'Listed for completeness; explicit v1 non-goal. Would need a separate crate and a real design pass.',
  },
];

export const shipped: ShippedRelease[] = [
  {
    version: '0.2.1',
    date: '2026-06-01',
    highlights: [
      'Security test suite: SQL injection, LIKE wildcard escaping, resource exhaustion (1k-disjunct OR, 10 MB line).',
      'Functional tests: proptest property-based, cross-format dedup, concurrent CLI ingest, parser edge cases, follow-mode, API integration, prune boundary.',
      'Supply-chain hardening: cargo-deny, SBOM via cargo-cyclonedx, daily audit CI, CI permissions: contents: read.',
      'Allocation improvements: LogEntry::with_tag takes &str, entry_to_json_string avoids clone per HTTP row.',
    ],
  },
  {
    version: '0.2.0',
    date: '2026-05-15',
    highlights: [
      'Added OR to the query language — (level=error OR level=warn) AND service=payments.',
      'Ingestion now accepts logfmt and plain-text lines alongside JSON.',
      'New --follow mode tails files with rotation and truncation detection.',
      'Introduced the prune subcommand for time-based retention with --older-than.',
      'HTTP API gained /version and /capabilities endpoints, plus configurable CORS.',
      'Docker image is now multi-stage and multi-arch, down to ~9 MB compressed.',
    ],
  },
  {
    version: '0.1.0',
    date: '2026-04-19',
    highlights: [
      'Initial release with ingest, query, and stats subcommands on the CLI.',
      'SQLite-backed local indexing with blake3 content hashing for dedup.',
      'Typed query language supporting AND, =, !=, >, <, contains, last, and since.',
      'Read-only HTTP server exposing /query as NDJSON and /stats as JSON.',
    ],
  },
];
```

- [ ] **Step 2: Verify TypeScript compiles**

```bash
cd landing
bun run build
```

Expected: no TypeScript errors.

- [ ] **Step 3: Commit**

```bash
cd /home/arysmart/Projects/Rust/logdive
git add landing/src/data/roadmap.ts
git commit -m "feat(landing): add typed roadmap and version data"
```

---

## Task 4: Base layout + shared components

**Files:**
- Create: `landing/src/layouts/Base.astro`
- Create: `landing/src/components/Header.astro`
- Create: `landing/src/components/Footer.astro`

- [ ] **Step 1: Create `Base.astro`**

```astro
---
// landing/src/layouts/Base.astro
export interface Props {
  title: string;
  description: string;
  activeNav?: 'docs' | 'about';
}
const { title, description, activeNav } = Astro.props;
import { CURRENT_VERSION } from '@data/roadmap';
import '../styles/tokens.css';
import '../styles/global.css';
import '../styles/components.css';
import Header from '@components/Header.astro';
import Footer from '@components/Footer.astro';
---
<!doctype html>
<html lang="en">
<head>
  <meta charset="UTF-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1.0" />
  <title>{title}</title>
  <meta name="description" content={description} />
  <link rel="icon" href="data:image/svg+xml;utf8,<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 24 24' fill='none' stroke='%230E1419' stroke-width='1.5'><line x1='3' y1='8' x2='21' y2='8' opacity='0.4'/><line x1='3' y1='14' x2='21' y2='14' opacity='0.4'/><line x1='3' y1='20' x2='21' y2='20' opacity='0.4'/><path d='M20 4 Q 14 6 11 14 T 4 22' stroke-width='2' stroke-linecap='round'/><circle cx='4' cy='22' r='1.5' fill='%23B85D44' stroke='none'/></svg>" />
  <script is:inline>
    (function(){
      try {
        var t = localStorage.getItem('logdive-theme');
        if (t === 'light' || t === 'dark') document.documentElement.setAttribute('data-theme', t);
      } catch(e){}
    })();
  </script>
</head>
<body>
  <Header activeNav={activeNav} />
  <slot />
  <Footer version={CURRENT_VERSION} />
</body>
</html>
```

- [ ] **Step 2: Create `Header.astro`**

```astro
---
// landing/src/components/Header.astro
export interface Props { activeNav?: 'docs' | 'about'; }
const { activeNav } = Astro.props;
const base = import.meta.env.BASE_URL;
---
<header class="site-header">
  <div class="wrap">
    <a class="logo" href={`${base}/`} aria-label="logdive home">
      <svg width="28" height="28" viewBox="0 0 24 24" fill="none" aria-hidden="true">
        <line x1="3" y1="8"  x2="21" y2="8"  stroke="currentColor" stroke-width="1.5" opacity="0.4"/>
        <line x1="3" y1="14" x2="21" y2="14" stroke="currentColor" stroke-width="1.5" opacity="0.4"/>
        <line x1="3" y1="20" x2="21" y2="20" stroke="currentColor" stroke-width="1.5" opacity="0.4"/>
        <path d="M20 4 Q 14 6 11 14 T 4 22" stroke="currentColor" stroke-width="2" stroke-linecap="round" fill="none"/>
        <circle cx="4" cy="22" r="1.5" fill="var(--accent)"/>
      </svg>
      <span class="wordmark">logdive</span>
    </a>
    <nav class="nav" id="primary-nav" aria-label="Primary">
      <a href={`${base}/docs`} class={activeNav === 'docs' ? 'is-active' : ''}>Docs</a>
      <a href={`${base}/about`} class={activeNav === 'about' ? 'is-active' : ''}>About</a>
      <a href="https://github.com/Aryagorjipour/logdive" rel="noopener">GitHub</a>
    </nav>
    <button class="icon-btn theme-toggle" type="button" aria-label="Toggle theme">
      <svg class="moon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"><path d="M21 12.79A9 9 0 1 1 11.21 3 7 7 0 0 0 21 12.79z"/></svg>
      <svg class="sun" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="4"/><path d="M12 2v2M12 20v2M4.93 4.93l1.41 1.41M17.66 17.66l1.41 1.41M2 12h2M20 12h2M4.93 19.07l1.41-1.41M17.66 6.34l1.41-1.41"/></svg>
    </button>
    <button class="icon-btn mobile-toggle" type="button" aria-label="Open menu" aria-expanded="false" aria-controls="primary-nav">
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"><line x1="4" y1="7" x2="20" y2="7"/><line x1="4" y1="12" x2="20" y2="12"/><line x1="4" y1="17" x2="20" y2="17"/></svg>
    </button>
  </div>
</header>

<script>
  const toggle = document.querySelector('.theme-toggle')!;
  const mobileToggle = document.querySelector('.mobile-toggle')!;
  const nav = document.querySelector('#primary-nav')!;

  function currentTheme(): string {
    const t = document.documentElement.getAttribute('data-theme');
    if (t === 'light' || t === 'dark') return t;
    return window.matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light';
  }

  toggle.addEventListener('click', () => {
    const next = currentTheme() === 'dark' ? 'light' : 'dark';
    document.documentElement.setAttribute('data-theme', next);
    try { localStorage.setItem('logdive-theme', next); } catch {}
  });

  mobileToggle.addEventListener('click', () => {
    const open = nav.classList.toggle('is-open');
    mobileToggle.setAttribute('aria-expanded', String(open));
  });
</script>
```

- [ ] **Step 3: Create `Footer.astro`**

```astro
---
// landing/src/components/Footer.astro
export interface Props { version: string; }
const { version } = Astro.props;
const base = import.meta.env.BASE_URL;
---
<footer class="site-footer">
  <div class="wrap">
    <a class="logo" href={`${base}/`} aria-label="logdive home">
      <svg width="20" height="20" viewBox="0 0 24 24" fill="none" aria-hidden="true">
        <line x1="3" y1="8"  x2="21" y2="8"  stroke="currentColor" stroke-width="1.5" opacity="0.4"/>
        <line x1="3" y1="14" x2="21" y2="14" stroke="currentColor" stroke-width="1.5" opacity="0.4"/>
        <line x1="3" y1="20" x2="21" y2="20" stroke="currentColor" stroke-width="1.5" opacity="0.4"/>
        <path d="M20 4 Q 14 6 11 14 T 4 22" stroke="currentColor" stroke-width="2" stroke-linecap="round" fill="none"/>
        <circle cx="4" cy="22" r="1.5" fill="var(--accent)"/>
      </svg>
      <span class="wordmark">logdive</span>
    </a>
    <span>MIT OR Apache-2.0</span>
    <span class="version">v{version}</span>
    <nav class="footer-links" aria-label="Footer">
      <a href="https://github.com/Aryagorjipour/logdive" rel="noopener">GitHub</a>
      <a href="https://github.com/Aryagorjipour" rel="noopener">@Aryagorjipour</a>
      <a href={`${base}/docs`}>Docs</a>
    </nav>
  </div>
</footer>
```

- [ ] **Step 4: Verify build**

```bash
cd landing && bun run build
```

Expected: no errors.

- [ ] **Step 5: Commit**

```bash
cd /home/arysmart/Projects/Rust/logdive
git add landing/src/layouts/ landing/src/components/Header.astro landing/src/components/Footer.astro
git commit -m "feat(landing): add Base layout, Header, Footer components"
```

---

## Task 5: CodeBlock and Tabs components

**Files:**
- Create: `landing/src/components/CodeBlock.astro`
- Create: `landing/src/components/Tabs.astro`

- [ ] **Step 1: Create `CodeBlock.astro`**

```astro
---
// landing/src/components/CodeBlock.astro
export interface Props {
  id: string;
  maxWidth?: string;
}
const { id, maxWidth } = Astro.props;
---
<div class="code-block" style={maxWidth ? `max-width: ${maxWidth}` : undefined}>
  <button class="copy-btn" type="button" data-copy-target={`#${id}`}>Copy</button>
  <pre id={id}><slot /></pre>
</div>

<script>
  document.querySelectorAll<HTMLButtonElement>('.copy-btn[data-copy-target]').forEach(btn => {
    btn.addEventListener('click', () => {
      const selector = btn.getAttribute('data-copy-target')!;
      const node = document.querySelector<HTMLElement>(selector);
      const text = node?.innerText ?? '';
      if (!text) return;

      const write = navigator.clipboard?.writeText(text);
      if (write) {
        write.then(() => flash(btn));
      } else {
        const ta = document.createElement('textarea');
        ta.value = text;
        document.body.appendChild(ta);
        ta.select();
        try { document.execCommand('copy'); } catch {}
        document.body.removeChild(ta);
        flash(btn);
      }
    });
  });

  function flash(btn: HTMLButtonElement) {
    const orig = btn.textContent!;
    btn.classList.add('is-copied');
    btn.textContent = 'COPIED';
    setTimeout(() => { btn.classList.remove('is-copied'); btn.textContent = orig; }, 1400);
  }
</script>
```

- [ ] **Step 2: Create `Tabs.astro`**

```astro
---
// landing/src/components/Tabs.astro
export interface Tab { id: string; label: string; }
export interface Props { tabs: Tab[]; }
const { tabs } = Astro.props;
---
<div data-tabs>
  <div class="tabs" role="tablist">
    {tabs.map((tab, i) => (
      <button
        class={`tab-btn${i === 0 ? ' is-active' : ''}`}
        type="button"
        data-tab={tab.id}
        role="tab"
        aria-selected={i === 0 ? 'true' : 'false'}
        aria-controls={`panel-${tab.id}`}
      >
        {tab.label}
      </button>
    ))}
  </div>
  <slot />
</div>

<script>
  document.querySelectorAll<HTMLElement>('[data-tabs]').forEach(group => {
    const buttons = group.querySelectorAll<HTMLButtonElement>('.tab-btn');
    const panels = group.querySelectorAll<HTMLElement>('.tab-panel');
    buttons.forEach(btn => {
      btn.addEventListener('click', () => {
        const id = btn.getAttribute('data-tab')!;
        buttons.forEach(b => {
          b.classList.toggle('is-active', b === btn);
          b.setAttribute('aria-selected', String(b === btn));
        });
        panels.forEach(p => p.classList.toggle('is-active', p.getAttribute('data-panel') === id));
      });
    });
  });
</script>
```

- [ ] **Step 3: Verify build**

```bash
cd landing && bun run build
```

Expected: no errors.

- [ ] **Step 4: Commit**

```bash
cd /home/arysmart/Projects/Rust/logdive
git add landing/src/components/CodeBlock.astro landing/src/components/Tabs.astro
git commit -m "feat(landing): add CodeBlock and Tabs interactive components"
```

---

## Task 6: TerminalPreview and RoadmapStatus components

**Files:**
- Create: `landing/src/components/TerminalPreview.astro`
- Create: `landing/src/components/RoadmapStatus.astro`

- [ ] **Step 1: Create `TerminalPreview.astro`**

```astro
---
// landing/src/components/TerminalPreview.astro
---
<div class="terminal" role="figure" aria-label="Example logdive queries">
  <div class="terminal-head">
    <span class="terminal-dots" aria-hidden="true"><span></span><span></span><span></span></span>
    <span>~/services/payments  —  logdive</span>
  </div>
  <div class="terminal-body">
    <div class="cmd">
      <span class="prompt">$</span>logdive query <span class="str">'level=error AND service=payments last 2h'</span>
    </div>
    <div class="note">All errors from the payments service in the last 2 hours.</div>

    <div class="cmd">
      <span class="prompt">$</span>logdive query <span class="str">'level=error OR level=warn'</span> --format json
    </div>
    <div class="note">Pipe results downstream — output is line-delimited JSON.</div>

    <div class="cmd">
      <span class="prompt">$</span>logdive query <span class="str">'message contains <span class="accent-tok">"timeout"</span> last 24h'</span>
    </div>
    <div class="note">Substring match on the message body. Highlighted token is the literal you're searching for.</div>
  </div>
</div>
```

- [ ] **Step 2: Create `RoadmapStatus.astro`**

```astro
---
// landing/src/components/RoadmapStatus.astro
import {
  roadmapNow, roadmapNext, roadmapLater, shipped,
  IN_PROGRESS_VERSION, ROADMAP_UPDATED,
  type RoadmapItem, type ShippedRelease,
} from '@data/roadmap';

function issueUrl(issue: number) {
  return `https://github.com/Aryagorjipour/logdive/issues/${issue}`;
}
---

<section id="status" aria-labelledby="status-h2">
  <div class="wrap">
    <header>
      <span class="eyebrow">Project status</span>
      <h2 id="status-h2">Project status.</h2>
      <p class="meta">
        <time datetime={ROADMAP_UPDATED}>Last updated {new Date(ROADMAP_UPDATED).toLocaleDateString('en-GB', { day: 'numeric', month: 'long', year: 'numeric' })}</time>
        <span class="sep" aria-hidden="true">·</span>
        <a href="https://github.com/Aryagorjipour/logdive/issues" rel="noopener">All issues on GitHub</a>
      </p>
    </header>

    <div class="status-lanes">
      <!-- NOW -->
      <article class="status-lane" aria-labelledby="lane-now-h">
        <h3 id="lane-now-h">Now</h3>
        <p class="lane-desc">Active development. Currently building v{IN_PROGRESS_VERSION}.</p>
        <ul>
          {roadmapNow.length === 0
            ? <li class="lane-empty">Nothing active right now.</li>
            : roadmapNow.map(item => (
              <li>
                {item.issue
                  ? <a href={issueUrl(item.issue)} rel="noopener">{item.title}</a>
                  : <span class="item-title">{item.title}</span>
                }
                {item.description && <p class="item-desc">{item.description}</p>}
              </li>
            ))
          }
        </ul>
      </article>

      <!-- NEXT -->
      <article class="status-lane" aria-labelledby="lane-next-h">
        <h3 id="lane-next-h">Next</h3>
        <p class="lane-desc">Planned for the next release or two. Likely to ship.</p>
        <ul>
          {roadmapNext.length === 0
            ? <li class="lane-empty">Nothing queued yet.</li>
            : roadmapNext.map(item => (
              <li>
                {item.issue
                  ? <a href={issueUrl(item.issue)} rel="noopener">{item.title}</a>
                  : <span class="item-title">{item.title}</span>
                }
                {item.versionTarget && <div class="item-meta">target: v{item.versionTarget}</div>}
                {item.description && <p class="item-desc">{item.description}</p>}
              </li>
            ))
          }
        </ul>
      </article>

      <!-- LATER -->
      <article class="status-lane" aria-labelledby="lane-later-h">
        <h3 id="lane-later-h">Later</h3>
        <p class="lane-desc">Under consideration. No timeline, may not happen.</p>
        <ul>
          {roadmapLater.length === 0
            ? <li class="lane-empty">Nothing under consideration.</li>
            : roadmapLater.map(item => (
              <li>
                {item.issue
                  ? <a href={issueUrl(item.issue)} rel="noopener">{item.title}</a>
                  : <span class="item-title">{item.title}</span>
                }
                {item.note && <div class="item-meta">{item.note}</div>}
                {item.description && <p class="item-desc">{item.description}</p>}
              </li>
            ))
          }
        </ul>
      </article>
    </div>

    <!-- SHIPPED -->
    <div class="status-shipped">
      <h3>Recently shipped</h3>
      <p class="shipped-meta">
        Full history in <a href="https://github.com/Aryagorjipour/logdive/blob/main/CHANGELOG.md" rel="noopener">CHANGELOG.md</a> on GitHub.
      </p>
      <ol id="shipped-list">
        {shipped.map((release, i) => (
          <li>
            <details open={i === 0}>
              <summary>
                <span class="release-version">v{release.version}</span>
                <span class="release-date">{new Date(release.date).toLocaleDateString('en-GB', { day: 'numeric', month: 'long', year: 'numeric' })}</span>
              </summary>
              <ul class="release-highlights">
                {release.highlights.map(h => <li>{h}</li>)}
              </ul>
            </details>
          </li>
        ))}
      </ol>
    </div>

    <p class="status-oos">
      Looking for something that isn't here? Check the <a href={`${import.meta.env.BASE_URL}/about#non-goals`}>v1 non-goals</a> — some things are intentionally out of scope.
    </p>
  </div>
</section>
```

- [ ] **Step 3: Verify build**

```bash
cd landing && bun run build
```

Expected: no errors.

- [ ] **Step 4: Commit**

```bash
cd /home/arysmart/Projects/Rust/logdive
git add landing/src/components/TerminalPreview.astro landing/src/components/RoadmapStatus.astro
git commit -m "feat(landing): add TerminalPreview and RoadmapStatus components"
```

---

## Task 7: Home page (`index.astro`)

**Files:**
- Create: `landing/src/pages/index.astro`

- [ ] **Step 1: Create `landing/src/pages/index.astro`**

```astro
---
// landing/src/pages/index.astro
import Base from '@layouts/Base.astro';
import CodeBlock from '@components/CodeBlock.astro';
import Tabs from '@components/Tabs.astro';
import TerminalPreview from '@components/TerminalPreview.astro';
import RoadmapStatus from '@components/RoadmapStatus.astro';
import { CURRENT_VERSION } from '@data/roadmap';
const base = import.meta.env.BASE_URL;
---
<Base
  title="logdive — jq with memory."
  description="Self-hosted log query for engineers. Single binary, SQLite-backed, no daemon. Written in Rust. MIT OR Apache-2.0."
>
<main>

  <!-- 1. HERO -->
  <section class="hero">
    <div class="hero-lines" aria-hidden="true"></div>
    <div class="wrap">
      <span class="eyebrow">v{CURRENT_VERSION} · MIT OR Apache-2.0</span>
      <h1>jq with memory<span class="accent">.</span></h1>
      <p class="hero-sub">Self-hosted log query for engineers who don't want a Datadog bill. Single binary, SQLite-backed, no daemon, no agent, no account.</p>
      <div class="hero-cta">
        <div class="install-code">
          <span class="prompt">$</span>
          <span id="hero-install-text">cargo install logdive</span>
          <button class="copy-btn" type="button" data-copy-target="#hero-install-text" aria-label="Copy install command">Copy</button>
        </div>
        <a class="btn btn-ghost" href="https://github.com/Aryagorjipour/logdive" rel="noopener">
          <svg width="14" height="14" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><path d="M12 .5C5.7.5.5 5.7.5 12c0 5.1 3.3 9.4 7.9 10.9.6.1.8-.2.8-.6v-2.1c-3.2.7-3.9-1.5-3.9-1.5-.5-1.4-1.3-1.7-1.3-1.7-1-.7.1-.7.1-.7 1.1.1 1.7 1.2 1.7 1.2 1 1.7 2.7 1.2 3.4.9.1-.7.4-1.2.7-1.5-2.6-.3-5.3-1.3-5.3-5.7 0-1.3.4-2.3 1.2-3.1-.1-.3-.5-1.5.1-3.1 0 0 1-.3 3.2 1.2.9-.3 1.9-.4 2.9-.4s2 .1 2.9.4c2.2-1.5 3.2-1.2 3.2-1.2.6 1.6.2 2.8.1 3.1.7.8 1.2 1.9 1.2 3.1 0 4.4-2.7 5.4-5.3 5.7.4.4.8 1.1.8 2.2v3.3c0 .3.2.7.8.6 4.6-1.5 7.9-5.8 7.9-10.9C23.5 5.7 18.3.5 12 .5z"/></svg>
          View on GitHub
        </a>
      </div>
    </div>
  </section>

  <!-- 2. QUERY LANGUAGE PREVIEW -->
  <section>
    <div class="wrap">
      <div class="section-head">
        <span class="eyebrow">The query language</span>
        <h2>Filters that read like English. Time ranges that just work.</h2>
        <p>Boolean expressions over fields, with first-class time windows and JSON path extraction. No regex required for the common case.</p>
      </div>
      <TerminalPreview />
    </div>
  </section>

  <!-- 3. PERFORMANCE -->
  <section>
    <div class="wrap">
      <div class="section-head">
        <span class="eyebrow">Performance</span>
        <h2>Fast enough that grep starts to feel slow.</h2>
        <p>Measured on the project's own criterion suite against a 100k-row corpus. Your hardware will vary; the shape of the numbers won't.</p>
      </div>
      <div class="stat-grid">
        <div class="stat-card">
          <div class="stat-num">210<span class="unit">k lines/s</span></div>
          <div class="stat-desc">Batched ingest throughput. Sustained, not peak.</div>
        </div>
        <div class="stat-card">
          <div class="stat-num">166<span class="unit">k lines/s</span></div>
          <div class="stat-desc">End-to-end parse + ingest, JSON in, indexed out.</div>
        </div>
        <div class="stat-card">
          <div class="stat-num">17<span class="unit">µs</span></div>
          <div class="stat-desc">Indexed-field query against 100k rows.</div>
        </div>
        <div class="stat-card">
          <div class="stat-num">3.6<span class="unit">ms</span></div>
          <div class="stat-desc">json_extract field query against 100k rows.</div>
        </div>
      </div>
      <p class="muted" style="margin-top: var(--space-6); font-size: 14px;">
        Full-table <code class="code-inline">CONTAINS</code> scans land at 36–40&thinsp;ms over 100k rows. Run <code class="code-inline">cargo bench</code> for your own baseline.
      </p>
    </div>
  </section>

  <!-- 4. PILLARS -->
  <section>
    <div class="wrap">
      <div class="section-head">
        <span class="eyebrow">What you get</span>
        <h2>Three things, done well.</h2>
      </div>
      <div class="pillar-grid">
        <div class="pillar">
          <svg width="32" height="32" viewBox="0 0 32 32" fill="none" stroke="currentColor" stroke-width="1.4" aria-hidden="true">
            <rect x="6" y="9" width="20" height="16" rx="1.5"/>
            <line x1="6" y1="14" x2="26" y2="14"/>
            <circle cx="10" cy="11.5" r="0.8" fill="currentColor"/>
            <circle cx="13" cy="11.5" r="0.8" fill="currentColor"/>
          </svg>
          <h3>Local-first</h3>
          <p>One binary, one SQLite file. No daemon to babysit, no cloud account to authorize, no agent to ship logs anywhere. Your data stays on the machine that produced it.</p>
        </div>
        <div class="pillar">
          <svg width="32" height="32" viewBox="0 0 32 32" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" aria-hidden="true">
            <path d="M6 22 L12 14 L17 19 L26 8"/>
            <path d="M20 8 L26 8 L26 14"/>
          </svg>
          <h3>Fast queries</h3>
          <p>SQLite handles the storage. blake3 deduplicates content hashes. <code class="code-inline">json_extract</code> reaches into structured payloads without re-parsing. Microsecond reads on indexed fields.</p>
        </div>
        <div class="pillar">
          <svg width="32" height="32" viewBox="0 0 32 32" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" aria-hidden="true">
            <path d="M8 8 L24 8"/><path d="M8 16 L20 16"/><path d="M8 24 L16 24"/>
            <path d="M28 6 L26 8 L28 10"/><path d="M28 14 L26 16 L28 18"/><path d="M28 22 L26 24 L28 26"/>
          </svg>
          <h3>Multi-format ingestion</h3>
          <p>JSON for the modern stack. logfmt for the old guard. Plain text for everything else. logdive sniffs the format per-line and normalizes into a single queryable shape.</p>
        </div>
      </div>
    </div>
  </section>

  <!-- 5. ARCHITECTURE -->
  <section>
    <div class="wrap">
      <div class="section-head">
        <span class="eyebrow">Architecture</span>
        <h2>A three-crate workspace.</h2>
        <p>The core does the work. The CLI is a thin wrapper. The API is read-only and optional. You can use one without the other two.</p>
      </div>
      <CodeBlock id="arch-tree" maxWidth="720px">
<span class="cm">// Cargo workspace</span>
logdive/
├── <span class="kw">logdive-core</span>     <span class="cm">// ingestion, query parser, SQLite layer</span>
├── <span class="kw">logdive</span>          <span class="cm">// CLI binary — 3.7 MB stripped</span>
└── <span class="kw">logdive-api</span>      <span class="cm">// read-only HTTP server — 4.1 MB stripped</span>
      </CodeBlock>
    </div>
  </section>

  <!-- 6. COMPARISON -->
  <section>
    <div class="wrap">
      <div class="section-head">
        <span class="eyebrow">When to reach for logdive</span>
        <h2>And, more importantly, when not to.</h2>
      </div>
      <div class="compare">
        <p>logdive is for one machine — a laptop, a VPS, a single Kubernetes node. You point it at log files or a stdin stream, you query them later. That's the whole product. It replaces the loop of <code class="code-inline">grep | jq | awk</code> with something that has indexes and time ranges.</p>
        <p><strong>Loki</strong> is the right answer when you have a fleet and you're already running Prometheus. <strong>Datadog</strong> is the right answer when someone else is paying the bill and you want a polished UI. <strong>Elastic</strong> is the right answer when you need full-text search at scale and have an ops team to run the cluster.</p>
        <p>logdive is what you reach for in the gap below all of that — when the alternative isn't another observability platform, it's a fifteen-line shell pipeline that you'd rather not write again.</p>
        <p class="honest"><strong>Honest limit:</strong> if you have more than one machine producing logs, logdive is the wrong tool. There is no clustering, no cross-host index, no shipping protocol. Use Loki.</p>
      </div>
    </div>
  </section>

  <!-- 7. PROJECT STATUS -->
  <RoadmapStatus />

  <!-- 8. INSTALLATION -->
  <section>
    <div class="wrap">
      <div class="section-head">
        <span class="eyebrow">Get it</span>
        <h2>Install in one command.</h2>
        <p>Pick whichever your build pipeline already understands.</p>
      </div>
      <Tabs tabs={[
        { id: 'cargo', label: 'cargo' },
        { id: 'docker', label: 'docker' },
        { id: 'source', label: 'from source' },
      ]}>
        <div class="tab-panel is-active" data-panel="cargo">
          <CodeBlock id="cargo-install">
<span class="prompt">$</span> cargo install logdive
<span class="prompt">$</span> cargo install logdive-api  <span class="cm"># optional, for the HTTP server</span>
          </CodeBlock>
        </div>
        <div class="tab-panel" data-panel="docker">
          <CodeBlock id="docker-install">
<span class="prompt">$</span> docker pull ghcr.io/aryagorjipour/logdive:{CURRENT_VERSION}
<span class="prompt">$</span> docker run -d \
    --name logdive \
    -v logdive-data:/data \
    -p 4000:4000 \
    ghcr.io/aryagorjipour/logdive:{CURRENT_VERSION}
          </CodeBlock>
        </div>
        <div class="tab-panel" data-panel="source">
          <CodeBlock id="source-install">
<span class="prompt">$</span> git clone https://github.com/Aryagorjipour/logdive
<span class="prompt">$</span> cd logdive
<span class="prompt">$</span> cargo build --release
<span class="prompt">$</span> ./target/release/logdive --version
          </CodeBlock>
        </div>
      </Tabs>
    </div>
  </section>

</main>
</Base>
```

- [ ] **Step 2: Verify build and check output**

```bash
cd landing && bun run build && ls dist/
```

Expected: `dist/logdive/index.html` exists.

- [ ] **Step 3: Smoke test locally**

```bash
cd landing && bun run preview
```

Open `http://localhost:4321/logdive/` — verify hero, terminal, stat grid, pillars, roadmap lanes, installation tabs all render. Check dark mode toggle.

- [ ] **Step 4: Commit**

```bash
cd /home/arysmart/Projects/Rust/logdive
git add landing/src/pages/index.astro
git commit -m "feat(landing): implement home page (7 sections)"
```

---

## Task 8: Docs page (`docs.astro`)

**Files:**
- Create: `landing/src/pages/docs.astro`

- [ ] **Step 1: Create `landing/src/pages/docs.astro`**

```astro
---
// landing/src/pages/docs.astro
import Base from '@layouts/Base.astro';
import CodeBlock from '@components/CodeBlock.astro';
import { CURRENT_VERSION } from '@data/roadmap';
---
<Base
  title="Docs — logdive"
  description="Documentation for logdive: CLI, HTTP API, query language, configuration."
  activeNav="docs"
>

<div class="docs">

  <aside class="docs-nav" aria-label="Documentation">
    <h4>Get started</h4>
    <ul>
      <li><a href="#quick-start">Quick start</a></li>
      <li><a href="#installation">Installation</a></li>
    </ul>
    <h4>The CLI</h4>
    <ul>
      <li><a href="#cli-ingest">ingest</a></li>
      <li><a href="#cli-query">query</a></li>
      <li><a href="#cli-stats">stats</a></li>
      <li><a href="#cli-prune">prune</a></li>
    </ul>
    <h4>The HTTP API</h4>
    <ul>
      <li><a href="#api-query">/query</a></li>
      <li><a href="#api-stats">/stats</a></li>
      <li><a href="#api-version">/version</a></li>
    </ul>
    <h4>Reference</h4>
    <ul>
      <li><a href="#query-language">Query language</a></li>
      <li><a href="#configuration">Configuration</a></li>
      <li><a href="#docker">Docker</a></li>
      <li><a href="#architecture">Architecture</a></li>
    </ul>
  </aside>

  <main class="docs-main">

    <!-- QUICK START -->
    <section id="quick-start">
      <h2>Quick start</h2>
      <p class="lede">Install the binary, ingest a log file, run a query. Five lines, no flags worth worrying about yet.</p>
      <CodeBlock id="qs-block">
<span class="prompt">$</span> cargo install logdive
<span class="prompt">$</span> logdive ingest --file /var/log/app.log
<span class="cm">  → 12,847 lines indexed in 78ms</span>
<span class="prompt">$</span> logdive query <span class="str">'level=error last 1h'</span>
<span class="prompt">$</span> logdive stats
      </CodeBlock>
      <p>By default, the index lives at <code class="code-inline">~/.logdive/index.db</code>. Override with <code class="code-inline">--db &lt;path&gt;</code> on any command, or set <code class="code-inline">LOGDIVE_DB</code>.</p>
    </section>

    <!-- INSTALLATION -->
    <section id="installation">
      <h2>Installation</h2>
      <p>Three supported paths.</p>

      <h3>From crates.io</h3>
      <CodeBlock id="i-cargo">
<span class="prompt">$</span> cargo install logdive
<span class="prompt">$</span> cargo install logdive-api  <span class="cm"># optional, for the HTTP server</span>
      </CodeBlock>

      <h3>From Docker</h3>
      <CodeBlock id="i-docker">
<span class="prompt">$</span> docker pull ghcr.io/aryagorjipour/logdive:{CURRENT_VERSION}
      </CodeBlock>

      <h3>From source</h3>
      <CodeBlock id="i-source">
<span class="prompt">$</span> git clone https://github.com/Aryagorjipour/logdive
<span class="prompt">$</span> cd logdive
<span class="prompt">$</span> cargo build --release
      </CodeBlock>
      <p class="lede">Resulting binaries: <code class="code-inline">logdive</code> at 3.7&thinsp;MB stripped, <code class="code-inline">logdive-api</code> at 4.1&thinsp;MB stripped. MSRV: Rust 1.85.</p>
    </section>

    <!-- CLI -->
    <section id="cli-ingest">
      <h2>The CLI</h2>
      <p>One binary, four subcommands.</p>

      <h3>ingest</h3>
      <p>Reads a file or stdin, parses log lines, and inserts them into the SQLite index. Supports JSON (default), logfmt, and plain text. Deduplicates via blake3 content hash.</p>
      <CodeBlock id="c-ingest">
<span class="prompt">$</span> logdive ingest --file ./logs/app.log
<span class="prompt">$</span> logdive ingest --file ./logs/app.log --format logfmt --tag production
<span class="prompt">$</span> docker logs my-container | logdive ingest --tag my-container
<span class="prompt">$</span> logdive ingest --file ./logs/app.log --follow
      </CodeBlock>
      <dl class="kv-list">
        <dt>--file &lt;PATH&gt;</dt>
        <dd>Read from a file. Mutually exclusive with stdin.</dd>
        <dt>--format json|logfmt|plain</dt>
        <dd>Input format. Default <code class="code-inline">json</code>.</dd>
        <dt>--tag &lt;TAG&gt;</dt>
        <dd>Attach a tag to every ingested entry that does not already have a <code class="code-inline">tag</code> field.</dd>
        <dt>--timestamp-now</dt>
        <dd>Assign current UTC time to entries lacking a <code class="code-inline">timestamp</code> field instead of skipping them.</dd>
        <dt>--follow</dt>
        <dd>Tail the file for new lines, similar to <code class="code-inline">tail -f</code>. Detects log rotation and truncation. Requires <code class="code-inline">--file</code>. Unix only (Windows support is v0.3+).</dd>
        <dt>--db &lt;PATH&gt;</dt>
        <dd>Database path override. Default <code class="code-inline">~/.logdive/index.db</code>. Also settable via <code class="code-inline">LOGDIVE_DB</code>.</dd>
      </dl>

      <h3 id="cli-query">query</h3>
      <p>Evaluates a query expression and prints matching rows.</p>
      <CodeBlock id="c-query">
<span class="prompt">$</span> logdive query <span class="str">'level=error AND service=payments last 2h'</span>
<span class="prompt">$</span> logdive query <span class="str">'level=error OR level=warn'</span> --format json
<span class="prompt">$</span> logdive query <span class="str">'message contains "timeout" last 24h'</span>
<span class="prompt">$</span> logdive query <span class="str">'since 2026-01-01'</span> --limit 0
      </CodeBlock>
      <dl class="kv-list">
        <dt>--format pretty|json</dt>
        <dd>Output format. Default <code class="code-inline">pretty</code> (colored). <code class="code-inline">json</code> is newline-delimited, pipe-friendly.</dd>
        <dt>--limit &lt;N&gt;</dt>
        <dd>Maximum results. Default 1000. Use <code class="code-inline">0</code> for unlimited.</dd>
        <dt>--db &lt;PATH&gt;</dt>
        <dd>Database path override.</dd>
      </dl>

      <h3 id="cli-stats">stats</h3>
      <p>Reports aggregate metadata about the index — row count, time range, tags, and DB size on disk.</p>
      <CodeBlock id="c-stats">
<span class="prompt">$</span> logdive stats
<span class="cm">logdive index: /home/user/.logdive/index.db
  Entries:       42,317
  Time range:    2026-03-14T08:22:01Z → 2026-04-22T19:45:03Z
  Tags:          api, nginx, payments, worker, (untagged)
  DB size:       8.4 MB (8,400,000 bytes)</span>
      </CodeBlock>

      <h3 id="cli-prune">prune</h3>
      <p>Deletes entries outside a retention window, then vacuums the database file to reclaim disk space. Safe for cron.</p>
      <CodeBlock id="c-prune">
<span class="prompt">$</span> logdive prune --older-than 30d
<span class="prompt">$</span> logdive prune --before 2026-01-01
<span class="prompt">$</span> logdive prune --older-than 7d --yes
      </CodeBlock>
      <dl class="kv-list">
        <dt>--older-than &lt;DURATION&gt;</dt>
        <dd>Delete entries older than this. Format: integer + <code class="code-inline">m</code>, <code class="code-inline">h</code>, or <code class="code-inline">d</code>. E.g. <code class="code-inline">30d</code>, <code class="code-inline">24h</code>. Mutually exclusive with <code class="code-inline">--before</code>.</dd>
        <dt>--before &lt;DATETIME&gt;</dt>
        <dd>Delete entries before this datetime. Accepts RFC 3339, ISO naive datetime, or ISO date. Mutually exclusive with <code class="code-inline">--older-than</code>.</dd>
        <dt>--yes</dt>
        <dd>Skip the interactive <code class="code-inline">[y/N]</code> confirmation. Useful in scripts and cron.</dd>
        <dt>--db &lt;PATH&gt;</dt>
        <dd>Database path override.</dd>
      </dl>
    </section>

    <!-- HTTP API -->
    <section id="api-query">
      <h2>The HTTP API</h2>
      <p>The <code class="code-inline">logdive-api</code> binary serves the same query language over HTTP, read-only. No authentication — bind it to localhost.</p>
      <CodeBlock id="api-run">
<span class="prompt">$</span> logdive-api --db ~/.logdive/index.db --port 4000
      </CodeBlock>

      <h3>GET /query</h3>
      <p>Runs a query and returns matching entries as newline-delimited JSON.</p>
      <dl class="kv-list">
        <dt>q (required)</dt>
        <dd>Query expression. URL-encoded. Same syntax as CLI.</dd>
        <dt>limit (optional)</dt>
        <dd>Maximum results. Default 1000. <code class="code-inline">0</code> for unlimited.</dd>
      </dl>
      <CodeBlock id="api-q">
<span class="prompt">$</span> curl <span class="str">'http://127.0.0.1:4000/query?q=level%3Derror&amp;limit=50'</span>
<span class="cm">{"timestamp":"2026-05-21T14:02:31Z","level":"error","message":"..."}
{"timestamp":"2026-05-21T14:02:33Z","level":"error","message":"..."}</span>
      </CodeBlock>

      <h3 id="api-stats">GET /stats</h3>
      <p>Returns aggregate metadata as a single JSON object.</p>
      <CodeBlock id="api-stats-ex">
<span class="cm">{
  "entries": 42317,
  "min_timestamp": "2026-03-14T08:22:01Z",
  "max_timestamp": "2026-04-22T19:45:03Z",
  "tags": [null, "api", "nginx", "payments", "worker"],
  "db_size_bytes": 8400000,
  "db_path": "/home/user/.logdive/index.db"
}</span>
      </CodeBlock>

      <h3 id="api-version">GET /version</h3>
      <p>Returns build version and supported capabilities. Never touches the database. Use as a liveness probe.</p>
      <CodeBlock id="api-version-ex">
<span class="cm">{
  "version": "{CURRENT_VERSION}",
  "formats": ["json", "logfmt", "plain"],
  "capabilities": ["query", "stats", "version"]
}</span>
      </CodeBlock>
    </section>

    <!-- QUERY LANGUAGE -->
    <section id="query-language">
      <h2>Query language</h2>
      <p class="lede">Boolean expressions over fields, plus an optional trailing time window. Fields can be indexed columns (<code class="code-inline">timestamp</code>, <code class="code-inline">level</code>, <code class="code-inline">message</code>, <code class="code-inline">tag</code>) or arbitrary JSON paths (<code class="code-inline">user.id</code>, <code class="code-inline">request.method</code>).</p>

      <h3>Grammar</h3>
      <div class="grammar"><span class="nt">query</span>      <span class="tok">::=</span> <span class="nt">or_expr</span> [ <span class="nt">time_range</span> ]
<span class="nt">or_expr</span>    <span class="tok">::=</span> <span class="nt">and_expr</span> ( <span class="tok">"OR"</span> <span class="nt">and_expr</span> )*
<span class="nt">and_expr</span>   <span class="tok">::=</span> <span class="nt">clause</span> ( <span class="tok">"AND"</span> <span class="nt">clause</span> )*
<span class="nt">clause</span>     <span class="tok">::=</span> <span class="nt">field</span> <span class="nt">op</span> <span class="nt">value</span> | <span class="nt">field</span> <span class="tok">"CONTAINS"</span> <span class="nt">string</span>
<span class="nt">op</span>         <span class="tok">::=</span> <span class="tok">"="</span> | <span class="tok">"!="</span> | <span class="tok">">"</span> | <span class="tok">"&lt;"</span>
<span class="nt">field</span>      <span class="tok">::=</span> <span class="nt">ident</span> ( <span class="tok">"."</span> <span class="nt">ident</span> )*
<span class="nt">value</span>      <span class="tok">::=</span> <span class="nt">ident</span> | <span class="nt">number</span> | <span class="nt">quoted_string</span>
<span class="nt">time_range</span> <span class="tok">::=</span> <span class="tok">"last"</span> <span class="nt">duration</span> | <span class="tok">"since"</span> <span class="nt">datetime</span>
<span class="nt">duration</span>   <span class="tok">::=</span> <span class="nt">number</span> ( <span class="tok">"m"</span> | <span class="tok">"h"</span> | <span class="tok">"d"</span> )</div>
      <p>Keywords are case-insensitive. Parenthesised expressions are not yet supported (planned for v0.3).</p>

      <h3>Operators</h3>
      <dl class="kv-list">
        <dt>=</dt><dd>Exact match. Hits an index on known fields.</dd>
        <dt>!=</dt><dd>Negation of <code class="code-inline">=</code>. Still indexed.</dd>
        <dt>&gt;, &lt;</dt><dd>Numeric or lexicographic comparison.</dd>
        <dt>CONTAINS</dt><dd>Case-insensitive substring match. Full-table scan on the target field.</dd>
        <dt>AND</dt><dd>Binds clauses within a group. Tighter precedence than OR.</dd>
        <dt>OR</dt><dd>Separates AND groups. Each group is evaluated independently.</dd>
        <dt>last &lt;N&gt;m|h|d</dt><dd>Time window ending now.</dd>
        <dt>since &lt;datetime&gt;</dt><dd>Absolute lower bound. RFC 3339, ISO naive datetime, or ISO date.</dd>
      </dl>

      <h3>Examples</h3>
      <CodeBlock id="ql-ex">
<span class="cm"># All errors from payments in the last 2 hours</span>
level=error AND service=payments last 2h

<span class="cm"># Errors OR warnings</span>
level=error OR level=warn

<span class="cm"># AND within each OR branch</span>
level=error AND service=payments OR level=warn AND tag=worker

<span class="cm"># Substring search on the message body</span>
message contains <span class="str">"timeout"</span> last 24h

<span class="cm"># Slow requests over 500ms</span>
duration_ms &gt; 500

<span class="cm"># Time range by absolute date</span>
since 2026-04-15T09:00:00Z
      </CodeBlock>
    </section>

    <!-- CONFIGURATION -->
    <section id="configuration">
      <h2>Configuration</h2>
      <p>All configuration is via command-line flags, with environment-variable fallbacks for containerised deployments.</p>
      <dl class="kv-list">
        <dt>LOGDIVE_DB</dt>
        <dd>Database path. Fallback for <code class="code-inline">--db</code>. Default <code class="code-inline">~/.logdive/index.db</code>.</dd>
        <dt>LOGDIVE_LOG</dt>
        <dd>Verbosity filter for internal diagnostics (<code class="code-inline">tracing_subscriber::EnvFilter</code>). Default <code class="code-inline">warn</code>.</dd>
        <dt>LOGDIVE_API_PORT</dt>
        <dd>Port for <code class="code-inline">logdive-api</code>. Fallback for <code class="code-inline">--port</code>. Default <code class="code-inline">4000</code>.</dd>
        <dt>LOGDIVE_API_HOST</dt>
        <dd>Bind host for <code class="code-inline">logdive-api</code>. Fallback for <code class="code-inline">--host</code>. Default <code class="code-inline">127.0.0.1</code>.</dd>
        <dt>LOGDIVE_API_CORS_ORIGINS</dt>
        <dd>Allowed CORS origins. Comma-separated list or <code class="code-inline">*</code>. Default: disabled.</dd>
        <dt>NO_COLOR</dt>
        <dd>Suppress ANSI colour in <code class="code-inline">logdive query</code> output when set.</dd>
      </dl>
    </section>

    <!-- DOCKER -->
    <section id="docker">
      <h2>Docker</h2>
      <p>Multi-arch images (linux/amd64 and linux/arm64) published to GHCR on every merge to main and every version tag.</p>
      <CodeBlock id="dk-1">
<span class="cm"># Start the API server</span>
<span class="prompt">$</span> docker volume create logdive-data
<span class="prompt">$</span> docker run -d \
    --name logdive \
    -v logdive-data:/data \
    -p 4000:4000 \
    ghcr.io/aryagorjipour/logdive:{CURRENT_VERSION}
      </CodeBlock>
      <CodeBlock id="dk-2">
<span class="cm"># Ingest with the CLI against the same volume</span>
<span class="prompt">$</span> docker run --rm \
    -v logdive-data:/data \
    -v /path/to/your/logs:/logs:ro \
    --entrypoint logdive \
    ghcr.io/aryagorjipour/logdive:{CURRENT_VERSION} \
    ingest --file /logs/app.log --tag production
      </CodeBlock>
      <p>Default entrypoint is <code class="code-inline">logdive-api</code>. The image pre-sets <code class="code-inline">LOGDIVE_DB=/data/index.db</code> and <code class="code-inline">LOGDIVE_API_HOST=0.0.0.0</code>. HEALTHCHECK on <code class="code-inline">GET /version</code>.</p>
    </section>

    <!-- ARCHITECTURE -->
    <section id="architecture">
      <h2>Architecture</h2>
      <p>A Cargo workspace with three crates. <code class="code-inline">logdive-core</code> is publishable to crates.io as a standalone library.</p>
      <CodeBlock id="arch-tree-docs">
logdive/
├── <span class="kw">logdive-core</span>     <span class="cm">// library: parsers, indexer, query AST+parser, executor</span>
├── <span class="kw">logdive</span>          <span class="cm">// CLI binary — clap, follow mode, render, progress</span>
└── <span class="kw">logdive-api</span>      <span class="cm">// HTTP server — axum, read-only, tokio</span>
      </CodeBlock>
      <h3>Storage model</h3>
      <p>Hybrid schema: <code class="code-inline">timestamp</code>, <code class="code-inline">level</code>, <code class="code-inline">message</code>, <code class="code-inline">tag</code> are indexed columns. Everything else is stored in a <code class="code-inline">fields TEXT</code> JSON blob and queried at read time via SQLite's <code class="code-inline">json_extract()</code>. Deduplication uses a <code class="code-inline">raw_hash UNIQUE</code> column with blake3 hashes and <code class="code-inline">INSERT OR IGNORE</code>.</p>
      <h3>Why SQLite</h3>
      <p>Zero infrastructure. A single file, transactional, with a query planner that handles indexes, joins, and aggregates. The interesting work is the query parser and the storage schema; SQLite handles the rest.</p>
      <h3>Why Rust</h3>
      <p>Parsing log lines at 200k/s with near-zero GC budget. The ingest path is where Rust earns its place. The query path is mostly SQL.</p>
    </section>

  </main>
</div>

<script>
  const docsNav = document.querySelector('.docs-nav');
  if (docsNav) {
    const links = Array.from(docsNav.querySelectorAll<HTMLAnchorElement>('a[href^="#"]'));
    const sections = links
      .map(a => document.getElementById(a.getAttribute('href')!.slice(1)))
      .filter((s): s is HTMLElement => s !== null);

    const observer = new IntersectionObserver(
      entries => {
        entries.forEach(entry => {
          if (entry.isIntersecting) {
            const id = entry.target.id;
            links.forEach(a => a.classList.toggle('is-active', a.getAttribute('href') === `#${id}`));
          }
        });
      },
      { rootMargin: '-80px 0px -70% 0px', threshold: 0 }
    );
    sections.forEach(s => observer.observe(s));
  }
</script>

</Base>
```

- [ ] **Step 2: Verify build**

```bash
cd landing && bun run build
```

Expected: `dist/logdive/docs/index.html` exists, no errors.

- [ ] **Step 3: Smoke test**

```bash
cd landing && bun run preview
```

Open `http://localhost:4321/logdive/docs` — verify sidebar scroll-spy, all sections visible, copy buttons on code blocks work.

- [ ] **Step 4: Commit**

```bash
cd /home/arysmart/Projects/Rust/logdive
git add landing/src/pages/docs.astro
git commit -m "feat(landing): implement docs page (README-accurate, v0.2.1)"
```

---

## Task 9: About page (`about.astro`)

**Files:**
- Create: `landing/src/pages/about.astro`

- [ ] **Step 1: Create `landing/src/pages/about.astro`**

```astro
---
// landing/src/pages/about.astro
import Base from '@layouts/Base.astro';
---
<Base
  title="About — logdive"
  description="Why logdive exists, what it isn't, and who built it."
  activeNav="about"
>
<main>

  <section class="tight">
    <div class="wrap">
      <span class="eyebrow">About</span>
      <h1>Why logdive exists<span style="color: var(--accent);">.</span></h1>
    </div>
  </section>

  <section class="tight">
    <div class="wrap">
      <div class="two-col">

        <div class="prose">
          <p>Every backend engineer has hit the same wall. Something went wrong in production three hours ago, and now there's a 2&thinsp;GB log file on a box somewhere, and you need to know which requests failed and what they had in common.</p>
          <p>The options have always been a spectrum, and the spectrum has always been bad. On one end: <code class="code-inline">grep</code> piped into <code class="code-inline">jq</code> piped into <code class="code-inline">awk</code>, a shell incantation you rebuild from scratch every incident. On the other end: a full observability stack — Loki, Datadog, an Elastic cluster — with monthly bills, ingestion limits, and an entire infrastructure surface you don't want to maintain for a side project, a small team, or a single VPS.</p>
          <p>logdive sits in that gap. It's the smallest tool that gives you indexes, time ranges, and a real query language, without asking you to provision anything. You install one binary, you point it at a log file, you query it later. That's the whole product.</p>
          <p>Most existing systems start from "we need to ingest at scale" and work backwards into a usability story. logdive starts from the shell pipeline you'd write if you weren't tired, and works forwards — keeping the surface area roughly equivalent to a CLI you already know, but giving it persistence, structured fields, and time semantics.</p>
          <p>If logdive does its job, you stop writing the same five-stage <code class="code-inline">jq</code> filter every time, and you stop paying someone to host the logs from your hobby Postgres instance. That's all it's trying to do.</p>
        </div>

        <aside class="nongoals" id="non-goals">
          <h3>What logdive is <em>not</em></h3>
          <p class="muted" style="font-size: 13px; margin-bottom: var(--space-4);">Framed honestly, so nobody is surprised six months in. None of these are on the v1 roadmap and most are explicit non-goals forever.</p>
          <ul>
            <li>No authentication on the API. Run it on localhost.</li>
            <li>No HTTP ingestion endpoint. logdive reads files and stdin.</li>
            <li>No multi-machine index. One DB, one host.</li>
            <li>No log shipping agents, no sidecar, no daemon.</li>
            <li>No browser UI. The CLI is the UI.</li>
            <li>No hosted version. None planned, ever.</li>
          </ul>
        </aside>

      </div>
    </div>
  </section>

  <section class="tight">
    <div class="wrap">
      <hr style="margin-bottom: var(--space-12);" />
      <div style="max-width: 56ch;">
        <span class="eyebrow">Built by</span>
        <h2 style="font-size: 28px; margin-bottom: var(--space-4);">Arya Gorjipour</h2>
        <p class="muted" style="font-size: 16px;">
          logdive is built and maintained by <a class="link-underline" href="https://github.com/Aryagorjipour" rel="noopener">@Aryagorjipour</a>, an Iranian Rust engineer. Written in Rust 2024 edition, MSRV 1.85. No company, no funding, no roadmap deck — just an engineer who got tired of writing the same <code class="code-inline">jq</code> filter twice.
        </p>
      </div>
    </div>
  </section>

</main>
</Base>
```

- [ ] **Step 2: Verify build**

```bash
cd landing && bun run build
```

Expected: `dist/logdive/about/index.html` exists, no errors.

- [ ] **Step 3: Smoke test**

```bash
cd landing && bun run preview
```

Open `http://localhost:4321/logdive/about` — verify two-column layout, non-goals list, built-by section.

- [ ] **Step 4: Commit**

```bash
cd /home/arysmart/Projects/Rust/logdive
git add landing/src/pages/about.astro
git commit -m "feat(landing): implement about page"
```

---

## Task 10: GitHub Actions deploy workflow

**Files:**
- Create: `.github/workflows/landing.yml`

- [ ] **Step 1: Create `.github/workflows/landing.yml`**

```yaml
name: Deploy landing page

on:
  push:
    branches: [main]
    paths: ['landing/**']
  workflow_dispatch:

permissions:
  contents: read
  pages: write
  id-token: write

concurrency:
  group: pages
  cancel-in-progress: false

jobs:
  build:
    runs-on: ubuntu-latest
    steps:
      - name: Checkout
        uses: actions/checkout@v4

      - name: Setup Bun
        uses: oven-sh/setup-bun@v2
        with:
          bun-version: latest

      - name: Install dependencies
        working-directory: landing
        run: bun install --frozen-lockfile

      - name: Build
        working-directory: landing
        run: bun run build

      - name: Upload Pages artifact
        uses: actions/upload-pages-artifact@v3
        with:
          path: landing/dist

  deploy:
    needs: build
    runs-on: ubuntu-latest
    environment:
      name: github-pages
      url: ${{ steps.deployment.outputs.page_url }}
    steps:
      - name: Deploy to GitHub Pages
        id: deployment
        uses: actions/deploy-pages@v4
```

- [ ] **Step 2: Verify workflow file is valid YAML**

```bash
python3 -c "import yaml; yaml.safe_load(open('.github/workflows/landing.yml'))" && echo "valid"
```

Expected: `valid`

- [ ] **Step 3: Commit**

```bash
cd /home/arysmart/Projects/Rust/logdive
git add .github/workflows/landing.yml
git commit -m "ci: add GitHub Actions workflow to deploy landing page"
```

---

## Task 11: Final integration smoke test + polish

**No new files.**

- [ ] **Step 1: Full build**

```bash
cd landing && bun run build
```

Expected: no warnings, no errors.

- [ ] **Step 2: Check all expected output files exist**

```bash
ls landing/dist/logdive/
ls landing/dist/logdive/docs/
ls landing/dist/logdive/about/
```

Expected: `index.html` in each directory.

- [ ] **Step 3: Start preview and manually verify all pages**

```bash
cd landing && bun run preview
```

Check each URL:
- `http://localhost:4321/logdive/` — hero renders, theme toggle works, all 8 sections present, install tab switches work, roadmap lanes show real data
- `http://localhost:4321/logdive/docs` — sidebar sticky, scroll-spy highlights correct link, all copy buttons work, version number is 0.2.1 throughout
- `http://localhost:4321/logdive/about` — two-column layout, non-goals list with × bullets, built-by section

Check dark mode on each page: click toggle, verify `[data-theme="dark"]` applied, refresh — should persist.

Check mobile (resize to 375px): nav collapses, hamburger opens it, stat grid stacks.

- [ ] **Step 4: Fix any regressions found above**

If any, fix inline and commit with `fix(landing): <description>`.

- [ ] **Step 5: Final commit**

```bash
cd /home/arysmart/Projects/Rust/logdive
git add -A
git commit -m "feat(landing): complete landing page implementation v0.2.1"
```

---

## Self-Review Against Spec

| Spec section | Covered by |
|---|---|
| Astro 5 + Bun + TypeScript | Tasks 1, 2, 3 |
| CSS design system (tokens, dark mode, typography) | Task 2 |
| `src/data/roadmap.ts` typed data | Task 3 |
| Base layout, Header, Footer components | Task 4 |
| CodeBlock + Tabs interactive islands | Task 5 |
| TerminalPreview + RoadmapStatus (build-time) | Task 6 |
| Home page — all 8 sections | Task 7 |
| Docs page — README-accurate, v0.2.1 flags only | Task 8 |
| About page | Task 9 |
| GitHub Actions Bun deploy workflow | Task 10 |
| `base: '/logdive'` path prefix throughout | Tasks 1, 4 |
| Dark mode persistence via localStorage | Task 4 (Header script) |
| Mobile nav | Task 4 (Header script) |
| Docs scroll-spy via IntersectionObserver | Task 8 |
| Copy-to-clipboard with fallback | Task 5 |
| Roadmap: v0.2.1, v0.2.0, v0.1.0 shipped data | Task 3 |
| Version bump 0.2.0→0.2.1 throughout | Tasks 3, 7, 8 |
| `prefers-reduced-motion` respected | Task 2 (global.css) |
| Accessibility: semantic HTML, aria-label, aria-expanded | Tasks 4, 6, 7, 8, 9 |
