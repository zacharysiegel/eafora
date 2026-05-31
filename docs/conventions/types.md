# Type naming convention

Source of truth for how Rust types are named and laid out across the Eafora codebase. Memory files reference this document; they don't restate it.

## Core dichotomy

Every DB-touched type has two shapes:

- **Domain** — what application code uses. Typed enums for constrained text columns. Bare-named (`Country`, `DataSource`, `CandidateValue`).
- **Wire** — what the database round-trips. `String` for text columns, primitive types for everything else. Suffixed (`Entity` for table-row mirrors, `Projection` for joins).

The two shapes are always two distinct structs, even when their fields are identical. The conversion lives next to the wire type as `From<Wire> for Domain` (infallible) or `TryFrom<Wire> for Domain` (when any field needs parsing into a typed enum).

The reason for keeping the pair even when fields are identical: uniformity. Reading any `_model.rs` file, you can scan for the `Entity` types and immediately see "these are the table mirrors, here's where parsing lives." Skipping the `Entity` for parse-free types makes that reading harder. The trivial `From` impl is the right amount of ceremony for the parse-free case.

## Decision tree

```
Is it a 1:1 mirror of a table row?
├── yes → `<Name>` (domain) + `<Name>Entity` (wire)
│         + `(Try)From<Entity> for Domain`
└── no (join, subset, or computed projection) →
    Is the projection meaningful as a domain object (used by name elsewhere)?
    ├── yes → `<Name>` (domain) + `<Name>Projection` (wire)
    │         + `(Try)From<Projection> for Domain`
    └── no  → `<Name>Projection` only (consumed inline at the call site,
              fields read directly into something else like a `BTreeMap`)
```

## Single-column queries

No struct. Use `query_scalar!` and bind to the primitive type directly.

## HTTP wire formats

Direction is the primary axis. Redaction is a secondary modifier and composes onto direction:

- `<Domain>SerialOut` — server → client (response bodies).
- `<Domain>SerialIn` — client → server (request bodies). Often differs from `Out` because the client doesn't supply server-generated fields like `id` or `created`.
- `<Domain>Serial` — only when In and Out are byte-identical for a given endpoint. Don't reach for this just to save a struct; if there's any chance In and Out will diverge later, write them separately from the start.
- `<Domain>PublicSerialOut`, `<Domain>InternalSerialOut`, etc. — context modifiers for redaction. Don't introduce these speculatively; wait until there's a real second consumer.
- Conversions:
  - `impl From<&Domain> for DomainSerialOut` (always infallible)
  - `impl TryFrom<DomainSerialIn> for Domain` (client input can fail validation)

This diverges from Singularity, which uses `Serial` for outbound and `PublicSerial` for the redacted variant. The In/Out split is more honest about why two structs exist when they do.

## Enums

Two flavors:

- **`<Noun>Kind`** when the enum enumerates the values of a `code`-style column. Method `code()` returns the wire string. Examples: `DataSourceKind`, `StatisticKind`.
- **Bare descriptive name** when the type name already conveys the role. Method `as_str()` returns the wire string. Examples: `LicenseClass`, `DataStatus`, `LicenseShardClass`.

Always implement `FromStr` — gives both `T::from_str(s)` and `s.parse::<T>()`.

## Variable naming inside `<feature>_db.rs`

- `record` (singleton) and `records` (collection) for sqlx query results, bare.
- Scoped to db.rs files only. Elsewhere the typed-prefix rule from `~/.claude/CLAUDE.md` applies (`country_records`, etc.).

## Conversion impl placement

- In `<feature>_model.rs`, immediately after the wire-format type.
- `From` when infallible. `TryFrom` when any field needs parsing.
- Callers in db.rs use:
  - `record.map(Domain::from).transpose()` (or `try_from`) for `Option<Entity>`
  - `records.into_iter().map(Domain::from).collect()` (or `try_from`) for `Vec<Entity>`

## Audit checklist (run after any type rename or shape change)

1. Grep for `*_code`, `*_id`, `*_str`, `by_code`-style names that may now hold a different type.
2. Variable/field name should describe the type that's there now, not the type that was there before.
3. Const definitions whose value is a single enum variant are usually redundant — inline.
