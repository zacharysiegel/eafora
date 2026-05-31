# Type naming convention

Source of truth for how Rust types are named and laid out across the Eafora codebase. Memory files reference this document; they don't restate it.

The **type-naming rules** below are strict: same struct shape always gets the same suffix (`Entity`, `Projection`, `SerialOut`, etc.). Drift here makes a `_model.rs` file unreadable.

The **variable-naming guidance** at the end is softer — a default to fall back to, not a rule to enforce in code review.

## Core dichotomy

Every DB-touched type has two shapes:

- **Model** — what application code uses. Typed enums for constrained text columns. Bare-named (`Country`, `DataSource`, `CandidateValue`).
- **Wire** — what the database round-trips. `String` for text columns, primitive types for everything else. Suffixed (`Entity` for table-row mirrors, `Projection` for joins).

The two shapes are always two distinct structs, even when their fields are identical. The conversion lives next to the wire type as `From<Wire> for Model` (infallible) or `TryFrom<Wire> for Model` (when any field needs parsing into a typed enum).

The reason for keeping the pair even when fields are identical: uniformity. Reading any `_model.rs` file, you can scan for the `Entity` types and immediately see "these are the table mirrors, here's where parsing lives." Skipping the `Entity` for parse-free types makes that reading harder. The trivial `From` impl is the right amount of ceremony for the parse-free case.

## Decision tree

```
Is it a 1:1 mirror of a table row?
├── yes → `<Name>` (model) + `<Name>Entity` (wire)
│         + `(Try)From<Entity> for Model`
└── no (join, subset, or computed projection) →
    Is the projection meaningful as a model object (used by name elsewhere)?
    ├── yes → `<Name>` (model) + `<Name>Projection` (wire)
    │         + `(Try)From<Projection> for Model`
    └── no  → `<Name>Projection` only (consumed inline at the call site,
              fields read directly into something else like a `BTreeMap`)
```

## Single-column queries

No struct. Use `query_scalar!` and bind to the primitive type directly.

## HTTP wire formats

Direction is the primary axis. Redaction is a secondary modifier and composes onto direction:

- `<Model>SerialOut` — server → client (response bodies).
- `<Model>SerialIn` — client → server (request bodies). Often differs from `Out` because the client doesn't supply server-generated fields like `id` or `created`.
- `<Model>Serial` — only when In and Out are byte-identical for a given endpoint. Don't reach for this just to save a struct; if there's any chance In and Out will diverge later, write them separately from the start.
- `<Model>SerialOutPublic`, `<Model>SerialOutInternal`, etc. — context modifiers for redaction, suffixed after the direction so every variant of the same direction shares a common prefix (`<Model>SerialOut...`). Don't introduce these speculatively; wait until there's a real second consumer.
- Conversions:
  - `impl From<&Model> for ModelSerialOut` (always infallible)
  - `impl TryFrom<ModelSerialIn> for Model` (client input can fail validation)

This diverges from Singularity, which uses `Serial` for outbound and `PublicSerial` for the redacted variant. The In/Out split is more honest about why two structs exist when they do.

## Enums

Two flavors:

- **`<Noun>Kind`** — when the bare name would shadow or be confused for a related struct. Without the suffix, a reader seeing `DataSource` (or `Statistic`, `Region`, etc.) reasonably assumes a data-bearing struct; the `Kind` suffix is the signal that this is an enumeration of variant tags. Use `Kind` whenever a related struct of the bare name exists, OR when the bare name would otherwise read as a noun-describing-a-thing rather than a noun-describing-a-classification.
- **Bare descriptive name** — when the type name already unambiguously reads as "an enumeration of values": `LicenseClass`, `DataStatus`, `LicenseShardClass`. The `Class` / `Status` / etc. suffix is doing the work `Kind` would. No `Kind` needed.

Method naming for the wire-string direction:

- `<column>()` — when the enum maps cleanly to a single named column whose name reads naturally as a method (`code()` for a `code` column). Examples: `DataSourceKind::code()`, `StatisticKind::code()`.
- `as_str()` — fallback for everything else. Examples: `LicenseClass::as_str()`, `DataStatus::as_str()`, `LicenseShardClass::as_str()`.

Always implement `TryFrom<&str>` for the wire-string → enum direction. This keeps the wire→model idiom uniform across boundaries: `Model::try_from(wire)` works whether `wire` is an `Entity`, a `Projection`, or a `&str` column value. Don't implement `FromStr` instead — the `parse::<T>()` shortcut isn't load-bearing here, and having two near-equivalent traits (`FromStr` for strings, `TryFrom<&str>` for everything else) just splits the codebase's idiom in half.

## Variable naming (guidance, not strict)

When in doubt, name variables after the type they hold, lowercase, plural for collections. The wire-format suffix (`Entity`, `Projection`) is part of the variable name; this avoids the redundancy of `record: AccountEntity` (where "record" and "Entity" both signal "DB-row-shape").

```rust
let account_entity: Option<AccountEntity> = sqlx::query_as!(...).fetch_optional(executor).await?;
account_entity.map(Account::try_from).transpose()

let account_entities: Vec<AccountEntity> = sqlx::query_as!(...).fetch_all(executor).await?;
account_entities.into_iter().map(Account::try_from).collect()
```

This is just the typed-prefix rule from `~/.claude/CLAUDE.md` applied without a special carve-out. Diverges from Singularity (which uses bare `record`/`records` in db.rs) because Singularity didn't have to differentiate `Entity` vs `Projection`; the redundancy didn't bite there.

That said: **type naming is the load-bearing part of this spec; variable naming is preference.** A bare `record` inside a small function is fine if it reads cleanly. Don't burn review time on variable-rename churn.

## Conversion impl placement

- In `<feature>_model.rs`, immediately after the wire-format type.
- `From` when infallible. `TryFrom` when any field needs parsing.
- Callers in db.rs use:
  - `account_entity.map(Account::try_from).transpose()` (or `from`) for `Option<Entity>`
  - `account_entities.into_iter().map(Account::try_from).collect()` (or `from`) for `Vec<Entity>`
