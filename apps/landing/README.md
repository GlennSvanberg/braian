# Landing page

Static site for [braian.io](https://braian.io).

Colors, type, radii, and component chrome come from the web app: `apps/web/src/styles.css` and `DESIGN.md`. This app imports that stylesheet. It does not keep a second copy of the tokens. The mark and favicon come from `apps/web/public/brand`.

## Develop

From the repo root, after `pnpm install`:

```bash
pnpm --dir apps/landing dev
```

The dev server is http://127.0.0.1:4173.

## Build

```bash
pnpm --dir apps/landing build
```

`just landing` runs the same build.

Output is `apps/landing/dist/`: `index.html`, hashed CSS and a small theme script, `brand/` (favicon and logo), `og.png`, and `robots.txt`. The page is prerendered, so the HTML already contains the copy. The JavaScript that ships is the light/dark toggle.

Regenerate the social image (needs Chrome) with `pnpm --dir apps/landing og`. The build does not require a browser.

## Deploy

Copy the contents of `dist/` to `/var/www/braian` on the VPS. Caddy `file_server` serves that directory.
