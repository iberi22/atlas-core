# ATLAS-05: Foundation — sessions + CLI surface

Goal (L1): durable session identity + complete CLI.
Stories: C-001, C-004 (session half), C-011. Branch: `mod/foundation`.

AC:
- [ ] `atlas start/status/stop/list` round-trip on temp DB.
- [ ] `--json` output stable and script-parseable.
- [ ] Session resume after process kill keeps history intact.

DoD: CLI integration tests green; resume test included.
