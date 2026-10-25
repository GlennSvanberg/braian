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

Output is `apps/landing/dist/`: `index.html`, hashed CSS and a small theme script, `brand/` (favicon and logo), `og.png`, and `robots.txt`. The page is prerendered, so the HTML already contains the copy. The JavaScript that ships (`src/client.ts`) is the light/dark toggle, the hero demo player, and the flow diagram animation.

Performance figures on the page come from `src/data/measurements.ts`. Update that file when a run is remeasured. All figures share one footnote with the hardware, so add a new footnote before publishing a figure from other hardware.

The hero shows an animated mock of the demo workspace (`src/demo/`). Its scenes are listed in `src/demo/timeline.ts`; markup marks what each scene shows with `data-show`, `data-sel`, and `data-flash`. When a navigation recording exists, replace `<HeroDemo />` inside the `data-video-slot` figure in `src/App.tsx`.

Regenerate the social image (needs Chrome) with `pnpm --dir apps/landing og`. The build does not require a browser.

## Deploy

Copy the contents of `dist/` to `/var/www/braian` on the VPS. Caddy `file_server` serves that directory.
