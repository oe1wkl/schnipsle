# Schnipsle manual

The manual is a vault of its own: each part is a note in [`notes/schnipsle-manual/`](notes/schnipsle-manual/), carrying the tag `#schnipsle_manual`. Schnipsle adds these notes to the vault it makes at its first start, and **Help › Manual** opens them (ADR-0048, ADR-0051).

In reading order:

1. [Schnipsle manual](notes/schnipsle-manual/schnipsle-manual.md), the title page
2. [Getting started](notes/schnipsle-manual/getting-started.md)
3. [Query language](notes/schnipsle-manual/query-language.md)
4. [Music](notes/schnipsle-manual/music.md)
5. [Keyboard and VoiceOver](notes/schnipsle-manual/keyboard.md)

Writing a manual note: keep its `id` and `created`, change `modified` with the text, link other parts with relative links (`[Music](music.md)`), and end it with the tag line `#schnipsle_manual`. Pictures go into `attachments/` as format.md §6 says. A test in `crates/core/src/manual.rs` checks all of this.
