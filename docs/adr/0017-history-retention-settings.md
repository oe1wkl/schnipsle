# ADR-0017: History retention settings in `settings.yaml`

- Status: Accepted
- Date: 2026-10-21
- Deciders: Willi Kraml
- Related: ADR-0011, [format.md §7](../format.md#7-version-history)

## Context

ADR-0011 and format.md §7 say the retention of history snapshots is configurable in `.notesapp/settings.yaml`, but name no keys, and §7 gives the daily period as "3 months", which is not a whole number of days. Settings in the vault sync to every device, so all devices thin the history the same way.

## Decision

`settings.yaml` holds two top-level keys, both whole numbers of days:

```yaml
history_keep_all_days: 7      # every snapshot is kept this long
history_keep_daily_days: 90   # then the last of each UTC day until this age
```

Older snapshots keep the last of each ISO week. A missing or invalid value means its default (7 and 90); a daily period shorter than the first is extended to it. "3 months" in §7 becomes 90 days. Unknown keys in `settings.yaml` are ignored and never removed (format.md §1).

## Options considered

| Option | Pros | Cons |
| --- | --- | --- |
| Two flat keys (chosen) | Readable, editable by hand, fits the flat YAML the app already reads | No grouping under a `history:` section |
| Nested `history:` map | Groups settings | Needs a fuller YAML reader for one feature |
| No setting, fixed defaults | Nothing to specify | Contradicts ADR-0011; users with large vaults cannot keep less |

## Consequences

- Positive: retention can be tuned per vault, the same on every device.
- Negative: a typo silently means the default; a settings UI would show the values in effect.
- Follow-up work: a settings view in the app (not in M1).
