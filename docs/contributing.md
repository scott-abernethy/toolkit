# Contributing

## Prerequisites

- [asdf](https://asdf-vm.com) with the rust plugin: `asdf plugin add rust && asdf install`
- [just](https://github.com/casey/just): `brew install just`
- [gitleaks](https://github.com/gitleaks/gitleaks): `brew install gitleaks`
- `~/.cargo/bin` on your `PATH`: add `export PATH="$HOME/.cargo/bin:$PATH"` to your `~/.zshrc`

## Setup

After cloning, run the one-time setup to configure git hooks:

```sh
just setup
```

This enables a pre-commit hook that scans staged changes for secrets using gitleaks. The hook warns but doesn't block if gitleaks isn't installed.

## Security

This repo is **public** and the tools handle credentials. Follow these rules:

- **Never commit real credentials, tokens, or passwords.** Use placeholders like `changeme`, `dapi...`, `dbc-abc123` in examples and templates.
- **Never commit config files** containing real connection details. The `.gitignore` blocks `config.yaml`, `*.key`, `*.pem`, `*.env`, and similar patterns.
- **Use your personal email** for git commits, not a work email. Check with `git config user.email` before your first commit.
- **Gitleaks runs on every commit** via the pre-commit hook. If it flags a false positive in example/template code, add an allowlist entry to `.gitleaks.toml`.

GitHub's **push protection** (secret scanning) is also enabled on the remote as a second line of defence.

## Development

```sh
just build    # build all tools
just test     # run all tests
just lint     # clippy
just fmt      # format
just install  # install binaries to ~/.cargo/bin
```

## Private Homebrew Tap Releases

Tagging with `v*` triggers `.github/workflows/release-private-tap.yml` to:

1. Build release binaries (`toolkit`, `tkpsql`, `tkmsql`, `tkdbr`) for macOS Intel + Apple Silicon
2. Upload tarballs to the GitHub release
3. Regenerate `Formula/toolkit.rb` in your private tap repository

Required repository settings in this repo:

- Variable `BREW_TAP_REPO` (format: `owner/homebrew-tap-repo`)
- Secret `BREW_TAP_PAT` (token with write access to the tap repo)

## Adding a New Tool

1. `cargo init crates/<name>` (e.g. `crates/foo` → binary `tkfoo`)
2. Add `"crates/<name>"` to `members` in the root `Cargo.toml`
3. Add `common = { path = "../common" }` to the new crate's dependencies
4. Add a `[name]` section to `~/.config/toolkit/config.yaml` if the tool needs config
5. Use `common::load_section::<MyConfig>("name")` to load it
6. See `AGENTS.md` for output conventions and token efficiency guidelines
