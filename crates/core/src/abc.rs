// SPDX-License-Identifier: Apache-2.0
//! Music notation in ABC (format.md §5.6, ADR-0013).
//!
//! [`index_text`] gives what rule 5 puts into the full-text index: the text of
//! the `T:` (title), `C:` (composer), `w:` and `W:` (lyrics) lines of a
//! ` ```abc ` block, so a note is found by the title or the words of a song in
//! it. The notes themselves, the other fields and `%` comments are left out.
//!
//! The text is read as ABC 2.1 writes it (§2.3 and §4.20 of the standard): a
//! field goes on in a `+:` line; `\"u`, `&uuml;` and `\u00fc` stand for `ü`;
//! in `w:` lines a `-` divides the syllables of a word, also at the end of a
//! line, `~` joins words under one note, and `_`, `*` and `|` only align
//! syllables with notes. So `w: auf-ge-gan-gen` is found as `aufgegangen`.

/// The text of the `T:`, `C:`, `w:` and `W:` lines of an `abc` block, one
/// line per field, each ending with a newline. Never fails: a line that is
/// not one of these fields adds nothing.
#[must_use]
pub fn index_text(source: &str) -> String {
    let mut lines: Vec<String> = Vec::new();
    // The field a `+:` line continues, if it is indexed.
    let mut last: Option<Field> = None;
    // The last lyrics ended inside a word (`auf-`), which goes on in the
    // next `w:` line.
    let mut open_word = false;

    for line in source.lines() {
        let line = line.trim_start();
        let Some((key, value)) = field(line) else {
            if line.starts_with('%') {
                continue; // a comment or a `%%` directive between fields
            }
            last = None; // music or an empty line
            continue;
        };
        let kind = match key {
            'T' | 'C' | 'W' => Field::Text,
            'w' => Field::Lyrics,
            '+' => match last {
                Some(kind) => kind,
                None => continue,
            },
            _ => {
                last = None;
                open_word = false;
                continue;
            }
        };
        let continues = key == '+';
        match kind {
            Field::Text => {
                open_word = false;
                let text = squeeze(&decode(value, false).0);
                if continues {
                    append(&mut lines, &text, " ");
                } else {
                    lines.push(text);
                }
            }
            Field::Lyrics => {
                let (text, open) = decode(value, true);
                let text = squeeze(&text);
                let next_open = (open && !text.is_empty()) || (open_word && text.is_empty());
                if open_word {
                    append(&mut lines, &text, "");
                } else if continues {
                    append(&mut lines, &text, " ");
                } else {
                    lines.push(text);
                }
                open_word = next_open;
            }
        }
        last = Some(kind);
    }

    let mut out = String::new();
    for line in lines.iter().filter(|l| !l.is_empty()) {
        out.push_str(line);
        out.push('\n');
    }
    out
}

/// What a field line holds for the index.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Field {
    /// Title, composer or words after the tune: plain text.
    Text,
    /// Words under the notes, with syllable marks.
    Lyrics,
}

/// The key and value of a field line such as `T:Kanon` or `+: more`.
fn field(line: &str) -> Option<(char, &str)> {
    let mut chars = line.chars();
    let key = chars
        .next()
        .filter(|c| c.is_ascii_alphabetic() || *c == '+')?;
    (chars.next() == Some(':')).then(|| (key, &line[2..]))
}

/// Adds `text` to the last line, after `separator`, or starts a line.
fn append(lines: &mut Vec<String>, text: &str, separator: &str) {
    match lines.last_mut() {
        Some(line) if !text.is_empty() => {
            if !line.is_empty() {
                line.push_str(separator);
            }
            line.push_str(text);
        }
        Some(_) => {}
        None => lines.push(text.to_owned()),
    }
}

/// One space between words, none at either end.
fn squeeze(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The text of a field value: escapes and entities replaced, a `%` comment
/// cut off. For `lyrics` the syllable marks are read too, and the flag says
/// whether the text ends inside a word.
fn decode(value: &str, lyrics: bool) -> (String, bool) {
    let mut out = String::new();
    let mut open = false; // after a `-`, the word goes on
    let mut rest = value;
    while let Some(c) = rest.chars().next() {
        rest = &rest[c.len_utf8()..];
        let text = match c {
            '%' => break,
            '\\' if lyrics && rest.starts_with('-') => {
                rest = &rest[1..];
                Some('-')
            }
            '\\' => match escape(rest) {
                Some((decoded, used)) => {
                    rest = &rest[used..];
                    Some(decoded)
                }
                None => Some('\\'),
            },
            '&' => match entity(rest) {
                Some((decoded, used)) => {
                    rest = &rest[used..];
                    Some(decoded)
                }
                None => Some('&'),
            },
            '-' if lyrics => {
                out.truncate(out.trim_end().len());
                open = true;
                None
            }
            '_' if lyrics => None,
            '~' | '*' | '|' if lyrics => Some(' '),
            c => Some(c),
        };
        match text {
            Some(c) if c.is_whitespace() => {
                if !open {
                    out.push(' ');
                }
            }
            Some(c) => {
                out.push(c);
                open = false;
            }
            None => {}
        }
    }
    (out, open)
}

/// Accent marks of ABC 2.1 §2.3: after the mark, pairs of a letter and the
/// letter with that accent.
const ACCENTS: &[(char, &str)] = &[
    ('`', "AÀEÈIÌOÒUÙaàeèiìoòuù"),
    ('\'', "AÁEÉIÍOÓUÚYÝaáeéiíoóuúyýCĆcćNŃnńSŚsśZŹzź"),
    ('^', "AÂEÊIÎOÔUÛaâeêiîoôuû"),
    ('"', "AÄEËIÏOÖUÜaäeëiïoöuüyÿ"),
    ('~', "AÃNÑOÕaãnñoõ"),
    ('c', "CÇcç"),
    ('o', "AÅUŮaåuů"),
    ('/', "OØoø"),
    ('u', "AĂGĞaăgğ"),
    ('v', "CČEĚNŇRŘSŠZŽcčeěnňrřsšzž"),
];

/// Ligatures and letters of ABC 2.1 §2.3 written with two letters.
const LIGATURES: &[(&str, char)] = &[
    ("ss", 'ß'),
    ("AE", 'Æ'),
    ("ae", 'æ'),
    ("OE", 'Œ'),
    ("oe", 'œ'),
    ("DH", 'Ð'),
    ("dh", 'ð'),
    ("TH", 'Þ'),
    ("th", 'þ'),
];

/// The accented letter for an accent mark and a letter, as in [`ACCENTS`].
fn accented(mark: char, letter: char) -> Option<char> {
    let (_, pairs) = ACCENTS.iter().find(|(m, _)| *m == mark)?;
    let pairs: Vec<char> = pairs.chars().collect();
    pairs
        .chunks(2)
        .find(|pair| pair[0] == letter)
        .map(|pair| pair[1])
}

/// The character a backslash escape stands for, and the bytes it used after
/// the backslash: `\u00fc`, `\U0001F3B5`, `\"u`, `\ss`, `\%`, `\\`, `\&`.
fn escape(rest: &str) -> Option<(char, usize)> {
    for (prefix, digits) in [('u', 4), ('U', 8)] {
        if let Some(hex) = rest.strip_prefix(prefix).and_then(|r| r.get(..digits))
            && hex.chars().all(|c| c.is_ascii_hexdigit())
            && let Some(c) = u32::from_str_radix(hex, 16).ok().and_then(char::from_u32)
        {
            return Some((c, 1 + digits));
        }
    }
    let mut chars = rest.chars();
    let first = chars.next()?;
    if let Some(second) = chars.next() {
        let pair = &rest[..first.len_utf8() + second.len_utf8()];
        if let Some((_, c)) = LIGATURES.iter().find(|(l, _)| *l == pair) {
            return Some((*c, pair.len()));
        }
        if let Some(c) = accented(first, second) {
            return Some((c, pair.len()));
        }
    }
    matches!(first, '%' | '\\' | '&').then_some((first, 1))
}

/// The character an HTML entity such as `&uuml;`, `&#252;` or `&#xFC;`
/// stands for, and the bytes it used after the `&`.
fn entity(rest: &str) -> Option<(char, usize)> {
    let end = rest.find(';').filter(|end| (1..=10).contains(end))?;
    let name = &rest[..end];
    if !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '#') {
        return None;
    }
    let c = if let Some(number) = name.strip_prefix('#') {
        let code = match number.strip_prefix(['x', 'X']) {
            Some(hex) => u32::from_str_radix(hex, 16),
            None => number.parse(),
        };
        char::from_u32(code.ok()?)?
    } else {
        match name {
            "amp" => '&',
            "lt" => '<',
            "gt" => '>',
            "quot" => '"',
            "apos" => '\'',
            "nbsp" => ' ',
            "szlig" => 'ß',
            "AElig" => 'Æ',
            "aelig" => 'æ',
            "OElig" => 'Œ',
            "oelig" => 'œ',
            "oslash" => 'ø',
            "Oslash" => 'Ø',
            _ => {
                let mut chars = name.chars();
                let letter = chars.next()?;
                let mark = match chars.as_str() {
                    "grave" => '`',
                    "acute" => '\'',
                    "circ" => '^',
                    "uml" => '"',
                    "tilde" => '~',
                    "cedil" => 'c',
                    "ring" => 'o',
                    "breve" => 'u',
                    "caron" => 'v',
                    _ => return None,
                };
                accented(mark, letter)?
            }
        }
    };
    Some((c, end + 1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titles_composers_and_lyrics_but_not_the_notes() {
        let text = index_text(
            "X:1\nT:Kanon\nT: in D\nC:Johann Pachelbel\nM:4/4\nL:1/4\nK:D\n\
             V:S name=\"Sopran\"\nf2 e2 | d2 c2 |\nw:Lob und Dank\nV:A\nd2 c2 |\n\
             W:Zweite Strophe\nN:not indexed\n",
        );
        assert_eq!(
            text,
            "Kanon\nin D\nJohann Pachelbel\nLob und Dank\nZweite Strophe\n"
        );
        assert_eq!(index_text("K:G\nGABc|\n"), "", "only notes");
        assert_eq!(index_text(""), "");
    }

    #[test]
    fn comments_and_directives_are_left_out() {
        let text = index_text("%%score S A\nT:Lied % mein Kommentar\n% T:kein Titel\nw:a\\%b\n");
        assert_eq!(text, "Lied\na%b\n");
    }

    #[test]
    fn syllables_are_joined_into_words() {
        let text = index_text(
            "w:Der Mond ist auf-ge-gan-gen, die__ gold-nen *\nw:Stern-lein~und | pran--gen\n",
        );
        assert_eq!(
            text,
            "Der Mond ist aufgegangen, die goldnen\nSternlein und prangen\n"
        );
        assert_eq!(
            index_text("w:hoch\\-zeit\n"),
            "hoch-zeit\n",
            "a written hyphen"
        );
        assert_eq!(
            index_text("W:Rot-Kreuz\n"),
            "Rot-Kreuz\n",
            "no syllables after the tune"
        );
    }

    #[test]
    fn a_word_goes_on_in_the_next_lyrics_line() {
        let text =
            index_text("w:Der Mond ist auf-\nGA|Bc|\nw:ge-gan-gen\nV:A\nw:end-\nT:Neu\nw:los\n");
        assert_eq!(text, "Der Mond ist aufgegangen\nend\nNeu\nlos\n");
    }

    #[test]
    fn plus_lines_continue_a_field() {
        let text = index_text("T:Ein langer\n+:Titel\nw:erste\n+:Zeile\nK:G\n+:nicht\n");
        assert_eq!(text, "Ein langer Titel\nerste Zeile\n");
    }

    #[test]
    fn escapes_and_entities_give_letters() {
        let text = index_text(
            "T:Gr\\\"u\\ss Gott\nC:Dvo\\vr\\'ak\nW:&Auml;pfel &amp; Birnen &#252;ber &#xE9;t&eacute;\n\
             W:\\u00e4 \\U0001F3B5 \\q &bogus; & \\\\\n",
        );
        assert_eq!(
            text,
            "Grüß Gott\nDvořák\nÄpfel & Birnen über été\nä 🎵 \\q &bogus; & \\\n"
        );
    }

    /// format.md §5.6 rule 5: the golden files in
    /// `tests/fixtures/format/5.6-index/`.
    #[test]
    fn index_text_of_the_fixtures() {
        let fixtures = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/format/5.6-index");
        let read = |name: &str| std::fs::read_to_string(fixtures.join(name)).unwrap();
        for name in ["score", "fragment"] {
            assert_eq!(
                index_text(&read(&format!("{name}.abc"))),
                read(&format!("{name}.txt")),
                "{name}"
            );
        }
    }
}
