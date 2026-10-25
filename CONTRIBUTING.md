# Contributing

Thanks for looking at Braian. Issues and pull requests are welcome.

## Setup

Follow the quick start in [README.md](README.md). Ports, Windows notes, and the replication slot are in [docs/dev.md](docs/dev.md).

```bash
cp .env.example .env
just up
pnpm install
just dev
```

## Before a pull request

```bash
just check
just test
```

For a change in the web app, also run `pnpm --dir apps/web typecheck` and `pnpm --dir apps/web test`. `just --list` shows the phase demos. `just demo-4` needs Redpanda and `just demo-5` needs Typesense, both started by `just up`.

## Project rules

- UI text is English, including errors and names shown in the command palette. Catalog values and attribute labels stay in the language of the data.
- Do not commit secrets, `.env`, or customer data. Demo and test catalogs are synthetic.
- A performance claim needs a note in [docs/measurements/](docs/measurements/) with the git commit, hardware, data size, the command, and the numbers. Do not invent figures in the README or the product.
- `crates/model` and `crates/rules` stay free of I/O. `crates/rules` must compile to `wasm32-unknown-unknown`.
- An agent acts for a person. Writes from the agent are proposals.

## License

By contributing, you agree that your contribution is licensed under the [MIT License](LICENSE).
