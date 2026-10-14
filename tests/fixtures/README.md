# Test fixtures

Golden files: an input and the exact expected output, compared byte for byte in tests.

- `format/` — conformance cases for every MUST rule in `docs/format.md`.
- `import/<source>/` — anonymized sample exports and the expected notes (from M4).
- `export/<format>/` — sample notes and the expected HTML, ODT and PDF text (from M5).

Files here are stored without line-ending conversion (see `.gitattributes`).
