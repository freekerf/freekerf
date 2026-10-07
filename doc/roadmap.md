# Roadmap

What was postponed, and open decisions. Milestones are in [PLAN.md](../PLAN.md) §10.

## Postponed from M0/M1

| Item | Why | When |
| --- | --- | --- |
| LaserGRBL (C#) numbers in `doc/benchmarks.md` | needs a Mono build of LaserGRBL and a timing harness | next benchmark update |
| Telnet and WebSocket transports, network discovery | M5 scope in the plan | M5 |
| `freekerf-ipc`, `freekerf-script`, `freekerf-mcp`, Python client | M2 scope | M2 |
| Fuzzing (`cargo-fuzz` targets for G-code) | planned with the importers' parsers | M2 |
| Permission grants (this run / session / always) | needs callers other than the user (scripts, agents) | M2 |
| Human fire confirmation of `laser` actions | needs the desktop UI | M4 |
| Sounds, notifications, hotkeys, safety countdown | UI concerns | M4 |
| Importing LaserGRBL settings (`Settings.bin`), buttons, counters | `freekerf-migrate` | M2 |

## Decided

- UI toolkit: Tauri 2, desktop first (ADR 0006). Multi-touch problems found on Linux are listed here when they appear and handled later.

- No auto-update for now (PLAN §14).
- No MCP prompts for now (PLAN §14).

## Open


- MCP: should `allow_laser` exist at all (PLAN §15)?
