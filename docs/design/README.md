# High-level design

The high-level design is maintained as a living document while the foundations are laid (milestone M0):

**[Markdown Notes App – High-Level Design](https://claude.ai/code/artifact/4099e0ee-346f-428e-80a7-60cabfe47ab6)**

It covers purpose and scope, requirements (F-01 … F-18, N-01 … N-07), data model, architecture, technology choices, import and export, virtual folders and the query language, version history, music notation, the development environment and method, and the roadmap.

At the end of every milestone a snapshot is saved as [`hld.md`](hld.md) in this folder (latest: end of M0), so the repository always holds a reviewed snapshot. Decisions themselves live in [`../adr/`](../adr/README.md) and the on-disk format in [`../format.md`](../format.md); where the design document and an ADR disagree, the ADR wins.
