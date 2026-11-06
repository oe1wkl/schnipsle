# ADR-0027: Pictures few programs show are converted for an export

- Status: Accepted; amended by [ADR-0028](0028-turned-pictures-upright-in-docx-and-odt.md) (turned pictures in DOCX and ODT)
- Date: 2026-11-05
- Deciders: Willi Kraml
- Related: F-06, F-13, ADR-0010, ADR-0015, ADR-0021, ADR-0025

## Context

ADR-0021 rule 5 puts a note's pictures into the export as they are. An export of 538 real notes showed that this loses some of them without a word. HEIC pictures, as the iPhone takes them, and TIFF pictures pasted on a Mac show in Safari, in Apple's apps and in PDF, which WebKit prints on macOS. Elsewhere they do not:

- DOCX: LibreOffice leaves HEIC out entirely. Pandoc does not know the type and stores it as `rIdN.so` with `application/octet-stream`.
- ODT: LibreOffice shows an empty frame with a broken-picture icon for HEIC.
- EPUB: HEIC and TIFF are no core media types of EPUB, and Pandoc marks HEIC as `application/octet-stream`.
- HTML: browsers built on Chromium or Firefox show neither HEIC nor TIFF.

The report said nothing, as the picture was there.

## Decision

An export converts HEIC, HEIF and TIFF pictures to JPEG, or to PNG if the file says it has transparency, in every format. The picture is turned upright by its orientation and keeps its full size and its colour profile. JPEG quality is 0.9. The vault is not touched. A folder export writes the converted copy next to where the original would go, with the new extension added (`attachments/ab/….heic.jpg`), and not the original.

The conversion uses `ImageIO` in `schnipsle-apple`, which works on macOS and iOS on any thread, so `schnipsle-export` uses that crate on Apple platforms. Where there is no converter, as on Linux for now, or the file cannot be read, the picture stays as it is, and the report names it as `pictureNotConverted`.

Other types are left alone: PNG, JPEG, GIF and SVG show everywhere, and WebP and AVIF in current browsers, LibreOffice and EPUB 3.3. Whether Word shows WebP is checked when Word is at hand.

## Options considered

| Option | Pros | Cons |
| --- | --- | --- |
| Convert with the system's `ImageIO` (chosen) | Every reader sees the pictures; no new library; the same on iOS later | Apple only; a JPEG is not the original file |
| Report only | Small; exports stay byte for byte | The pictures stay missing for most readers |
| A Rust image library | Works on every platform | No permissive decoder for HEIC (libheif is LGPL, ADR-0005); TIFF and WebP alone do not solve it |
| Convert in Pandoc | No code of ours | Pandoc cannot read HEIC; it would not help HTML, ODT or PDF |

## Consequences

- Positive: an export of iPhone photos opens with its pictures in Word, LibreOffice, e-book readers and any browser.
- Negative: converted pictures are not the original files, and larger: a HEIC screenshot of 150 kB became a JPEG of 430 kB, and a HEIC with transparency of 0.5 MB a PNG of 6.4 MB. An export takes a little longer for each such picture. A HEIC photo in a PDF is converted although WebKit could show it, for one rule in all formats.
- Follow-up work: a converter on Linux and Windows when exports come there. ADR-0021 rule 5 is amended by this.
