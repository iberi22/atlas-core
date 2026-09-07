# ATLAS-03: Rule verifier + promotion gate

Goal: `feat-verifier` layer 1 — rule checks wired to promotion;
LLM reviewer is a stubbed trait.

Stories: C-013.

AC:
- [ ] No COMPLETED without rule pass (tests green, artifacts, DoD list).
- [ ] Promotion writes decision + evidence on the task node.
- [ ] Agent self-declaration path rejected by test.

DoD: a task that skips the verifier cannot reach COMPLETED in tests.
