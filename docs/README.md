# imulup docs

The documentation site, built with [Starlight](https://starlight.astro.build/). Pages live in
`src/content/docs/`; the sidebar is defined in `astro.config.mjs`.

```sh
npm ci
npm run dev      # http://localhost:4321/imulup/
npm run build    # static site in dist/
```

Pushes to `main` that touch `docs/` deploy to GitHub Pages (`.github/workflows/docs.yml`).
