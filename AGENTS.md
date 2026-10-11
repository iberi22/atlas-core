# Agent Guide

## Repo Identity
This repository, `atlas-core`, is a Rust task-state store for Atlas.
It provides the core state management for Atlas tasks and interactions.

## Build and Test Commands
To build and test the codebase, use the following commands:
- `cargo build`: Compiles the project.
- `cargo test`: Runs the test suite to ensure correctness.
- `cargo clippy`: Lints the code. Make sure to fix any warnings. You should run it as `cargo clippy -- -D warnings`.

## Contribution Rules
1. All changes must be tested.
2. Ensure you have no regressions before opening a PR.
3. Code changes should strictly follow the boundaries laid out in the tasks.
4. Do not touch `Cargo.toml`, `src/`, or `.gitcore/features.json` unless explicitly requested.
5. All new PRs must run successfully against CI before merge.
6. Make sure all issues are updated upon resolution.
7. Any dependencies changes must go through a security review.
8. Update the PR descriptions with a clear context of what is changed.
9. Limit the use of `unsafe` code in Rust to absolute minimum and justify it.
10. All governance changes should be reviewed by repo owners.

## Language Policy
English-only policy: All documentation, code comments, commit messages, and PR descriptions must be written in English. No exceptions.

## Best Practices
- Keep PRs small and focused.
- Ensure that the scope of the change does not exceed what is requested.
- If dependencies need changes, they should be well justified and isolated.
- Respect the existing code style.
- Use meaningful variable names and document complex logic.

## CI Guidelines
- Our CI workflows are defined in `.github/workflows/`.
- `ci.yml` is the primary gatekeeper for merges to `main`.
- We use Dependabot for keeping our dependencies up to date.

## Security
- Do not check in sensitive information.
- Review changes for potential vulnerabilities.
- Follow best practices for web and system security.

## Communication
- For any questions, clarify with the task owner or reviewer.
- When blocked, clearly state the blocker in the task or PR.
- Document any specific environmental constraints required to test the changes.

## Release Process
- Releases are cut from the `main` branch.
- Ensure all CI checks are green before tagging a release.
- Update documentation and changelogs when necessary.

## End of Guide
Please adhere to these guidelines strictly when contributing to this repository.
