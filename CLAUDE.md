# wingcut: agent notes

- Read `SPEC.md` first. It is the source of truth for scope, stages and engineering rules.
- Stages 1–4 are built (CLI core, multi-panel, GUI in `gui/`, machine control in `machine/`). Stage 4 is only tested against the simulators until the hardware arrives: see `docs/hardware-bringup.md`. Never weaken the safety rules in `machine/src/controller.rs` (pause = hold + wire off; stop = hold then reset).
- Clean-room: never decompile or copy closed tools (WiHoWi, devWing, etc.).
- Before each push: `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`.
- Ask the owner before adding dependencies beyond those listed in SPEC §8.
- Project skills live in `.claude/skills/` (from jeffallan/claude-skills). Use `rust-engineer` and `test-master` when writing code, and `code-reviewer` (spec compliance first, then quality) before each PR.
