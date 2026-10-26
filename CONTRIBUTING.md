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

For a change in the web app, also run `pnpm --dir apps/web typecheck` and `pnpm --dir apps/web test`. `just --list` shows the phase demos. `just demo-4` needs Postgres and `just demo-5` needs Typesense, both started by `just up`.

## Project rules

- UI text is English, including errors and names shown in the command palette. Catalog values and attribute labels stay in the language of the data.
- Do not commit secrets, `.env`, or customer data. Demo and test catalogs are synthetic.
- A performance claim needs a note in [docs/measurements/](docs/measurements/) with the git commit, hardware, data size, the command, and the numbers. Do not invent figures in the README or the product.
- `crates/model` and `crates/rules` stay free of I/O. `crates/rules` must compile to `wasm32-unknown-unknown`.
- An agent acts for a person. Writes from the agent are proposals.

## License

Braian is licensed under the Apache License, Version 2.0. See [LICENSE](LICENSE), [NOTICE](NOTICE), and the [License](README.md#license) section of the README.

## Developer Certificate of Origin

Contributions require a [Developer Certificate of Origin](https://developercertificate.org/) sign-off. Sign every commit with `git commit -s`, which adds:

```
Signed-off-by: Your Name <you@example.com>
```

By adding that `Signed-off-by` line you certify the Developer Certificate of Origin 1.1:

```
Developer Certificate of Origin
Version 1.1

Copyright (C) 2004, 2006 The Linux Foundation and its contributors.

Everyone is permitted to copy and distribute verbatim copies of this
license document, but changing it is not allowed.

Developer's Certificate of Origin 1.1

By making a contribution to this project, I certify that:

(a) The contribution was created in whole or in part by me and I
    have the right to submit it under the open source license
    indicated in the file; or

(b) The contribution is based upon previous work that, to the best
    of my knowledge, is covered under an appropriate open source
    license and I have the right under that license to submit that
    work with modifications, whether created in whole or in part
    by me, under the same open source license (unless I am
    permitted to submit under a different license), as indicated
    in the file; or

(c) The contribution was provided directly to me by some other
    person who certified (a), (b) or (c) and I have not modified
    it.

(d) I understand and agree that this project and the contribution
    are public and that a record of the contribution (including all
    personal information I submit with it, including my sign-off) is
    maintained indefinitely and may be redistributed consistent with
    this project or the open source license(s) involved.
```
