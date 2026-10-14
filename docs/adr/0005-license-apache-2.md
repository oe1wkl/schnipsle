# ADR-0005: License: Apache-2.0

- Status: Accepted
- Date: 2026-10-13
- Deciders: Willi Kraml
- Related: N-07, ADR-0010

## Context

The project is open source on GitHub and must be distributable through Apple's App Store. GPL-licensed software conflicts with App Store terms. The main dependencies use permissive licenses: Tauri (MIT/Apache-2.0), CodeMirror (MIT), comrak (BSD-2-Clause), SQLite (public domain), rusqlite (MIT).

## Decision

The project is licensed under **Apache-2.0**. Contributions are accepted under the same license (inbound = outbound), confirmed by a Developer Certificate of Origin sign-off (`git commit -s`), no CLA.

## Options considered

| Option | Pros | Cons |
| --- | --- | --- |
| Apache-2.0 | Permissive, explicit patent grant, App Store compatible, matches dependencies | Forks may close their changes |
| MIT | Simplest | No patent grant |
| MPL-2.0 | Changes to our files stay open, App Store compatible | Less familiar, slightly more friction |
| GPL-3.0 | Strong copyleft | Conflicts with App Store distribution |

## Consequences

- `LICENSE` and `NOTICE` files in the repository root; SPDX headers (`// SPDX-License-Identifier: Apache-2.0`) in source files.
- `cargo deny` and a license checker for npm block dependencies with incompatible licenses (GPL, AGPL, SSPL) in CI.
- **Pandoc is GPL-2.0-or-later.** It is never linked or bundled into the iOS app. On desktop it is invoked as a separate, user-installed or separately shipped executable. Before the first desktop release this arrangement gets a short legal check; see ADR-0010.
