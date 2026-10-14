# ADR-0002: Target platforms and priorities

- Status: Accepted
- Date: 2026-10-13
- Deciders: Willi Kraml
- Related: N-05

## Context

The primary user works on a Mac and an iPhone. Notes must be available and editable on both from version 1. Open-source users are likely to ask for Linux and Android; Windows is less important for the initial audience.

## Decision

| Platform | Priority | Meaning |
| --- | --- | --- |
| macOS | Must | Supported in v1 |
| iOS / iPadOS | Must | Supported in v1 |
| Linux | Desirable | No design decision may block it; CI builds it from M1 |
| Android | Desirable | No design decision may block it; first build after v1 |
| Windows | Nice to have | Not blocked, not actively tested before v1 |

## Consequences

- The stack must cover macOS and iOS from one code base and must not rule out Linux and Android (see ADR-0003).
- Nothing may depend on desktop-only abilities without an iOS fallback, e.g. starting external programs (see ADR-0010).
- iOS sandboxing rules shape file access (see ADR-0004, ADR-0006).

## Amendments

- 2026-10-14: Minimum iOS version is **16.0** (`bundle.iOS.minimumSystemVersion`). Xcode 27 no longer builds for iOS 14, and the UI is built for Safari 16 (`vite.config.ts`). iOS 16 runs on iPhone 8 and later.
