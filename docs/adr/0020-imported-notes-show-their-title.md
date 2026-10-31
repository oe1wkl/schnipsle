# ADR-0020: Imported notes show the title they are named by

- Status: Accepted
- Date: 2026-10-31
- Deciders: Willi Kraml
- Related: F-05, ADR-0012, ADR-0019, [format.md §5.2](../format.md#52-title), [§12.1](../format.md#121-imported-notes)

## Context

A note's title is its leading level-1 heading, otherwise its first line (format.md §5.2). Some sources name a note outside its text: an HTML page in `<title>`, a Markdown file of another editor in its front matter `title`, a ChordPro file in `{title}`. The import names the file after that title, but the app then showed the first line instead, e.g. "Parts list and the [photo](…) again, stored once." for a note named `antenna-tuner.md`, and renamed the file to that line after the first edit (§3).

format.md §12.1 rule 3 allowed no change to the text beyond the conversion, so an HTML page's `<title>` and a Markdown file's front-matter `title` had no place in the note's text.

## Decision

Where an imported note's text does not start with a level-1 heading but the source names the note, the import puts that name as `# <title>` at the top of the body, after the front matter. This applies to the HTML `<title>`, a Markdown file's front-matter `title` (which stays in the front matter) and the ChordPro `{title}`. A leading level-1 heading wins over these names. A note the source does not name keeps its text as it is and is named by its first line, as in §5.2.

## Options considered

| Option | Pros | Cons |
| --- | --- | --- |
| Heading from the source's title (chosen) | The note shows and keeps the title it was named by; plain Markdown, readable anywhere | One line added to the text; the title is there twice in a Markdown file with front matter |
| Front-matter `title` as a title rule in §5.2 | No change to the text | Every tool reading the vault needs the rule; does not help HTML or ChordPro |
| Leave the text, name the file by the first line | No change at all | The source's title is lost; a Markdown file's front-matter title is ignored |

## Consequences

- Positive: imported notes look in the list as they did in the source, and editing them does not rename their files.
- Negative: the text of an imported Markdown file differs from the original by one heading.
- Follow-up work: format.md 0.12 adds §12.1 rule 7 and mentions it in rule 3; golden files of the HTML and Markdown imports.
