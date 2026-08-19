# Secrets

> **Status: draft, 2026-08-19.** How `secr`, `.env`, and the ingestion binary fit together.

## Where things go

Secrets go in `secrets.yaml` through `secr`. Everything else goes in `.env`.

Both halves of one credential can split: `R2_PUBLISH_ACCESS_KEY_ID` lives in `.env`, `cloudflare.r2.publish.secret_access_key` in `secr`.

Names read `<vendor>.<account>.<purpose>`.

## Reading one

```rust
secrets::master_decrypt(name) -> Result<Vec<u8>, AppError>
secrets::master_decrypt_utf8(name) -> Result<String, AppError>
```

Needs `MASTER_SECRET` (base64 master key) and `SECR_STORE_PATH` from `.env`. The store is a `LazyLock` with an `expect`, so a missing store panics when a secret is first read rather than at startup.

Read secrets at the point of use, not at startup, so a source that is not running needs no credential present.

## Working-directory dependencies

`dotenvy::var` searches for `.env` upward from the current directory, and swallows a missing file before falling back to the process environment — so running from elsewhere fails with a missing-variable error that never mentions `.env`. `SECR_STORE_PATH` is relative too.

The scheduled job satisfies both through `WorkingDirectory` in `ingestion/eafora-ingestion.plist.template`. Its `EnvironmentVariables` block carries only `RUST_LOG`, and launchd inherits no shell environment, so removing that `WorkingDirectory` breaks decryption.

## `.env` is regenerated

`setup.sh` rewrites `.env` from `template.env` on every run, preserving only `MASTER_SECRET`. Put new identifiers in `template.env`, not `.env`.

## Adding a secret

```sh
secr encrypt --key "$MASTER_SECRET" --name hfd.siegelzc.password '<the-password>'
```

Paste the resulting `nonce` and `ciphertext` into `secrets.yaml`. The file is committed; the ciphertext is what is in it and the master key is what is not. Run this in your own terminal.

```sh
secr decrypt --key "$MASTER_SECRET" hfd.siegelzc.password
```

## Inventory

| Name | Purpose |
|---|---|
| `cloudflare.r2.publish.token` | Cloudflare API token for the artifact bucket |
| `cloudflare.r2.publish.secret_access_key` | S3 secret for uploading artifacts |
| `hfd.siegelzc.password` | Human Fertility Database account, for downloading data files |

Only `ingestion` reads secrets. The web client has none: it is a static deploy of public artifacts.
