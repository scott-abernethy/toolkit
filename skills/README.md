# Agent Skills for Toolkit

This directory contains [opencode](https://opencode.ai) agent skill definitions for the toolkit CLI tools.

Each skill file tells the agent when and how to use the corresponding tool.

## Setup

After cloning the toolkit repository and installing tools:

```bash
# 1. Install the CLI tools
cd ~/path/to/toolkit
just install

# 2. Link the skills to opencode
ln -s ~/path/to/toolkit/skills/tkpsql ~/.config/opencode/skills/tkpsql
ln -s ~/path/to/toolkit/skills/tkdbr ~/.config/opencode/skills/tkdbr
```

Or to link all skills at once:

```bash
cd ~/path/to/toolkit
for skill in skills/*/; do
  skill_name=$(basename "$skill")
  ln -s "$(pwd)/$skill" ~/.config/opencode/skills/"$skill_name"
done
```

To update existing symlinks (if you're re-running setup), use `-sf`:

```bash
for skill in skills/*/; do
  skill_name=$(basename "$skill")
  ln -sf "$(pwd)/$skill" ~/.config/opencode/skills/"$skill_name"
done
```

## Skills

- **[tkpsql](tkpsql/SKILL.md)** - PostgreSQL queries with safe defaults
- **[tkdbr](tkdbr/SKILL.md)** - Databricks metadata exploration and bundle management

## How it works

Opencode loads skill definitions from `~/.config/opencode/skills/`. Each skill has a `SKILL.md` file that specifies:

1. **What the tool does** — brief description
2. **When to use it** — trigger phrases and use cases
3. **How to install it** — setup instructions
4. **Usage examples** — common commands and patterns
5. **Configuration** — setup files and environment variables

When you ask the agent a question, it searches your installed skills and automatically suggests relevant tools.

## Adding new skills

To create a new skill for a toolkit tool:

1. Create a directory: `skills/<tool_name>/`
2. Add a `SKILL.md` file with the frontmatter and documentation (follow the pattern in `tkpsql/SKILL.md`)
3. Link it to opencode: `ln -s $(pwd)/skills/<tool_name> ~/.config/opencode/skills/<tool_name>`

## Environment Setup

Most toolkit tools require configuration in `~/.config/toolkit/config.toml`. See individual skill files for details.

Example setup:

```bash
# Create the config directory if it doesn't exist
mkdir -p ~/.config/toolkit

# Edit the config (use your editor of choice)
$EDITOR ~/.config/toolkit/config.toml
```

Sample `~/.config/toolkit/config.toml`:

```toml
[psql.local]
host     = "localhost"
port     = 5432
database = "mydb"
user     = "readonly"
password = "secret"

[dbr.dev]
profile = "databricks-dev"
bundle_target = "dev"
```

## Troubleshooting

**Skill not showing up in opencode:**
- Ensure the symlink is in `~/.config/opencode/skills/`
- Check that `SKILL.md` exists and has valid frontmatter
- Reload opencode config: restart the editor or run the refresh command

**Tool not found when agent tries to use it:**
- Ensure `just install` was run: `tkpsql --help` should work in terminal
- Check `~/.cargo/bin` is on your `PATH`: `echo $PATH`
- Add to shell profile if needed: `export PATH="$HOME/.cargo/bin:$PATH"`

**Configuration issues:**
- Verify `~/.config/toolkit/config.toml` exists and has the right section names
- Use `TOOLKIT_CONFIG=/path/to/other.toml` to override config path if needed
- Check credentials/permissions are correct by testing the tool manually

## References

- [Opencode Skills Documentation](https://opencode.ai/docs/skills)
- [Toolkit Tools Documentation](../README.md)
- [Toolkit Development Guide](../AGENTS.md)
