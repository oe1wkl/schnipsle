# ADR-0001: Record architecture decisions

- Status: Accepted
- Date: 2026-10-13
- Deciders: Willi Kraml (OE1WKL)

## Context

This is an open-source project that will run for years and, ideally, attract contributors. Decisions made now (stack, file format, license) will be questioned later. Without a record, the reasons get lost and decisions get re-argued or silently broken.

## Decision

Architectural decisions are recorded as ADRs in `docs/adr/`, in MADR style, one Markdown file per decision, kept in the repository and reviewed through pull requests like code. The rules are in [README.md](README.md).

## Consequences

- Positive: reasons are traceable; newcomers can read why things are as they are.
- Negative: small overhead per decision.
- Follow-up: the high-level design (`docs/design/hld.md`) links to the ADRs instead of repeating them.
