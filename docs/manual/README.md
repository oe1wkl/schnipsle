# Schnipsle manual

The manual is a vault of its own: each part is a note in [`notes/schnipsle-manual/`](notes/schnipsle-manual/), carrying the tag `#schnipsle_manual`. Schnipsle adds these notes to the vault it makes at its first start, and **Help › Manual** opens them (ADR-0048, ADR-0051).

In reading order:

1. [Schnipsle manual](notes/schnipsle-manual/schnipsle-manual.md), the title page
2. [Getting started](notes/schnipsle-manual/getting-started.md)
3. [Markdown](notes/schnipsle-manual/markdown.md)
4. [Notes and pictures](notes/schnipsle-manual/notes-and-pictures.md)
5. [Tags and smart folders](notes/schnipsle-manual/tags-and-smart-folders.md)
6. [Query language](notes/schnipsle-manual/query-language.md)
7. [Vaults and files](notes/schnipsle-manual/vaults-and-files.md)
8. [Sync with iCloud](notes/schnipsle-manual/sync-with-icloud.md)
9. [History and trash](notes/schnipsle-manual/history-and-trash.md)
10. [Import](notes/schnipsle-manual/import.md)
11. [Export](notes/schnipsle-manual/export.md)
12. [Music](notes/schnipsle-manual/music.md)
13. [Settings](notes/schnipsle-manual/settings.md)
14. [Keyboard and VoiceOver](notes/schnipsle-manual/keyboard.md)
15. [Example: Pancakes](notes/schnipsle-manual/example-pancakes.md)
16. [Example: Tomato soup](notes/schnipsle-manual/example-tomato-soup.md)
17. [Example: Packing list](notes/schnipsle-manual/example-packing-list.md)
18. [Example: A walk by the lake](notes/schnipsle-manual/example-a-walk-by-the-lake.md)

Writing a manual note: keep its `id` and `created`, change `modified` with the text, link other parts with relative links (`[Music](music.md)`), and end it with the tag line `#schnipsle_manual`. Pictures go into `attachments/` as format.md §6 says, and example notes carry tags under `#schnipsle_manual` (such as `#schnipsle_manual/recipes`) and are listed under "Examples to try". A test in `crates/core/src/manual.rs` checks all of this.
