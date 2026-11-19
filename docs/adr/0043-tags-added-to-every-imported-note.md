# ADR-0043: Tags added to every note of an import

- Status: Proposed
- Date: 2026-11-19
- Deciders: Willi Kraml
- Related: F-03, F-04, F-05, ADR-0012, ADR-0019, format.md §5.2, §5.3, §12.1 rules 3 and 5, §12.2, §12.3

## Context

A bulk import of hundreds of notes mixes them in with the vault's own. Their folders already become tags (§12.1 rule 4), but a tag for the import itself (`#import/2026-11`, `#inbox`, `#from-evernote`) would let you find, review and sort out the whole batch in one place, or save a smart folder for it. Today that means editing every note afterwards.

Where the tags go matters:

- §12.1 rule 5 already writes folder and source tags on one final line, and rule 3 limits what an import may change in the text to that line, the title heading and the conversion.
- §5.2 takes the title from a first heading, otherwise from the first line. A tag line at the top of a note without a heading would become its title, so the note list would show `#import/2026-11 #inbox` instead of the note's name.
- Rule 7 already puts `# <title>` at the top of some imported notes. Tags right below that heading would sit on line 2 in some notes and need a rule of their own in notes without one.

## Decision

**The import dialog takes optional tags that every note of the run gets. They go on the final tag line of §12.1 rule 5, before the folder and source tags, and the import log records them.**

1. **Input.** One text field, "Add tags", before choosing the export. Tags are separated by spaces or commas, and the `#` is optional. Each word must be a tag by §5.3 as a whole: `#2024` (no letter), `#a0b1c2` (a colour code) or `radio/` are refused with a message before the dry run, not silently changed or dropped as a source's tags are (rule 5). Words are NFC-normalized; a repeat (after the normalization of §5.3) counts once. Changing the field after a dry run runs the dry run again, as the notes to write change. The field starts empty each time the dialog opens, so tags of an earlier import are not reused unseen.
2. **Placement.** The tags are written on the final tag line, first, then the folder tag, then the source's tags. A tag the text already carries, or that the folder or source gives too, is not repeated (as in rule 5, compared after normalization). A note with an empty body gets just the line. No other place in the text changes, so §5.2 titles stay as they are.
3. **Import log.** The log gets an optional top-level `tags` list after `input`, without `#`, written only if there are any. Older readers keep it, as rule 5 of §12.2 asks, so the log `format` stays 1. File › Imports… shows the tags with the run.
4. **Re-import.** An update (§12.3) writes the note as this run makes it, with this run's tags. Importing the same export again with other tags therefore changes the tag line of the notes it updates; notes it skips keep theirs. Undo (§12.4) is not affected.

Spec: format.md revision 0.17 (§12.1 rules 3 and 5, §12.2).

## Options considered

| Option | Pros | Cons |
| --- | --- | --- |
| On the final tag line (chosen) | One place where an import adds tags; existing de-duplication applies; titles and the note's opening stay as in the source | Not visible without scrolling in a long note |
| First line of the note | Visible at once | Becomes the title of every note without a heading (§5.2); a second place where an import changes the text |
| Below the title heading | Visible at once, titles stay | Notes without a heading still need another rule; tags in two places when the source also has tags |
| A front-matter key (`tags:`) | Text untouched | Not a tag by §5.3, so neither search nor the tag list would see it; a format change for all notes, not only imports |

## Consequences

- Positive: a batch can be found and handled as a whole right after the import; nothing new to learn in the file format, since the tags are ordinary hashtags.
- Negative: the dialog has one more field. A re-import with different tags changes notes that were unchanged here.
- Follow-up work: none planned. A list of recent import tags to choose from could come later if the field is used often.
