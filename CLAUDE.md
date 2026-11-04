# wingcut: agent notes

- Read `SPEC.md` first. It is the source of truth for scope, stages and engineering rules.
- Build stages 1–2 only (CLI core + multi-panel). Stop and report before starting the GUI.
- Clean-room: never decompile or copy closed tools (WiHoWi, devWing, etc.).
- Before each push: `cargo fmt`, `cargo clippy -- -D warnings`, `cargo test`.
- Ask the owner before adding dependencies beyond those listed in SPEC §8.
- Project skills live in `.claude/skills/` (from jeffallan/claude-skills). Use `rust-engineer` and `test-master` when writing code, and `code-reviewer` (spec compliance first, then quality) before each PR.
