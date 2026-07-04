# Extending the Protocol

When adding a new feature (new action type, config field, button mode, etc.), follow this checklist to keep all repos in sync.

## 1. Protocol (`pedalboard-protocol`)

This is always the starting point. All business logic lives here.

- [ ] Add/modify types in `src/config.rs` (serialized structs)
- [ ] Implement logic in `src/engine.rs` (pure, testable, no hardware)
- [ ] Add unit tests covering the new behavior (edge cases, boundaries, state transitions)
- [ ] If serialized `Preset` layout changed, bump `PRESET_SCHEMA_VERSION` in `config.rs`
  - Reordering, removing, or changing field types = **must bump**
  - Appending a new `#[serde(default)]` field at the end = **no bump needed** (postcard tolerates trailing data)
  - See `dotgithub/docs/adr-versioning.md` for the full versioning strategy
- [ ] Run `cargo test` — all tests must pass
- [ ] Push protocol first (firmware/CLI pre-commit hooks depend on remote protocol)

## 2. CLI (`pedalboard-cli`)

The CLI is the user-facing configuration layer.

- [ ] Add YAML types in `src/config.rs` (with `#[derive(Deserialize, JsonSchema)]`)
- [ ] Add doc comments describing **behavior**, not just data types — these become the JSON schema descriptions
- [ ] Add conversion logic in `yaml_to_presets()` or `convert_actions()`
- [ ] Run `cargo test` — the `schema_matches_committed_file` test will auto-regenerate `schema/pedalboard.schema.json` if structs changed
- [ ] Update `docs/config-reference.md` (hand-written) with the new feature: what it does, YAML syntax, behavioral notes
- [ ] Add/update an example in `examples/` that exercises the new feature (parsed by `all_examples_parse_successfully` test)
- [ ] Push CLI

## 3. Firmware (`pedalboard-midi`)

The firmware wires protocol logic to hardware.

- [ ] Wire new engine results in `pe_handler.rs` or `main.rs` (thin adapter only — no business logic here)
- [ ] If protocol changed serialization: `cargo update` to pick up new protocol commit
- [ ] Run host tests: `cd tests-host && cargo test`
- [ ] Flash device: `make flash`
- [ ] Run integration tests: `cd ../pedalboard-cli && ./tests/integration.sh`
- [ ] Push firmware

## Documentation Locations

| What | Where | Updated by |
|------|-------|------------|
| Behavioral semantics (doc comments) | `pedalboard-cli/src/config.rs` | When adding YAML fields |
| JSON Schema (machine-readable) | `pedalboard-cli/schema/pedalboard.schema.json` | Auto-generated from structs (validated by test) |
| User-facing reference | `pedalboard-cli/docs/config-reference.md` | Hand-written — update when adding features |
| Protocol engine tests | `pedalboard-protocol/src/engine.rs` | When adding/changing engine logic |
| Firmware architecture | `pedalboard-midi/docs/architecture.md` | When adding tasks or changing data flow |
| System architecture | `dotgithub/docs/software-architecture.md` | When cross-module flow changes |

## Key Principles

- **Protocol-first**: business logic in `pedalboard-protocol`, not firmware. This makes it testable without hardware.
- **Schema is the source of truth**: doc comments on CLI structs → JSON schema → user docs. Don't document in only one place.
- **Test the behavior, not the shape**: engine tests should verify what happens (MIDI output, state transitions, system actions), not just that structs serialize.
- **Integration tests catch deserialization drift**: if protocol serialization changes but firmware isn't reflashed, `integration.sh` will fail at content verification. Always flash before running integration tests after protocol changes.
- **Push order matters**: protocol → CLI → firmware. Pre-commit hooks validate against the remote protocol.
