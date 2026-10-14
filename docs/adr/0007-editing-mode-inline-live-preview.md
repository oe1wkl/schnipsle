# ADR-0007: Editing mode: inline live preview with source toggle

- Status: Accepted
- Date: 2026-10-13
- Deciders: Willi Kraml
- Related: F-01, ADR-0003

## Context

Users want to see formatted notes while writing, but the Markdown must stay visible and editable. A side-by-side preview needs screen width that an iPhone does not have.

## Decision

- **Default:** inline live preview in CodeMirror 6. Formatting is rendered in place (headings, emphasis, lists, task checkboxes, images, links); Markdown markers are shown only on the line or selection being edited.
- **Toggle:** a plain-source mode with syntax highlighting only, per note or globally.
- The file on disk is always exactly what the user typed; the editor never rewrites Markdown on its own (no auto-normalization of list markers, emphasis style, etc.).

## Consequences

- One editor component on all platforms; the mode is a set of CodeMirror extensions.
- Rendering in place is more work than a separate preview; images, tables and checkboxes need custom widgets.
- Read-only view (e.g. trash, history) uses the same renderer as export to HTML.
