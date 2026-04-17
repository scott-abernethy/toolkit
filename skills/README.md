# Agent Skills & Agents for Toolkit

This repo contains two types of AI agent configuration:

- **Skills** (`skills/`) — for [opencode](https://opencode.ai) and similar tools
- **Agents** (`agents/`) — for [GitHub Copilot CLI](https://docs.github.com/copilot/concepts/agents/about-copilot-cli)

## Setup

After cloning the toolkit repository:

```bash
# 1. Install the CLI tools (required for tool skills only)
cd ~/path/to/toolkit
just install

# 2. Link all skills to opencode
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

## Tool Skills

These skills wrap the CLI tools in this repo. They require `just install` to be run first.

- **[tkpsql](tkpsql/SKILL.md)** - PostgreSQL queries with safe defaults
- **[tkdbr](tkdbr/SKILL.md)** - Databricks metadata exploration and bundle management

## Workflow Skills

These skills encode team conventions and processes. No installation beyond symlinking is required.

*(Coming soon — see the [Agents](../README.md#agents) section in the root README for the git-flow workflow agent)*

## How it works

Opencode loads skill definitions from `~/.config/opencode/skills/`. Each skill has a `SKILL.md` file that specifies:

1. **What the tool does** — brief description
2. **When to use it** — trigger phrases and use cases
3. **How to install it** — setup instructions
4. **Usage examples** — common commands and patterns
5. **Configuration** — setup files and environment variables

When you ask the agent a question, it searches your installed skills and automatically suggests relevant tools.

## Adding new skills

**Tool skill** (wraps a CLI tool):
1. Create a directory: `skills/<tool_name>/`
2. Add a `SKILL.md` file (follow the pattern in `tkpsql/SKILL.md`)
3. Link it: `ln -s $(pwd)/skills/<tool_name> ~/.config/opencode/skills/<tool_name>`

**Workflow skill** (team convention):
1. Create a directory: `skills/<skill_name>/`
2. Add a `SKILL.md` describing the convention/process the agent should follow
3. Link it: `ln -s $(pwd)/skills/<skill_name> ~/.config/opencode/skills/<skill_name>`

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
