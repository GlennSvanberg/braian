# Object sizes per tier

Measured from the phase 1 working tree on top of `3d79b8b` (the implementation was not committed yet).

- Hardware: Intel Core Ultra 7 155U, 32 GB RAM, KIOXIA NVMe 1 TB. Local process, not Docker. `rustc 1.93.1`.
- Data: 100 objects per tier, seed 1. Light = 50 attributes and 3 languages, medium = 300 and 10, heavy = 2000 and 20. 30% of attributes are language-dependent. Each object fills 60% (light 30, medium 180, heavy 1200), including the key. Text values are 32 ASCII characters, so real product text will be larger.
- Command: `cargo run -p braian-synth --release -- sizes --count 100` (`just measure-sizes`).
- Percentiles are nearest-rank: sorted samples, index `ceil(p/100 * n) - 1`.

Bytes are compact JSON. Human-key sizes change only the keys inside `values`; `id`, `type_id` and `groups` stay UUIDs. The parts are the value documents a per-language storage layout would write: one language-independent object, plus one object per language. They do not include the object row.

## Light

| | mean | p50 | p95 | max |
| --- | ---: | ---: | ---: | ---: |
| Full object, UUID keys | 2 670 | 2 678 | 2 853 | 2 934 |
| Full object, human keys | 1 738 | 1 746 | 1 921 | 2 002 |
| Language-independent part | 1 309 | 1 302 | 1 471 | 1 665 |
| One language part | 533 | 530 | 685 | 789 |
| All language parts | 1 599 | 1 590 | 2 040 | 2 344 |
| Validation, microseconds | 12 | 12 | 16 | 43 |

Definition set: 16 898 bytes.

## Medium

| | mean | p50 | p95 | max |
| --- | ---: | ---: | ---: | ---: |
| Full object, UUID keys | 25 217 | 25 288 | 27 155 | 27 896 |
| Full object, human keys | 19 635 | 19 706 | 21 573 | 22 314 |
| Language-independent part | 7 707 | 7 702 | 8 172 | 8 537 |
| One language part | 3 301 | 3 314 | 3 670 | 3 956 |
| All language parts | 33 010 | 33 104 | 36 313 | 38 677 |
| Validation, microseconds | 109 | 95 | 204 | 259 |

Definition set: 92 179 bytes.

## Heavy

| | mean | p50 | p95 | max |
| --- | ---: | ---: | ---: | ---: |
| Full object, UUID keys | 268 944 | 269 039 | 282 083 | 285 340 |
| Full object, human keys | 231 742 | 231 837 | 244 881 | 248 138 |
| Language-independent part | 51 521 | 51 511 | 52 629 | 53 339 |
| One language part | 22 009 | 22 005 | 23 171 | 23 914 |
| All language parts | 440 190 | 439 544 | 461 951 | 472 471 |
| Validation, microseconds | 1 492 | 1 288 | 2 551 | 3 725 |

Definition set: 604 019 bytes.

## What this decides

A heavy object is about 263 KB with UUID value keys, not several megabytes, at this text length. Human keys save about 37 KB (14%), which is the key text itself: 1 200 filled attributes times roughly 31 bytes.

Splitting values by language makes a single-attribute change smaller. A localized edit rewrites one part of about 22 KB instead of the whole 263 KB document, and a language-independent edit rewrites about 50 KB. The parts are larger in total (about 480 KB) than one document, because each language part repeats the attribute ids. That is input to spike 1, which still has to measure WAL bytes. It does not choose the storage form by itself.

Validation of one heavy object takes 1.5 ms on average and 2.6 ms at p95, on one core, with no database. That is about 670 objects per second at the mean. The bulk target of 5 000 heavy objects per second cannot be met on a single core if every object is validated this way.
