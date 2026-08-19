# Secrets

> **Status: draft, 2026-08-19.** How `secr`, `.env`, and the ingestion binary fit together. `secrets.yaml` lists the secrets and documents adding one; this covers what it does not.

## Where things go

Secrets go in `secrets.yaml` through `secr`. Everything else goes in `.env`.

Both halves of one credential can split: `R2_PUBLISH_ACCESS_KEY_ID` lives in `.env`, `cloudflare.r2.publish.secret_access_key` in `secr`.

Names read `<vendor>.<account>.<purpose>`.

## Reading one

`secrets::master_decrypt_utf8(name)`, or `master_decrypt` for bytes. Both need `MASTER_SECRET` and `SECR_STORE_PATH` from `.env`. The store is a `LazyLock` with an `expect`, so a missing store panics when a secret is first read rather than at startup.

Read at the point of use, not at startup, so a source that is not running needs no credential present.

## Working-directory dependencies

`dotenvy::var` searches for `.env` upward from the current directory, and swallows a missing file before falling back to the process environment — so running from outside the repository fails with a missing-variable error that never mentions `.env`. A relative `SECR_STORE_PATH` resolves against the workspace root the same way, so it holds from any directory inside the tree.

The scheduled job satisfies this through `WorkingDirectory` in `ingestion/eafora-ingestion.plist.template`. Its `EnvironmentVariables` block carries only `RUST_LOG`, and launchd inherits no shell environment, so removing that `WorkingDirectory` breaks decryption.

## `.env` is regenerated

`setup.sh` rewrites `.env` from `template.env` on every run, preserving only `MASTER_SECRET`. Put new identifiers in `template.env`, not `.env`.
