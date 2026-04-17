# Contributing

## Prerequisites

- [asdf](https://asdf-vm.com) with the rust plugin: `asdf plugin add rust && asdf install`
- [just](https://github.com/casey/just): `brew install just`
- `~/.cargo/bin` on your `PATH`: add `export PATH="$HOME/.cargo/bin:$PATH"` to your `~/.zshrc`

## Development

```sh
just build    # build all tools
just test     # run all tests
just lint     # clippy
just fmt      # format
just install  # install binaries to ~/.cargo/bin
```

## Adding a New Tool

1. `cargo init crates/<name>` (e.g. `crates/foo` → binary `tkfoo`)
2. Add `"crates/<name>"` to `members` in the root `Cargo.toml`
3. Add `common = { path = "../common" }` to the new crate's dependencies
4. Add a `[name]` section to `~/.config/toolkit/config.toml` if the tool needs config
5. Use `common::load_section::<MyConfig>("name")` to load it
6. See `AGENTS.md` for output conventions and token efficiency guidelines
