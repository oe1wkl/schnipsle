// SPDX-License-Identifier: Apache-2.0
// Writes the metadata the Apple Notes Markdown export leaves out (ADR-0019):
// for every note its id, account, folder path, the smart folders that show
// it, title, creation and modification dates, plus the start of its text, so
// the importer can match each exported .md file to its note.
//
// Usage (JavaScript for Automation, macOS only):
//   osascript -l JavaScript scripts/apple-notes-metadata.js <export-folder>
// Writes <export-folder>/apple-notes-metadata.json. The first run asks to
// let the terminal control Notes. Reads Notes only, never changes it.

/* global Application, $ */

const FILE_NAME = "apple-notes-metadata.json";
const FORMAT = 1;
const SNIPPET_LENGTH = 200;

// JXA calls `run` with the command line arguments; its return value is printed.
// eslint-disable-next-line @typescript-eslint/no-unused-vars
function run(argv) {
  if (argv.length !== 1) {
    throw new Error("usage: osascript -l JavaScript apple-notes-metadata.js <export-folder>");
  }
  const notes = Application("Notes");
  const folders = folderPaths(notes);
  const list = [];
  const smart = new Map(); // note id → names of the smart folders showing it
  for (const account of notes.accounts()) {
    for (const folder of account.folders()) {
      if (isSmart(folder)) {
        for (const id of folder.notes.id()) {
          smart.set(id, [...(smart.get(id) ?? []), folder.name()]);
        }
      } else {
        list.push(...notesIn(folder, account.name(), folders));
      }
    }
  }
  for (const note of list) note.smartFolders = smart.get(note.id) ?? [];
  const out = {
    format: FORMAT,
    exported: new Date().toISOString(),
    notes: list,
  };
  const path = `${argv[0].replace(/\/+$/, "")}/${FILE_NAME}`;
  const text = $.NSString.alloc.initWithUTF8String(JSON.stringify(out, null, 2) + "\n");
  if (!text.writeToFileAtomicallyEncodingError(path, true, $.NSUTF8StringEncoding, null)) {
    throw new Error(`cannot write ${path}`);
  }
  return `${String(list.length)} notes in ${path}`;
}

// Folder id → list of names from the top level down, e.g. ["Radio", "QRP"].
function folderPaths(notes) {
  const parent = new Map();
  const name = new Map();
  for (const folder of notes.folders()) {
    const id = folder.id();
    name.set(id, folder.name());
    // `class()` fails on a container, but the id tells folders from accounts.
    const container = folder.container().id();
    parent.set(id, container.includes("/ICFolder/") ? container : null);
  }
  const paths = new Map();
  for (const id of name.keys()) {
    const path = [];
    for (let at = id; at !== null && path.length < 100; at = parent.get(at) ?? null) {
      path.unshift(name.get(at));
    }
    paths.set(id, path);
  }
  return paths;
}

// A smart folder lists notes that live in other folders, so its notes have
// another container. (Scripting shows smart folders as ordinary folders.)
function isSmart(folder) {
  const first = folder.notes[0];
  return first.exists() && first.container().id() !== folder.id();
}

// The notes directly in `folder`. Properties are fetched for all its notes at
// once, which is much faster than note by note.
function notesIn(folder, account, folders) {
  const n = folder.notes;
  const ids = n.id();
  if (ids.length === 0) return [];
  const names = n.name();
  const created = n.creationDate();
  const modified = n.modificationDate();
  const locked = n.passwordProtected();
  const shared = n.shared();
  const path = folders.get(folder.id()) ?? [folder.name()];
  return ids.map((id, i) => ({
    id,
    account,
    folder: path,
    name: names[i],
    created: created[i].toISOString(),
    modified: modified[i].toISOString(),
    locked: locked[i],
    shared: shared[i],
    snippet: locked[i] ? "" : snippet(n[i]),
  }));
}

function snippet(note) {
  try {
    return note.plaintext().replace(/\s+/g, " ").trim().slice(0, SNIPPET_LENGTH);
  } catch {
    return "";
  }
}
