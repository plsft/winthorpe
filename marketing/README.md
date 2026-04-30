# Winthorpe — marketing site

Static marketing site for Winthorpe. Built with Vite, Tailwind v4, and
Alpine.js. Designed to deploy to Cloudflare Pages.

## Stack

- **Vite 7** — multi-page build, dev server, HMR.
- **Tailwind v4** — via `@tailwindcss/vite`. No `tailwind.config.js`; theme
  tokens live in `src/styles/app.css`.
- **Alpine.js** — small interactions (mobile menu, modal, easter egg).
- **No analytics, no cookies, no third-party scripts.** Matches what the
  product itself promises.

## Structure

```
marketing/
├── index.html                     # Home (hero + pillars + workflow + CTA)
├── features/index.html
├── how-to-use/index.html          # 12-step walkthrough
├── download/index.html
├── contact/index.html
├── legal/
│   ├── privacy/index.html
│   └── terms/index.html
├── src/
│   ├── partials/
│   │   ├── header.html            # injected via <% header %>
│   │   └── footer.html            # injected via <% footer %>
│   ├── styles/app.css             # Tailwind + brand tokens
│   ├── main.js                    # Alpine bootstrap + helpers
│   └── easter-egg.js              # Trading Places quotes (Konami / "winthorpe")
├── public/
│   ├── favicon.svg
│   ├── hero-screenshot.png
│   └── _headers                   # Cloudflare Pages headers
├── vite.config.js                 # MPA config + partials plugin
└── package.json
```

## Partials

The custom Vite plugin in `vite.config.js` rewrites `<% name %>` tokens into
the contents of `src/partials/<name>.html`. Use this anywhere in any page:

```html
<body>
  <% header %>
  <main>...</main>
  <% footer %>
</body>
```

To add a new partial: drop `src/partials/foo.html`, then reference it as
`<% foo %>`. Tokens that don't match a partial are passed through unchanged.

## Adding a new page

Create `your-page/index.html` with the same skeleton as the existing pages
(reference `<% header %>` and `<% footer %>`). Vite's `collectPages` walker
in `vite.config.js` picks it up automatically — no input list to maintain.

For navigation, add an entry in the `template x-for` array inside
`src/partials/header.html`.

## Local development

```bash
bun install
bun run dev          # http://localhost:5173
```

## Build

```bash
bun run build        # → dist/
bun run preview      # serve dist/ locally on :4173
```

## Deploy to Cloudflare Pages

### Manual one-shot

```bash
bun run deploy
```

(Requires `wrangler login` once. Project name `winthorpe-marketing` —
change in `package.json` if you want a different one.)

### Connect to git (recommended)

In the Cloudflare dashboard → Workers & Pages → Create → Connect to git:

- **Repository:** `plsft/winthorpe`
- **Root directory:** `marketing`
- **Build command:** `bun run build`
- **Build output directory:** `dist`
- **Node version:** 22+ (Cloudflare's default)

Push to `main` → auto-deploys.

## Easter egg

Press the Konami code (`↑ ↑ ↓ ↓ ← → ← → B A`) anywhere on the site, OR type
the word `winthorpe`. A modal surfaces a random Trading Places quote.

The product is named for Louis Winthorpe III. The film is from 1983 — the
year before the original Macintosh shipped, in case you were wondering why
this is a Windows-first IDE with a Trading Places reference.
