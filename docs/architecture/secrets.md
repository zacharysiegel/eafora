# Secrets

> **Status: draft, 2026-08-19.** The contract between `secr`, `.env`, and the ingestion binary. Written because the wiring works but was only inferable by reading `setup.sh`, `secrets.yaml`, and `ingestion/src/secrets.rs` together.

## The rule

**A secret is a value that grants access. Everything else is an identifier, however obscure it looks.**

- Secrets live encrypted in `secrets.yaml`, are decrypted at runtime through `secr`, and never appear in plaintext in the repository or in a shell history.
- Identifiers live in `.env`, in plaintext. Account ids, bucket names, usernames, base URLs.

The distinction is not about how sensitive a value feels. A username identifies an account and grants nothing on its own; a password grants access. Both halves of one credential can therefore end up in different places, which is the part that surprises people.

Worked examples:

| Value | Where | Why |
|---|---|---|
| `R2_ACCOUNT_ID` | `.env` | identifies an account |
| `R2_ARTIFACT_BUCKET` | `.env` | names a bucket |
| `R2_PUBLISH_ACCESS_KEY_ID` | `.env` | names a key, grants nothing alone |
| `cloudflare.r2.publish.secret_access_key` | `secrets.yaml` | grants write access to the bucket |

`.env` is gitignored, so an identifier that is also personal data (an account email, say) belongs there rather than in `template.env`.

## How a secret is read

`ingestion/src/secrets.rs` exposes two functions, keyed by the dotted name under which the secret is stored:

```rust
secrets::master_decrypt(name) -> Result<Vec<u8>, AppError>
secrets::master_decrypt_utf8(name) -> Result<String, AppError>
```

Both need two environment variables, read through `dotenvy`:

- `MASTER_SECRET` — the base64 master key every secret in the store is encrypted against.
- `SECR_STORE_PATH` — the store's path, `secrets.yaml` by default.

The store is a `LazyLock`, loaded once on first use and `expect`ed. A missing or unreadable store therefore panics at the moment a secret is first needed, not at startup, and the panic names the store rather than the caller.

## Two dependencies on the working directory

Both of these are load-bearing and easy to break.

**`dotenvy::var` searches for `.env` from the current directory upward.** It is not the process environment alone: `var` calls `dotenv()` once behind a `call_once`, and a missing `.env` is swallowed (`dotenv().ok()`), after which the lookup falls back to the real environment and fails only if the variable is absent there too. Run the binary from outside the repository with no `.env` in an ancestor directory and every secret lookup fails with a missing-variable error rather than anything mentioning `.env`.

**`SECR_STORE_PATH` is relative.** `template.env` sets it to `secrets.yaml`, resolved against the current directory.

The scheduled job satisfies both by setting `WorkingDirectory` to the repository root in `ingestion/eafora-ingestion.plist.template`. Its `EnvironmentVariables` block carries only `RUST_LOG`; `MASTER_SECRET` reaches the job through `.env` and the working directory, not through the plist. A launchd job inherits no shell environment, so moving or removing that `WorkingDirectory` breaks decryption in a way nothing else would explain.

## `setup.sh` regenerates `.env` on every run

`setup.sh` rewrites `.env` from `template.env` each time it runs, substituting exactly one value: `MASTER_SECRET`, taken from its first argument or from the existing `.env`. Everything else comes from the template.

**Anything hand-edited into `.env` is destroyed by the next `setup.sh`.** A new identifier therefore belongs in `template.env`, committed, with `.env` regenerated. Only `MASTER_SECRET` survives.

## Adding a secret

`secr` is installed as a CLI (`cargo install secr`). Encrypt against the same master key the store already uses, and give the secret the dotted name the code will ask for:

```sh
secr encrypt --key "$MASTER_SECRET" --name hfd.password '<the-password>'
```

Paste the resulting `nonce` and `ciphertext` pair into `secrets.yaml` under that name. `secrets.yaml` is committed: its contents are ciphertext, and the master key is what is secret.

Run the command in your own terminal rather than through an agent, and prefer a form that keeps the plaintext out of shell history.

To read one back:

```sh
secr decrypt --key "$MASTER_SECRET" hfd.password
```

## Inventory

| Name | Read by | Purpose |
|---|---|---|
| `cloudflare.r2.publish.token` | ingestion | Cloudflare API token for the artifact bucket |
| `cloudflare.r2.publish.secret_access_key` | ingestion, `publish cloudflare-r2` | S3 secret for uploading artifacts |

Nothing outside `ingestion` reads a secret. The web client has none by construction: it is a static deploy serving public artifacts, and a secret shipped to a browser would not be one.

## When adding an upstream source that needs credentials

1. Put the identifier in `template.env`, regenerate `.env` through `setup.sh`.
2. Encrypt the secret into `secrets.yaml` under a dotted name that reads as `<vendor>.<system>.<purpose>`.
3. Read it in the source's `_client.rs` via `secrets::master_decrypt_utf8`, at the point of use rather than at startup, so a source that is not being run needs no credential present.
4. Add a row to the inventory above.
