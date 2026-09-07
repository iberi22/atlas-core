# ATLAS-06: Estimator — actuals + moving averages

Goal (L5): `atlas estimate` with tracked drift.
Stories: C-006. Branch: `mod/estimate`.

AC:
- [ ] Actuals recorded per task (duration, tokens, outcome).
- [ ] Whole-tree rollup respects dependencies (critical path).
- [ ] Drift report (estimate vs actual) per task and per run.

DoD: estimator unit tests with fixture histories green.
