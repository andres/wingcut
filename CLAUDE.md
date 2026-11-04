# wingcut: agent notes

- Read `SPEC.md` first. It is the source of truth for scope, stages and engineering rules.
- Stages 1–3 are done (CLI core, multi-panel, GUI in `gui/`). Stop and report before starting stage 4 (machine control).
- Clean-room: never decompile or copy closed tools (WiHoWi, devWing, etc.).
- Before each push: `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`.
- Ask the owner before adding dependencies beyond those listed in SPEC §8.
- Project skills live in `.claude/skills/` (from jeffallan/claude-skills). Use `rust-engineer` and `test-master` when writing code, and `code-reviewer` (spec compliance first, then quality) before each PR.
