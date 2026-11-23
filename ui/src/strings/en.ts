// SPDX-License-Identifier: Apache-2.0
// The English text of the UI (ADR-0041): every word the app shows, by view.
// A text with values is a function; counts go through `count` and `forms`,
// which follow `Intl.PluralRules`, so another language brings its own forms.
// Another language is a file like this one, typed as `Strings`. This file
// has no imports, so scripts/check-strings.mjs can load it in Node.

const rules = new Intl.PluralRules("en");
/** The form of a word for `n`: `forms(1, "is", "are")` is "is". */
const forms = (n: number, one: string, other: string): string =>
  rules.select(n) === "one" ? one : other;
/** `n` and the noun for it: `count(3, "note", "notes")` is "3 notes". */
const count = (n: number, one: string, other: string): string =>
  `${String(n)} ${forms(n, one, other)}`;
/** A quoted name. */
const q = (name: string): string => `“${name}”`;

export const en = {
  common: {
    cancel: "Cancel",
    close: "Close",
    delete: "Delete",
    done: "Done",
    export: "Export",
    keep: "Keep",
    notNow: "Not now",
    save: "Save",
    undo: "Undo",
    restore: "Restore",
    inICloudOnly: (name: string) => `${name} (in iCloud only)`,
    notDownloaded: "Not downloaded yet – try again shortly.",
    readOnly: "This vault is read-only.",
  },

  /** Errors the core reports by kind, with the path or name they are about. */
  errors: {
    noVault: () => "No vault folder is open",
    noIndex: () => "The search index is not available",
    notADirectory: (path: string) => `Not a folder: ${path}`,
    notAVault: (path: string) => `Not a Schnipsle vault yet: ${path}`,
    insideVault: (path: string) => `This folder is inside the vault ${path}; open that one instead`,
    invalidPath: (path: string) => `Invalid note path: ${path}`,
    notDownloaded: (path: string) => `Not downloaded yet: ${path}`,
    conflict: (path: string) => `Changed elsewhere since it was opened: ${path}`,
    alreadyExists: (path: string) => `A note with this name already exists: ${path}`,
    invalidId: (id: string) => `The note's id is not valid: ${id}`,
    emptyTag: () => "Enter a name for the tag",
    notATag: (name: string) =>
      `#${name} is not a tag: use letters, digits, _, - and / between levels`,
    tagIntoItself: () => "A tag cannot be moved into one of its own subtags",
  },

  app: {
    back: "Back to notes",
    newNote: "New",
    openVault: "Open Vault…",
    openVaultTitle: "Open another vault, or make a folder a new one",
    settings: "Settings",
  },

  setup: {
    create: "Make it a vault",
    createAndMove: "Move into “notes” and open",
    details: "What will be moved",
    moves: (folder: string, notes: number, items: number) =>
      `${q(folder)} is not a Schnipsle vault yet. To open it, Schnipsle moves everything in it ` +
      `(${count(notes, "note", "notes")} in ${count(items, "item", "items")}) into a new folder ` +
      `“notes” inside it. Nothing is changed or deleted, but other apps will find these files ` +
      `in “notes” from now on.`,
    adds: (folder: string) =>
      `${q(folder)} is not a Schnipsle vault yet. Make it one? Schnipsle adds the folders ` +
      `“notes” and “.notesapp” to it.`,
    downloading: (items: number) =>
      ` ${count(items, "item is", "items are")} still in iCloud only and being downloaded; ` +
      `try again when they are here.`,
    cancelled: "Folder not opened; nothing was changed.",
    notOpened: (message: string) => `Not opened: ${message}`,
  },

  strays: {
    move: "Move into “notes”",
    show: "Show them",
    text: (notes: number) =>
      `${count(notes, "note is", "notes are")} outside the “notes” folder of this vault, so ` +
      `${forms(notes, "it is", "they are")} not listed. An older version of Schnipsle on another ` +
      `device may have saved ${forms(notes, "it", "them")} there.`,
    someInICloud: " Some are still in iCloud only; they are moved once they are downloaded.",
    moved: (items: number) => `Moved ${count(items, "item", "items")} into “notes”.`,
    relinked: (items: number) => ` Updated the links in ${String(items)} of them.`,
    skipped: (names: string) => ` Not moved: ${names} (name taken or not downloaded yet).`,
    notMoved: (message: string) => `Not moved: ${message}`,
  },

  conflict: {
    text: "This note was changed on another device while you were editing.",
    useOther: "Use other version",
    keepMine: "Keep mine",
    deletedElsewhere: "The note was deleted elsewhere.",
    choose: "Conflict – choose a version.",
    keptMine: "Saved your version. The other one is in the note's history.",
    notKept: (message: string) =>
      `Your version was not saved and the other one is unchanged: ${message}`,
    usedOther: "Showing the other version. Your text is in the note's history.",
    mineInTrash: (title: string) =>
      `The note was deleted elsewhere. Your text is in the trash as ${q(title)}; restore it from there.`,
    notUsed: (message: string) => `Nothing changed, your text is still here: ${message}`,
    /** Why a note with a conflict cannot be left or quit. */
    unresolved: "choose a version in the banner first",
  },

  quit: {
    title: "Quit without saving?",
    text: (title: string, why: string) => `Your changes to ${q(title)} could not be saved: ${why}.`,
    anyway: "Quit Without Saving",
  },

  syncConflict: {
    keepThis: "Keep this version",
    keepBoth: "Keep both as separate notes",
    show: "Show the other version",
    anotherDevice: "Another device",
    savedOn: (device: string, when: string, more: string) =>
      `${device} saved a different version at ${when}${more}.`,
    copy: (path: string, when: string, more: string) =>
      `${q(path)} is a copy of this note with different text (saved ${when})${more}.`,
    oneOf: (n: number) => ` (1 of ${String(n)})`,
    resolved: "Conflict resolved – the other version is kept in the note's history.",
    notResolved: (message: string) => `Not resolved: ${message}`,
    keptBoth: (path: string) => `Kept both: ${q(path)} is now a separate note.`,
    notChanged: (message: string) => `Not changed: ${message}`,
  },

  list: {
    label: "Notes",
    search: "Search",
    searchLabel: "Search notes",
    saveSearchTitle: "Keep this search as a smart folder",
    sortLabel: "Sort notes",
    sortModified: "Recently changed",
    sortTitle: "Title",
    trash: "Trash",
    notes: "Notes",
    showAll: "Show all",
    noVault:
      "Open a vault to start: pick its folder, or an empty folder for a new vault. On iPhone, " +
      "pick a folder in iCloud Drive.",
    indexing: "The index is being built; try again in a moment.",
    nothingFound: "Nothing found.",
    noNotesYet: "No notes in this folder yet. Tap New.",
    noNotesHere: "No notes here.",
    untagged: "Untagged notes",
    searchFor: (query: string) => `Search: ${query}`,
    inICloud: "Stored in iCloud only – tap to download",
    inTrash: "In the trash – restore it from the Trash view",
    inTrashStatus: (title: string) =>
      `${q(title)} is in the trash. Restore it from the Trash view to open it.`,
    pinned: "Pinned",
  },

  trash: {
    readIt: "Read it; restore it to edit it",
    deleted: (when: string, path: string) => `Deleted ${when} · was ${path}`,
    empty: "The trash is empty.",
    cannotRead: (title: string, message: string) => `${q(title)} cannot be read: ${message}`,
    shown: (title: string) => `${q(title)} is in the trash. Restore it to edit it.`,
    restored: (title: string, path: string) => `Restored ${q(title)} to ${path}.`,
    notRestored: (message: string) => `Not restored: ${message}`,
    moved: (title: string) => `Moved ${q(title)} to the trash.`,
    notDeletedConflict: "Not deleted: resolve the conflict first, so no text is lost.",
    notDeleted: (message: string) => `Not deleted: ${message}`,
  },

  editor: {
    label: "Editor",
    editing: "Editing…",
    /** The text a new note starts with; its title is the first line. */
    newNoteTitle: "New note",
    selectNote: "Select a note",
    noteText: "Note text",
    taskDone: "Done",
    taskNotDone: "Not done",
    imageMissing: (name: string) => `Image not available: ${name}`,
    clickToOpen: (apple: boolean) => `${apple ? "⌘" : "Ctrl"}-click to open`,
  },

  note: {
    pin: "Pin",
    unpin: "Unpin",
    image: "Image",
    music: "Music",
    source: "Source",
    preview: "Preview",
    opened: (path: string) => `Opened ${path}`,
    openedNoConflictCheck: (path: string, message: string) =>
      `Opened ${path}, but could not check for sync conflicts: ${message}`,
    downloading: (title: string) => `Downloading ${q(title)} from iCloud…`,
    renamed: (path: string) => `Renamed to ${path}`,
    notRenamed: (message: string) => `Not renamed: ${message}`,
    saved: (time: string) => `Saved ${time}`,
    notSaved: (message: string) => `Not saved: ${message}`,
    /** Why a note with text not saved yet cannot be left. */
    notSavedYet: "they are not saved yet",
    staysOpen: (why: string) => `The note stays open, so your changes are not lost: ${why}.`,
    pinnedFirst: "Pinned – listed first.",
    unpinned: "Unpinned.",
    notPinned: (message: string) => `Not pinned: ${message}`,
    notUnpinned: (message: string) => `Not unpinned: ${message}`,
    snapshotFailed: (message: string) => `Version not stored in the history: ${message}`,
    chordsUnsaved: "The note is not saved, so its chords were not rewritten.",
    chordsFailed: (message: string) => `Chords not rewritten: ${message}`,
    chordNamesLater: (message: string) =>
      `Song sheets show the new names when the note is opened again: ${message}`,
    deletedElsewhere: "This note was deleted or moved elsewhere.",
    updated: "Updated with changes from elsewhere.",
    openFirst: "Open a note first.",
    notAttached: (name: string, message: string) => `Not attached: ${name}: ${message}`,
    linkInvalid: (link: string) => `This link cannot be opened: ${link}`,
    linkFailed: (link: string, message: string) => `Cannot open ${link}: ${message}`,
  },

  music: {
    menu: "Music",
    majorKeys: "Major keys",
    minorKeys: "Minor keys",
    modes: "Modes",
    dorian: (key: string) => `${key} Dorian`,
    mixolydian: (key: string) => `${key} Mixolydian`,
    clef: {
      treble: "Treble",
      tenor: "Tenor (treble 8vb)",
      alto: "Alto",
      bass: "Bass",
    },
    template: {
      melody: "Melody with chords",
      twoVoices: "Two voices",
      choir: "Choir (SATB)",
    },
    newScore: "New score",
    metre: "Metre",
    addVoice: "Add a voice",
    lyricsLine: "Lyrics line",
    chordSymbol: "Chord symbol",
    cursorInNotes: "Put the cursor in the notes first",
    insert: "Insert",
    anotherTune: "Another tune",
    abcStandard: "ABC notation 2.1 ↗",
    voice: (n: number) => `Voice ${String(n)}`,
    tune: (n: number) => `Tune ${String(n)}`,
    playback: "Score playback",
    score: "Music score; click to edit its ABC source",
    scoreNamed: (titles: string) =>
      titles === ""
        ? "Music score; click to edit its ABC source"
        : `Music score: ${titles}; click to edit its ABC source`,
    loading: "Loading…",
    stop: "■ Stop",
    playTune: (title: string) => `▶ ${title}`,
    play: "▶ Play",
    stopTitle: (title: string) => `Stop ${title}`,
    playTitle: (title: string) => `Play ${title}`,
    speed: "Speed in percent of the tune's tempo",
    tempo: "Tempo",
    slower: "Slower",
    faster: "Faster",
    semitones: "Semitones the viewer adds",
    transpose: "Transpose",
    down: "Transpose down a semitone",
    up: "Transpose up a semitone",
    chords: "Chords",
    chordsTitle: "Play an accompaniment to the chord symbols",
    mute: (voice: string) => `Mute ${voice}`,
    solo: (voice: string) => `${voice} alone`,
    notEngraved: (message: string) => `Score not engraved: ${message}`,
    problems: (n: number) => `Score not engraved: ${count(n, "problem", "problems")} in the ABC`,
    at: (line: number, column: number, message: string) =>
      `Line ${String(line)}, column ${String(column)}: ${message}`,
  },

  song: {
    chorus: "Chorus",
    bridge: "Bridge",
    music: "Music:",
    words: "Words:",
    key: "Key",
    capo: "Capo",
    tempo: "Tempo",
    time: "Time",
    view: "Song sheet view",
    notShown: (message: string) => `Song sheet not shown: ${message}`,
    chordNames: "Chord names",
    chordNamesWritten: (german: boolean) =>
      `Chord names (written in ${german ? "German" : "English"})`,
    english: "B, B♭ (English)",
    german: "H, B (German)",
    lyricsOnly: " Lyrics only",
    apply: "Apply transposition",
    applyTitle: "Rewrite the chords in the note; the version before is kept in the history",
    changedMeanwhile: "The song changed meanwhile, so its chords were not rewritten.",
  },

  tags: {
    title: "Tags",
    rename: "Rename or merge",
    renameTag: (name: string) => `Rename #${name}`,
    open: (name: string) => `Open ${name}`,
    close: (name: string) => `Close ${name}`,
    indexing: "Tags appear once the index is built.",
    none: "No tags yet. Write #tag anywhere in a note.",
    untagged: "Untagged",
    combination: "Notes with all of these tags",
  },

  smart: {
    title: "Smart folders",
    new: "New smart folder",
    edit: "Edit smart folder",
    editFolder: (name: string) => `Edit ${name}`,
    moveFolder: (name: string) =>
      `Move ${name}: drag, or use the arrow keys (left and right change the level)`,
    noVault: "No vault is open",
    none: "No smart folders yet. Search, then Save, or tap New smart folder.",
    noneReadOnly: "No smart folders.",
    topLevel: "Top level",
    needName: "Give the folder a name.",
    needQuery: "Enter what the folder searches for.",
    confirmDelete: (name: string, below: number) =>
      `Delete ${q(name)}? No notes are deleted.` +
      (below > 0 ? ` Its ${forms(below, "subfolder moves", "subfolders move")} up a level.` : ""),
    sort: {
      "modified-desc": "Recently changed",
      "modified-asc": "Least recently changed",
      "created-desc": "Recently created",
      "title-asc": "Title",
    },
    notChanged: (message: string) => `Smart folders not changed: ${message}`,
  },

  history: {
    title: "History",
    closeHistory: "Close history",
    compare: "Compare",
    compareWith: "Compare with",
    restore: "Restore this version",
    asNewNote: "As new note",
    versions: "Versions",
    selectVersion: "Select a version",
    notRestoredDirty: "Not restored: the note has unsaved changes or a conflict.",
    restored: (version: string) =>
      `Restored the version of ${version}; the text before is in the history.`,
    notRestored: (message: string) => `Not restored: ${message}`,
    copied: (version: string) => `Copied the version of ${version} as a new note.`,
    notCopied: (message: string) => `Not copied: ${message}`,
    notAvailable: (message: string) => `History not available: ${message}`,
    none: "No earlier versions yet. One is stored after a pause in typing and when you leave the note.",
    earlier: (n: number) => count(n, "earlier version", "earlier versions"),
    withNow: "with the note now",
    withVersion: (version: string) => `with ${version}`,
    cannotRead: (message: string) => `This version cannot be read: ${message}`,
    changesToNow: (from: string) => `Changes from ${from} to the note now`,
    changes: (from: string, to: string) => `Changes from ${from} to ${to}`,
    notCompared: (message: string) => `Not compared: ${message}`,
    removedLine: (text: string) => `Removed: ${text}`,
    addedLine: (text: string) => `Added: ${text}`,
    emptyLine: "empty line",
    unchanged: (n: number) => `… ${count(n, "unchanged line", "unchanged lines")}`,
    showThem: "Show them",
    added: "(added) ",
    removed: "(removed) ",
    same: "The two versions are the same.",
    onlyModified: "The text is the same; only the time of the last change differs.",
    onlyFrontMatter: "The text is the same; the front matter differs.",
    linesAdded: (n: number) => `${count(n, "line", "lines")} added`,
    linesRemoved: (n: number) => `${count(n, "line", "lines")} removed`,
    frontMatter: "Front matter",
    text: "Text",
  },

  folder: {
    title: "Smart folder",
    name: "Name",
    search: "Search",
    searchExample: "#radio modified:30d",
    inside: "Inside",
    order: "Order",
    delete: "Delete…",
  },

  retag: {
    newName: "New name",
    help:
      "Its subtags move along. To merge it into another tag, enter that tag. Every note changed " +
      "keeps its earlier version in its history.",
    start: "Rename",
    title: (name: string) => `Rename #${name}`,
    summary: (notes: number, tags: number) =>
      `${count(notes, "note changes", "notes change")} (${count(tags, "tag", "tags")}).`,
    folders: (n: number) => `${count(n, "smart folder changes", "smart folders change")}.`,
    merge: (old: string, name: string) => `#${old} is merged into #${name}, which exists already.`,
    nothing: "Nothing to change: no note has this tag spelled otherwise.",
    more: (n: number) => `and ${String(n)} more`,
    folder: (name: string, query: string, renamed: string) =>
      `Smart folder ${name}: ${query} → ${renamed}`,
    notRead: (title: string, reason: string) => `${title}: not read (${reason})`,
    renamedIn: (n: number) => `Renamed in ${count(n, "note", "notes")}.`,
    undoneIn: (n: number) => `Undone in ${count(n, "note", "notes")}.`,
    noneRenamed: "No note was renamed.",
    noneUndone: "No note was changed back.",
    foldersChanged: (n: number) => `${count(n, "smart folder", "smart folders")} changed.`,
    leftAsIs: (n: number) =>
      `${count(n, "note was", "notes were")} left as ${forms(n, "it is", "they are")}:`,
    skipped: (title: string, reason: string) => `${title}: ${reason}`,
    foldersError: (message: string) => `Smart folders not changed: ${message}`,
    undoLast: (old: string, name: string) => `Undo the rename of #${old} to #${name}`,
    renamed: (old: string, name: string) => `#${old} renamed to #${name}.`,
    undone: (old: string) => `Rename of #${old} undone.`,
  },

  import: {
    title: "Import notes",
    from: "From",
    source: {
      appleNotes: "Apple Notes",
      simplenote: "Simplenote",
      html: "HTML files",
      markdown: "Markdown files",
    },
    help: {
      appleNotes: "Two steps, because the export from Notes leaves out dates and folders:",
      appleNotesExport:
        "In Notes, open a folder, select its notes (⌘A) and choose File › Export as › Markdown. " +
        "Save them into a folder of their own inside one folder for the import, e.g. " +
        "“Schnipsle-Import”. Do this for each folder you want.",
      appleNotesChoose:
        "Choose that folder here. Schnipsle then reads the dates and folders of your notes from " +
        "Notes; macOS asks once whether it may. Nothing in Notes is changed.",
      simplenote:
        "In Simplenote, choose File › Export Notes (in the web app: Settings › Tools › Export " +
        "Notes). It saves a zip file; choose it here as it is, there is no need to unpack it.",
      simplenoteTags:
        "Tags become hashtags at the end of each note, unless the text has them already. Notes in " +
        "Simplenote’s trash go to the trash here.",
      html:
        "Choose a folder of HTML files (.html, .htm), such as web pages saved from a browser, or " +
        "a single file. Each file becomes a note, and its subfolders become tags.",
      htmlPictures:
        "Pictures saved with a page and pictures embedded in it are copied into the vault, as are " +
        "files the page links to in the folder. Pictures and links on the web stay links.",
      markdown:
        "Choose a folder of Markdown files (.md), such as notes from another editor, or a single " +
        "file. Each file becomes a note as it is, front matter included, and its subfolders " +
        "become tags. Hidden folders such as .obsidian are left out.",
      markdownPictures:
        "Pictures and files the notes link to in the folder are copied into the vault. Songs in " +
        "ChordPro (.cho, .crd, .chopro, .chordpro) become notes with their title and the song as " +
        "a chordpro block.",
    },
    tagsLabel: "Add tags",
    tagsExample: "#import/2026-11",
    tagsHelp:
      "Optional: tags for every imported note, separated by spaces. They go on the tag line at " +
      "the end of each note, unless the text has them already.",
    tagsAdded: (tags: string) => `Every note gets ${tags}.`,
    chooseFolder: "Choose folder…",
    chooseFile: "Choose file…",
    readMetadata: "Read dates and folders from Notes",
    column: {
      note: "Note",
      import: "Import",
      where: "Where, or why not",
    },
    start: "Import",
    action: {
      add: "New",
      update: "Update",
      skip: "Skipped",
    },
    pick: {
      "apple-notes": "Choose folder…",
      simplenote: "Choose zip file…",
      html: "Choose folder…",
      markdown: "Choose folder…",
    },
    reason: {
      duplicate: "In the export twice; imported once",
      changedHere: "Changed here since the last import, so kept as it is",
      notNewer: "Not changed in the source since the last import",
      noModifiedTime: "Imported before; without a modification time it is not updated",
      missing: "Imported before and deleted here since",
      inTrash: "Imported before and in the trash here",
      changedMeanwhile: "Changed here during the import, so kept as it is",
      failed: "Could not be written",
    },
    noVault: "Open a vault first: the notes are imported into it.",
    reading: "Reading the export…",
    metadataFound:
      "Dates and folders come from apple-notes-metadata.json in this folder. If you changed " +
      "notes since, read them again.",
    readAgain: "Read again from Notes",
    metadataMissing:
      "Dates and folders are still missing: without them every note gets the time of the " +
      "export, and importing again later with them would add the notes a second time.",
    readingNotes:
      "Reading your notes from Notes… This takes about half a minute for a few hundred notes.",
    readingFailed: "Reading from Notes did not work.",
    importing: "Importing…",
    importingList: "Importing… The notes not yet written are the ones listed below.",
    progress: (done: number, total: number) =>
      `${String(done)} of ${count(total, "note", "notes")}`,
    failed: (message: string) =>
      `${message}. Notes written up to then are in the vault and in its import log.`,
    added: (n: number) => `${String(n)} new`,
    intoTrash: (n: number) => `${String(n)} of them into the trash`,
    updated: (n: number) => `${String(n)} updated`,
    skipped: (n: number) => `${String(n)} skipped`,
    imported: (parts: string) => `Imported: ${parts}.`,
    nothing: (n: number) =>
      `Nothing to import: ${forms(n, "the note is", `all ${String(n)} notes are`)} skipped.`,
    plan: (n: number, input: string, parts: string) =>
      `${count(n, "note", "notes")} in ${input}: ${parts}. Nothing is written until you choose ` +
      `Import.`,
    newIntoTrash: "New, into the trash",
  },

  imports: {
    title: "Imports",
    column: {
      when: "When",
      notes: "Notes",
    },
    all: "All imports",
    undo: "Undo import",
    source: {
      "apple-notes": "Apple Notes",
      simplenote: "Simplenote",
      html: "HTML",
      markdown: "Markdown",
    },
    action: {
      trash: "Into the trash",
      restore: "Previous version back",
      keep: "Stays",
      missing: "Gone already",
      inTrash: "In the trash already",
    },
    openFirst: "Open a vault first.",
    noVault: "Open a vault first: its imports are listed here.",
    reading: "Reading the import logs…",
    none: "Nothing has been imported into this vault yet.",
    explain:
      "Undoing an import moves its notes to the trash, or gives notes it updated their previous " +
      "version back. Notes changed here since stay as they are.",
    added: (n: number) => `${String(n)} new`,
    updated: (n: number) => `${String(n)} updated`,
    undoneAt: (when: string) => `Undone ${when}`,
    undoButton: "Undo…",
    looking: "Looking at the notes of this import…",
    intoTrash: (n: number) => `${count(n, "note", "notes")} into the trash`,
    restored: (n: number) =>
      `${count(n, "note gets its", "notes get their")} previous version back`,
    stay: (n: number) => count(n, "note stays", "notes stay"),
    alreadyInTrash: (n: number) => `${count(n, "note is", "notes are")} in the trash already`,
    gone: (n: number) => `${count(n, "note is", "notes are")} gone already`,
    theSource: "the source",
    withTags: (from: string, tags: string) => `${from}, with ${tags}`,
    what: (input: string, source: string, when: string) =>
      `the import from ${input} (${source === "" ? "" : `${source}, `}${when})`,
    undone: (what: string, parts: string) => `Undone ${what}: ${parts}.`,
    nothing: (what: string, parts: string) => `Undoing ${what} would change nothing: ${parts}.`,
    plan: (what: string, parts: string) =>
      `Undo ${what}: ${parts}. Nothing changes until you choose Undo import.`,
    untitled: "(untitled)",
    openNote: "Open this note",
    undoing: "Undoing the import…",
  },

  export: {
    title: "Export notes",
    notes: "Notes",
    formatLabel: "Format",
    noPandoc: "Word and EPUB appear here once Pandoc is installed.",
    files: "Files",
    paper: "Page size",
    paperA4: "A4",
    paperLetter: "Letter",
    contents: "Start with a table of contents",
    pageNumbers: "Page numbers",
    keepTags: "Keep the line of tags at the end of a note",
    abcSource: "Add the ABC source under each score",
    differentColumn: "What is different in the export",
    share: "Share…",
    start: "Export…",
    startHere: "Export",
    formats: {
      html: "HTML",
      pdf: "PDF",
      odt: "OpenDocument text (ODT)",
      docx: "Word (DOCX)",
      epub: "EPUB",
    },
    throughPandoc: (format: string, version: string) => `${format}, through Pandoc ${version}`,
    problem: {
      noteNotRead: () =>
        "Could not be read, perhaps not downloaded yet, so it is not in the export",
      pictureMissing: (target: string) =>
        `Picture not found, its description is shown instead: ${target}`,
      webPictureAsLink: (target: string) => `Picture on the web, shown as a link: ${target}`,
      pictureNotConverted: (target: string) =>
        `Picture left as it is, which many programs cannot show: ${target}`,
      attachmentNotIncluded: (target: string) =>
        `Attachment not included, only the text of its link: ${target}`,
      noteNotExported: (target: string) =>
        `Link to a note not in this export; only its text stays: ${target}`,
      noteInOtherFile: (target: string) =>
        `Link to a note in another file; only its text stays: ${target}`,
      scoreNotEngraved: (target: string) =>
        `Score shown as its ABC source, as it could not be engraved: ${target}`,
    },
    numbered: (n: number, shown: string, more: number) =>
      `${count(n, "file", "files")} got a number, as ${forms(n, "its name was", "their names were")} ` +
      `already taken: ${shown}${more > 0 ? ` and ${String(more)} more` : ""}.`,
    thisNote: (title: string) => `This note: ${title}`,
    noNote: "This note (none is open)",
    listIsTrash: "The notes in the list (it shows the trash now)",
    allNotes: (n: number) => `All notes in the list (${count(n, "note", "notes")})`,
    listNotes: (label: string, n: number, inTrash: number) =>
      `${label} (${count(n, "note", "notes")}${inTrash > 0 ? `, without ${String(inTrash)} in the trash` : ""})`,
    oneDocument: "One document",
    oneFile: "One file",
    perNoteFolderCopies: "One file per note, in a folder with copies of their attachments",
    perNoteFolder: "One file per note, in a folder",
    perNoteZipCopies: "One file per note, in a zip archive with copies of their attachments",
    perNoteZip: "One file per note, in a zip archive",
    folderCopies: "In a folder, with copies of its attachments",
    zipCopies: "In a zip archive, with copies of its attachments",
    unsaved: (title: string) =>
      `${q(title)} has changes that are not saved. The export holds the version last saved.`,
    noVault: "Open a vault first: the notes are exported from it.",
    pandocMissing: "Word and EPUB appear here once Pandoc is installed.",
    pandocFails: (problem: string) =>
      `Word and EPUB need Pandoc. ${problem}. Choose another one in Settings.`,
    notesGone: "These notes are no longer there.",
    engraving: (done: number, total: number) =>
      `Engraving scores: ${String(done)} of ${String(total)}…`,
    exporting: (n: number) => `Exporting ${count(n, "note", "notes")}…`,
    failed: (message: string) => `${message}. Files written up to then stay where they are.`,
    nothing: "Nothing was exported.",
    sharedOne: (name: string) =>
      `Exported ${q(name)}. Share it to save it in Files, send it or print it.`,
    sharedFolder: (n: number, name: string) =>
      `Exported ${count(n, "file", "files")} into the folder ${q(name)}. Share it to save it ` +
      `in Files or send it.`,
    sharedZip: (n: number, name: string) =>
      `Exported ${count(n, "file", "files")} into the zip archive ${q(name)}. Share it to save ` +
      `it in Files or send it.`,
    toShareOne: (name: string) => `Exported ${q(name)} to share.`,
    toShareFolder: (n: number, name: string) =>
      `Exported ${count(n, "file", "files")} into the folder ${q(name)} to share.`,
    toShareZip: (n: number, name: string) =>
      `Exported ${count(n, "file", "files")} into the zip archive ${q(name)} to share.`,
    intoZip: (n: number, to: string) => `Exported ${count(n, "file", "files")} into ${to}.`,
    toOne: (to: string) => `Exported to ${to}.`,
    toFolder: (n: number, to: string) => `Exported ${count(n, "file", "files")} to ${to}.`,
    different: (n: number) => ` ${count(n, "thing is", "things are")} different from the notes:`,
    warning: {
      linksMissed: (title: string, n: number) =>
        `${title}: ${count(n, "link", "links")} could not be put back`,
      linksFailed: (title: string, error: string) => `${title}: links could not be added: ${error}`,
      pageNumbersFailed: (title: string, error: string) =>
        `${title}: page numbers could not be added: ${error}`,
      svgOnly: (title: string, description: string | null) =>
        `${title}: ${description === null ? "An SVG drawing" : `The SVG drawing ${q(description)}`} ` +
        `is in the document as SVG only, which Word before 2016 and some other programs do not ` +
        `show. With rsvg-convert installed (on a Mac: brew install librsvg), a PNG copy is added ` +
        `for them.`,
      converter: (title: string, message: string) => `${title}: ${message}`,
    },
    showInFinder: "Show in Finder",
    showInFolder: "Show in folder",
  },

  settings: {
    vault: "This vault",
    retention: "Earlier versions of a note",
    keepAllBefore: "Keep every version for",
    keepAllAfter: "days,",
    keepDailyBefore: "then one a day until",
    keepDailyAfter: "days, then one a week.",
    removeThem: "Remove them",
    keepThem: "Keep them",
    chords: "Chord names in song sheets",
    chordsEnglish: "English: B and B♭",
    chordsGerman: "German: H and B",
    device: "This Mac",
    deviceHint: "Saved on this Mac only.",
    pandoc: "Pandoc, for Word and EPUB",
    pandocPick: "Choose Program…",
    pandocFind: "Find It Automatically",
    updates: "Check for updates once a day",
    updatesHint:
      "Schnipsle then asks GitHub whether a newer version exists. It sends nothing else.",
    invalid: (written: string, name: string, valid: string) =>
      `The vault's settings file has ${q(written)} for ${name}, which is not ${valid}; the ` +
      `default applies.`,
    keepingAll: "keeping every version",
    keepingDaily: "keeping one a day",
    wholeDays: "a whole number of days",
    chordNames: "the chord names",
    chordValues: "“english” or “german”",
    extended: (days: number) =>
      `One a day is kept until ${count(days, "day", "days")}, as every version is kept that long.`,
    defaults: (all: number, daily: number) =>
      `Defaults: every version for ${count(all, "day", "days")}, one a day until ` +
      `${count(daily, "day", "days")}.`,
    pandocFails: (path: string, problem: string) =>
      `${path} does not run: ${problem}. Word and EPUB are not offered.`,
    pandocMissing: "Not found. Install Pandoc (for example with Homebrew) or choose the program.",
    pandocFound: (version: string, path: string) => `Pandoc ${version}, found at ${path}`,
    pandocChosen: (version: string, path: string) => `Pandoc ${version}, chosen at ${path}`,
    noVault: "Open a vault to see its settings.",
    readOnly: (why: string) =>
      `The vault is open read-only, so its settings cannot be changed. ${why}`,
    inVault: "Saved in the vault, so they apply on every device that opens it.",
    vaultUnreadable: (message: string) => `The vault's settings cannot be read: ${message}`,
    deviceUnreadable: (message: string) => `This Mac's settings cannot be read: ${message}`,
    notSaved: (message: string) => `Not saved: ${message}`,
    needDays: "Enter a whole number of days.",
    removes: (versions: number, notes: number) =>
      `This removes ${count(versions, "earlier version", "earlier versions")} of ` +
      `${count(notes, "note", "notes")} for good, on every device.`,
  },

  diagnostics: {
    title: "Diagnostics",
    hint:
      "How long this app's steps took since it started, and how much memory it uses. Kept in " +
      "memory only: nothing is saved or sent.",
    refresh: "Refresh",
    copy: "Copy",
    copied: "Copied.",
    notCopied: "Not copied. Select the text below and copy it.",
    nothing: "Nothing measured yet.",
    unreadable: (message: string) => `The measurements cannot be read: ${message}`,
    app: (version: string, device: string | null, system: string) =>
      `Schnipsle ${version}${device === null ? "" : ` on ${device}`}, ${system}`,
    memory: (now: string, peak: string | null) =>
      peak === null ? `Memory: ${now}` : `Memory: ${now}, at most ${peak}`,
    noMemory: (why: string) => `Memory not measured: ${why}`,
    megabytes: (mb: string) => `${mb} MB`,
    ms: (ms: string) => `${ms} ms`,
    summary: (n: number, median: string, slowest: string) =>
      `${count(n, "time", "times")}; median of the last ones ${median}, slowest ${slowest}`,
    kind: {
      start: "Start, until the note list shows",
      openVault: "Opening the vault",
      indexSync: "Index sync",
      allNotes: "All notes",
      allNotesUi: "All notes, until shown",
      tagFolders: "Tag folders",
      notesInTags: "Notes of a tag folder",
      notesInTagsUi: "Notes of a tag folder, until shown",
      smartFolder: "Notes of a smart folder",
      smartFolderUi: "Notes of a smart folder, until shown",
      search: "Search",
      searchUi: "Search, until shown",
      readNote: "Reading a note",
      readNoteUi: "Opening a note, until shown",
      export: "Export",
    } as Record<string, string>,
    detail: {
      notes: (n: number) => count(n, "note", "notes"),
      folders: (n: number) => count(n, "folder", "folders"),
      query: (query: string) => `for ${q(query)}`,
      kilobytes: (kb: number) => `${String(kb)} KB`,
      read: (n: number) => `${String(n)} read`,
      removed: (n: number) => `${String(n)} removed`,
      format: (format: string) => format.toUpperCase(),
    },
  },
};

export type Strings = typeof en;
