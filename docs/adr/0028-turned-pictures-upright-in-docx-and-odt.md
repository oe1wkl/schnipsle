# ADR-0028: Turned pictures are upright in DOCX and ODT

- Status: Accepted
- Date: 2026-11-06
- Deciders: Willi Kraml
- Related: F-06, F-13, ADR-0021, ADR-0025, ADR-0027

## Context

A camera stores a photo as its sensor saw it and writes into the file how to turn it for showing: the orientation of EXIF and TIFF. A test export with such a JPEG, stored landscape and to be shown upright, showed it upright in the PDF, in HTML in Chrome and in the EPUB in Books. LibreOffice and WordCraft ignored the orientation and showed it on its side, in DOCX and in ODT alike.

ADR-0027 converts HEIC, HEIF and TIFF and turns them upright on the way, so only the other types, JPEG above all, are affected.

## Decision

A DOCX or ODT export turns a picture upright whose orientation says to show it turned or mirrored, any value other than 1. It is converted as in ADR-0027: a JPEG of quality 0.9, or a PNG if the file has transparency, at full size and with its colour profile. HTML, PDF and EPUB keep the original file, which shows upright there.

`schnipsle-apple` tells such a picture by its orientation with `ImageIO` (`is_turned`). Where there is no converter, the picture stays as it is; without `ImageIO` there is no telling either, so the report says nothing.

## Options considered

| Option | Pros | Cons |
| --- | --- | --- |
| DOCX and ODT only (chosen) | Upright where it was not; the other formats keep the original file | One more rule per format |
| Every format, like ADR-0027 | One rule for all | HTML and EPUB get a re-encoded copy where the original shows upright |
| Leave it | Nothing to do | Photos lie on their side in office programs |

## Consequences

- Positive: photos from a phone stand upright in LibreOffice, WordCraft and other office programs.
- Negative: in DOCX and ODT such a JPEG is encoded again, which costs a little quality and time, and its metadata, the place it was taken included, is not carried over.
- Follow-up work: a converter on Linux and Windows when exports come there, as for ADR-0027. ADR-0027 is amended by this.
