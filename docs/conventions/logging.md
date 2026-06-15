# Logging conventions

Format for any `log::*!` call: `<human-readable message>; [key=value key=value]`. The semicolon separates prose from structured data; the brackets group all key-value pairs. Drop the `; [...]` section entirely when the message has no structured data — prose-only messages stand on their own.

## Examples

```rust
// With structured data — semicolon, brackets, key=value pairs:
log::debug!("uploaded shard; [key={}]", key);
log::debug!("uploaded manifest; [key={} url={}]", manifest_key, manifest_url);
log::info!("inserted artifact_version; [id={} version_label={}]", artifact_version.id, artifact_version.version_label);

// Prose only — no semicolon, no brackets:
log::info!("publish complete");
log::warn!("source {} failed: {}", source_kind.code(), error);
```

## Wrong

```rust
log::debug!("uploaded shard key={}", key);                                  // no separator, no brackets
log::debug!("uploaded shard, key={}", key);                                 // wrong separator (comma)
log::info!("inserted artifact_version id={} label={}", id, label);          // no brackets
log::info!("publish complete;");                                            // dangling semicolon (no bracket section)
```

## Rationale

The semicolon-and-brackets convention makes the boundary between prose and structured data visible to humans scanning logs and consistent for any future log-parsing tooling. Without the separator, key=value runs together with the message; without brackets, it's ambiguous where the structured section ends.

This applies to every level (`error!`, `warn!`, `info!`, `debug!`, `trace!`) and to both library and binary code.
