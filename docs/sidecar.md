# The RevuCraft sidecar: `markups.json`, format 1

Status: format 1, proposed 2026-11-01 (agreed with Steve: sidecar transport; punch status only;
per-PDF folder; tasks carry their space). Implementation: `crates/annot/src/sidecar.rs`.

## Purpose

RevuCraft writes one small JSON file beside each drawing's folder on save, so PlanCraft can read
the punch list and spaces without parsing the PDF. PlanCraft only reads it. Nothing in the
sidecar is a source of truth for RevuCraft: the PDF keeps the data, and the file is rebuilt from
it.

## Where it goes

Beside the PDF, in its job folder:

```
Smith Residence.pdf
Smith Residence/
    markups.json        <- this file
    notes/
    photos/
```

PlanCraft's link to the drawing is its relative path (the per-PDF folder, decided 2026-11-01).

## Rules

- **UTF-8 JSON**, two-space indent, one trailing newline.
- **Deterministic.** The same markups give the same bytes: no timestamps, no device names, rows in
  page order and then drawing order (annotation index on the page).
- **Written atomically** (a temporary file, then a rename) and **skipped when the bytes would not
  change**, so Syncthing sees no churn.
- **Written on every save** of the drawing in RevuCraft.
- **Only RevuCraft writes it.** PlanCraft never writes this file.
- Unknown fields must be ignored by readers. New fields in a later version come with a higher
  `version`.

## Format 1

```json
{
  "format": "revucraft-markups",
  "version": 1,
  "punch": [
    {
      "name": "3f1c…",
      "page": 0,
      "no": 3,
      "assignee": "Electrician",
      "due": "2026-11-02",
      "status": "Open",
      "closed": false,
      "condition": "Installed",
      "space": "Server room"
    }
  ],
  "spaces": [
    { "name": "9ab2…", "page": 0, "label": "Server room" }
  ]
}
```

### Top level

| Field | Type | Meaning |
|---|---|---|
| `format` | string | Always `"revucraft-markups"`. |
| `version` | integer | `1` for this format. |
| `punch` | array | One row per punch item, in page order. |
| `spaces` | array | One row per space, in page order. |

### Punch rows

| Field | Type | Meaning |
|---|---|---|
| `name` | string | The markup's permanent name (its PDF `/NM`). The item's identity: stable when other markups are added or removed. Every row has one; a punch item without a name is left out. |
| `page` | integer | 0-based page index. |
| `no` | integer | The punch number on the sheet (the badge), 1 to 999999. |
| `assignee` | string | Who it is for; empty when none. |
| `due` | string | The due date as `YYYY-MM-DD`; empty when none. |
| `status` | string | One of `Open`, `InProgress`, `Ready`, `Closed`. |
| `closed` | boolean | `true` when `status` is `Closed`. Set so readers need not compare strings. |
| `condition` | string or null | The markup's condition (`Existing`, `Proposed`, `Installed`, `Remove`, or the user's own word); `null` when none. |
| `space` | string or null | The label of the smallest space on the same page that holds the markup's middle point; `null` when none. Used for a task's location. |

### Space rows

| Field | Type | Meaning |
|---|---|---|
| `name` | string | The space's permanent name (`/NM`). |
| `page` | integer | 0-based page index. |
| `label` | string | The space's name as drawn (its `/Contents`). It is what a punch row's `space` holds. |

## Not in format 1

Deferred by decision on 2026-11-01:

- Ticking tasks from RevuCraft (readers must not expect task writes from RevuCraft).
- Dashboard contents (the summary panel is a later format, in its own repository).
- Checklist progress from notes.

## Changes

- 2026-11-01: format 1 proposed.
