---
id: 01M8EAJZW039MQ7CNCPJP1R137
created: 2026-11-27T23:00:00+01:00
modified: 2026-11-27T23:00:00+01:00
---

# Markdown

A note in Schnipsle is written in Markdown: plain text with a few signs that say what is a heading, a list, a link or a table. The file stays readable as it is, in any editor, and other programs show it formatted.

Schnipsle shows the text formatted while you write. The line with the cursor shows its Markdown, so you can change it; the other lines show the result. **Source** in the bar under the note shows the whole note as Markdown, and **Preview** goes back.

This page shows everything Schnipsle understands. Each example is first written as Markdown, between two lines of three backticks and in the font of code, and then, where it makes sense, as it looks. The Markdown is CommonMark with the additions of GitHub (tables, tasks, strikethrough, links written as plain addresses), and footnotes.

## Paragraphs and line breaks

Text separated by an empty line is two paragraphs:

```markdown
The first paragraph.

The second paragraph.
```

Lines that follow each other without an empty line belong to one paragraph. In the editor they stay on their own lines; in an export they run together, like this:

```markdown
Roses are red,
violets are blue.
```

To keep a line break in exports too, end the line with a backslash `\` (or with two spaces, which you cannot see):

```markdown
Roses are red,\
violets are blue.
```

Roses are red,\
violets are blue.

## Headings

A line starting with `#` and a space is a heading. More `#` signs make a smaller heading, down to six:

```markdown
# A heading of the first level

## Second level

### Third level

#### Fourth level
```

The first heading of a note is its title, which names the note in the list and in links, and gives its file its name. Any level works for that: a note that starts with `## Shopping` is called "Shopping".

A heading can also be underlined with `===` (first level) or `---` (second level) on the next line:

```markdown
A heading
=========
```

A `#` directly followed by a word, without the space, is a tag, not a heading: see [Tags and smart folders](tags-and-smart-folders.md).

## Emphasis

| Markdown                 | Shows as              |
| ------------------------ | --------------------- |
| `*italic*` or `_italic_` | _italic_              |
| `**bold**` or `__bold__` | **bold**              |
| `***bold and italic***`  | _**bold and italic**_ |
| `~~struck through~~`     | ~~struck through~~    |
| `` `code` ``             | `code`                |

The underscore works only at the edges of a word, so `snake_case_names` stay as they are. Code shows in a font where every letter is as wide as the next, and nothing inside it is formatted: `**not bold**`, `#not_a_tag`.

To show a backtick inside code, put two backticks around it: ``` `` `code` `` ``` shows as `` `code` ``.

## Lists

A line starting with `-`, `*` or `+` and a space is an item of a list. A number with a full stop (`1.`) or a parenthesis (`1)`) starts a numbered list:

```markdown
- bread
- tomatoes
- olive oil

1. Heat the pan.
2. Pour in the batter.
3. Turn it after two minutes.
```

- bread
- tomatoes
- olive oil

1. Heat the pan.
2. Pour in the batter.
3. Turn it after two minutes.

A numbered list counts on from its first number, whatever numbers follow: `1.`, `1.`, `1.` is shown as 1, 2, 3 in exports. Start with `3.` to count from 3.

Indent a line to put it inside the item above it: a list in a list, or a second paragraph of an item. **⌘]** indents a line, **⌘[** outdents it.

```markdown
- Fruit
  - apples
  - pears
- Vegetables
  - leeks
```

- Fruit
  - apples
  - pears
- Vegetables
  - leeks

## Tasks

An item starting with `[ ]` is a task, one with `[x]` is done:

```markdown
- [ ] Buy stamps
- [x] Water the plants
```

- [ ] Buy stamps
- [x] Water the plants

Tap or click the box to tick a task; Schnipsle writes the `x` for you. The search finds notes with tasks with `has:task` ([Query language](query-language.md)).

## Quotes

A line starting with `>` is a quote. Quotes can hold every other kind of text, and quotes inside quotes:

```markdown
> Write it down before you forget it.
>
> > A quote in a quote.
```

> Write it down before you forget it.
>
> > A quote in a quote.

## Code blocks

Three backticks on a line of their own start a block of code, and three more end it. Everything in between is shown as it is written, in the code font, and is never formatted or read for tags:

````markdown
```
for n in 1..=3 {
    println!("{n}");
}
```
````

A word after the first backticks names the language, such as ` ```python `. Three such words make Schnipsle do more:

- ` ```abc ` is a score, which Schnipsle engraves and plays;
- ` ```chordpro ` is a song sheet with chords over the lyrics;

both described in [Music](music.md). The third, ` ```mermaid `, is meant for diagrams; Schnipsle shows it as code for now.

Lines indented by four spaces are a code block too, but backticks are easier to see.

## Links

```markdown
[Schnipsle on GitHub](https://github.com/oe1wkl/schnipsle)
[Schnipsle on GitHub](https://github.com/oe1wkl/schnipsle "Its source code")
<https://github.com/oe1wkl/schnipsle>
https://github.com/oe1wkl/schnipsle
<someone@example.com>
```

The text in square brackets is what you see, the address in parentheses is where the link goes, and a text in quotes after it is shown when you point at the link. An address in angle brackets, or written out with `https://` or `www.`, is a link by itself.

To follow a link, tap it on the iPhone, or ⌘-click it on the Mac (Ctrl-click elsewhere). Web pages open in your browser and mail addresses in your mail app. On the iPhone, only links to other notes open so far.

A link can also name its address further down, which keeps long addresses out of the text:

```markdown
The [manual][docs] is on GitHub.

[docs]: https://github.com/oe1wkl/schnipsle/tree/main/docs/manual
```

### Links to other notes

A link to another note is a link to its file, written relative to this note's file:

```markdown
[Music](music.md)
[A note one folder up](../shopping.md)
[A note in a folder below](recipes/pancakes.md)
```

This manual links its parts like that, for example to [Getting started](getting-started.md). A link to a note opens that note in Schnipsle. The file names Schnipsle gives have no spaces; for another file whose name has some, write each space as `%20`: `[Notes](my%20notes.md)`.

Links by title, `[[Shopping list]]` or `[[Shopping list|the list]]`, are kept as you write them, and an export links them to the note when it holds that note too; but Schnipsle does not open them yet.

## Pictures

A picture is a link with `!` in front. The text in square brackets describes the picture, for those who cannot see it and where the picture is missing:

```markdown
![The lake in the morning](../attachments/4d/4d…png)
```

You hardly need to write this yourself: paste or drop a picture into a note, or use **Image**, and Schnipsle stores the picture in the vault and writes the link. [Notes and pictures](notes-and-pictures.md) tells more.

A link to a file that is not a picture, such as a PDF, is written without the `!`. A picture on the web, such as `![Logo](https://example.com/logo.png)`, is kept as written, but Schnipsle shows only pictures in the vault: the editor shows its description instead, and only an HTML export shows the picture itself.

## Tables

```markdown
| Instrument | Strings | Lowest note |
| :--------- | ------: | :---------: |
| Violin     |       4 |      G      |
| Guitar     |       6 |      E      |
| Bass       |       4 |      E      |
```

| Instrument | Strings | Lowest note |
| :--------- | ------: | :---------: |
| Violin     |       4 |      G      |
| Guitar     |       6 |      E      |
| Bass       |       4 |      E      |

The first line names the columns, the second separates them from the rest. Colons in the second line align a column: `:---` to the left, `---:` to the right, `:---:` in the middle. The columns need not line up in the text; Schnipsle lines them up when it shows the table. A table that is too wide for the screen scrolls sideways.

Cells can hold emphasis, code and links, but not lists or line breaks. A `|` in a cell is written `\|`.

## Lines

Three or more `-`, `*` or `_` on a line of their own draw a line across the page. Put an empty line above `---`, or it underlines the text above as a heading.

```markdown
Above the line.

---

Below the line.
```

## Footnotes

```markdown
Bake the bread for forty minutes.[^1]

[^1]: In a fan oven, five minutes less.
```

Bake the bread for forty minutes.[^1]

The note of a footnote can stand anywhere in the note; in the editor it stays where you wrote it. Exports number the footnotes in order: Word and ODT put them at the bottom of the page, the other formats at the end of the note.

[^1]: In a fan oven, five minutes less.

## Formulas

Text between dollar signs is a formula in TeX: `$E = mc^2$` in a line, or `$$…$$` for a formula of its own. Schnipsle keeps it as written and does not typeset it yet; exports show it as code.

## Signs that should stay signs

A backslash in front of a sign keeps it from doing anything: `\*not italic\*`, `\# not a heading`, `\#not_a_tag`, `1986\. A good year.` This works for these signs:

```text
\ ` * _ { } [ ] ( ) < > # + - . ! | ~ $
```

## HTML

Markdown may hold HTML, such as `<sub>2</sub>` or `<kbd>⌘</kbd>`. Schnipsle keeps it, shows it as written in the editor, and puts it into exports after removing anything that could run a program. Text in Markdown is easier to read, so use HTML only for what Markdown cannot say.

## The block at the top

Every note Schnipsle has written starts with a few lines between two `---`, its front matter:

```yaml
---
id: 01JA7Q5Y0Z8H3K2M6N4P9R1S2T
created: 2026-11-20T08:15:00+01:00
modified: 2026-11-27T19:42:10+01:00
---
```

- `id` stays the same for the life of the note, whatever its title or file name. It ties the note to its history, and lets Schnipsle tell a copy from the original.
- `created` and `modified` are the times the note was made and last changed.
- `pinned: true` is there while the note is pinned.

Schnipsle shows these lines muted and writes them itself; you can leave them alone. Lines that other programs add here are kept.

## Tags

A `#` directly followed by letters is a tag, such as `#recipes`, anywhere in the text but not in code. The tags are what Schnipsle sorts your notes by: [Tags and smart folders](tags-and-smart-folders.md).

#schnipsle_manual
