# Welcome {#note-welcome}

This vault holds **sample notes** for the exporters, with *emphasis*,
~~strikethrough~~ and `inline code`. A line ends here\
with a hard break.

## Links

- A page on the web: [Schnipsle](https://github.com/oe1wkl/schnipsle)
- An address: [write to the club](mailto:club@example.org)
- A bare one: [www.example.org](http://www.example.org) and <https://example.org/qrp>
- A footnote.[^note-welcome-1]

### Tags in the text

Tags such as \#radio and \#projects/tuner stay text in an export.

[^note-welcome-1]:
    Footnotes go to the end of the note.

```{=openxml}
<w:p><w:r><w:br w:type="page"/></w:r></w:p>
```

# Packing list for Field Day {#note-packing-list-for-field-day}

1. Radio
   - Keyer
   - Spare fuses
2. Antenna
3. Logbook

<!-- end list -->

- ☑ Charge the batteries
- ☐ Print the band plan

> Leave nothing behind but footprints.

-----

A literal \#notatag, a colour \#00ff00 and C\# stay as written.

Bring the \#keyer and \#m32.

```{=openxml}
<w:p><w:r><w:br w:type="page"/></w:r></w:p>
```

# Antenna tuner {#note-antenna-tuner}

The finished box:

![Front view of the tuner](picture-1.png)

The circuit is in the datasheet.

\[Inside, not photographed yet\]

[A picture on the web](https://example.org/images/tuner.jpg)

See the [welcome note](#note-welcome), the [Packing list for Field Day](#note-packing-list-for-field-day)
and last year's log.

```{=openxml}
<w:p><w:r><w:br w:type="page"/></w:r></w:p>
```

# Bread rolls {#note-bread-rolls}

![Rolls on the tray](picture-2.jpg)

| Ingredient | Amount | Note |
| :-- | --: | :-: |
| Flour | 500 g | type 700 |
| Water | 320 g | cold |
| Salt | 10 g |  |

```text
Bake at 230 °C for 20 minutes.
```

    Indented code is code too.

```{=openxml}
<w:p><w:r><w:br w:type="page"/></w:r></w:p>
```

# Abendlied {#note-abendlied}


```{=openxml}
<w:p><w:pPr><w:keepNext/><w:spacing w:before="120" w:after="0"/></w:pPr><w:r><w:rPr><w:b/><w:sz w:val="28"/></w:rPr><w:t xml:space="preserve">Abendlied</w:t></w:r></w:p>
<w:p><w:pPr><w:spacing w:before="0" w:after="0"/></w:pPr><w:r><w:rPr><w:color w:val="6E6E73"/><w:sz w:val="18"/></w:rPr><w:t xml:space="preserve">Key G</w:t></w:r></w:p>
<w:p><w:pPr><w:keepNext/><w:spacing w:before="0" w:after="0"/></w:pPr><w:r><w:rPr><w:rFonts w:ascii="Courier New" w:hAnsi="Courier New" w:cs="Courier New"/><w:b/><w:color w:val="0A66D8"/><w:sz w:val="20"/></w:rPr><w:t xml:space="preserve">    G             D</w:t></w:r></w:p>
<w:p><w:pPr><w:spacing w:before="0" w:after="0"/></w:pPr><w:r><w:rPr><w:rFonts w:ascii="Courier New" w:hAnsi="Courier New" w:cs="Courier New"/><w:sz w:val="20"/></w:rPr><w:t xml:space="preserve">Der Mond ist aufgegangen,</w:t></w:r></w:p>
<w:p><w:pPr><w:keepNext/><w:spacing w:before="0" w:after="0"/></w:pPr><w:r><w:rPr><w:rFonts w:ascii="Courier New" w:hAnsi="Courier New" w:cs="Courier New"/><w:b/><w:color w:val="0A66D8"/><w:sz w:val="20"/></w:rPr><w:t xml:space="preserve">    G                 C   D</w:t></w:r></w:p>
<w:p><w:pPr><w:spacing w:before="0" w:after="0"/></w:pPr><w:r><w:rPr><w:rFonts w:ascii="Courier New" w:hAnsi="Courier New" w:cs="Courier New"/><w:sz w:val="20"/></w:rPr><w:t xml:space="preserve">die goldnen Sternlein prangen</w:t></w:r></w:p>
```

![Kanon](picture-3.svg)

```mermaid
graph LR
  Rig --> Tuner --> Antenna
```

Inline math `$E = mc^2$` and a block:

    $$
    f = \frac{c}{\lambda}
    $$

```{=openxml}
<w:p><w:r><w:br w:type="page"/></w:r></w:p>
```

# Raw HTML {#note-raw-html}

Text with <b>bold</b>, <kbd>Ctrl</kbd>, <span>a span</span> and H<sub>2</sub>O.

A div with a handler; its text stays.

