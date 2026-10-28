# Query language

The search field and smart folders (format.md §9) use the same language. A query is a list of terms; a note is found if it matches **all** of them.

## Terms

| Term                | Finds notes …                                                            | Example                                                    |
| ------------------- | ------------------------------------------------------------------------ | ---------------------------------------------------------- |
| `word`              | with a word starting with it; case and accents are ignored               | `antenne` finds "Antennentuner", `gehause` finds "Gehäuse" |
| `"words in order"`  | with these words next to each other                                      | `"40 m band"`                                              |
| `#tag` or `tag:tag` | with the tag or one of its subtags                                       | `#radio` finds `#radio/qrp` too                            |
| `created:…`         | created in a period (front matter `created`, else the file's time)       | `created:2026`                                             |
| `modified:…`        | last changed in a period (front matter `modified`, else the file's time) | `modified:7d`                                              |
| `has:image`         | with an image                                                            |                                                            |
| `has:task`          | with a task (`- [ ]` or `- [x]`)                                         |                                                            |
| `has:link`          | with a link to a note, a file or the web                                 |                                                            |
| `has:chord`         | with a chord in a song sheet (` ```chordpro ` block)                     |                                                            |
| `chord:…`           | with this chord in a song sheet                                          | `chord:Am`, `chord:F#m7`, `chord:D/F#`                     |
| `is:pinned`         | pinned (front matter `pinned: true`)                                     |                                                            |
| `is:untagged`       | without any tag                                                          |                                                            |
| `in:trash`          | in the trash; without it the trash is not searched                       | `#radio in:trash`                                          |

Operators and their values ignore case: `Has:Task` works too. An operator with a value it does not know (`has:music`, `created:soon`) is searched as a word.

## Song sheets

Words and phrases find song sheets (format.md §5.7) by their lyrics, read without the chord brackets (`aufgegangen` finds `aufge[D]gangen`), and by the `title`, `subtitle`, `artist`, `composer` and `lyricist` directives. ChordPro comments (`# …`), tablature and other directives are not searched.

`chord:` compares the whole chord as written in the song: `chord:Am` does not find `Am7` or `A`. The root letter may be lower case (`chord:am`), and `♯`/`♭` count as `#`/`b`. Songs with German note names are searched as written, so `chord:H7` finds a German `[H7]`.

## Combining

| Write    | Meaning                  | Example                     |
| -------- | ------------------------ | --------------------------- |
| `a b`    | both                     | `#radio has:task`           |
| `a OR b` | either; `OR` in capitals | `dipol OR yagi`             |
| `-a`     | not                      | `#radio -#done`             |
| `( … )`  | grouping                 | `(dipol OR yagi) -in:trash` |

`OR` binds weaker than the space: `a b OR c` means "(a and b) or c".

## Dates

A date term takes a period, a comparison or a range:

| Value                | Period                                                    |
| -------------------- | --------------------------------------------------------- |
| `2026`               | the year                                                  |
| `2026-10`            | the month                                                 |
| `2026-10-23`         | the day                                                   |
| `today`, `yesterday` | the day                                                   |
| `7d`, `2w`           | the last 7 days, the last 2 weeks (counted back from now) |
| `3m`, `1y`           | since the same date and time 3 months or 1 year ago       |

| Form           | Meaning                                                          | Example                                       |
| -------------- | ---------------------------------------------------------------- | --------------------------------------------- |
| `created:P`    | within P                                                         | `created:2026-10`                             |
| `created:>P`   | after the end of P                                               | `modified:>2026-06`                           |
| `created:>=P`  | from the start of P                                              | `created:>=2026`                              |
| `created:<P`   | before the start of P                                            | `created:<2026`                               |
| `created:<=P`  | until the end of P                                               | `modified:<=yesterday`                        |
| `created:P..Q` | from the start of P to the end of Q; either side may be left out | `created:2026-01..2026-03`, `modified:..2025` |

Days begin at midnight in your time zone. For the relative forms, `>` and `<` compare with the moment they name: `modified:<7d` finds notes **not** changed in the last 7 days.

## Results

Pinned notes come first, then the rest. Words and phrases rank the results: notes where they occur in the title come first, then by how well the text matches. A query without words (only tags, dates or flags) lists the most recently changed notes first. At most 200 notes are shown.
