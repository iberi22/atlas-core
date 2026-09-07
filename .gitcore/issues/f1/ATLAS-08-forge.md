# ATLAS-08: Minimal forge loop + main-only deploy

Goal (L7): agent PR throughput + main-only mirror.
Stories: C-008, C-009. Branch: `mod/forge`.

AC:
- [ ] Issue/PR create-comment-merge API usable by agents.
- [ ] Fast CI profile documented with time budget; heavy opt-in.
- [ ] Non-main deploy rejected; rollback path tested.

DoD: loop test issue->PR->fast-CI->main green on fixture repo.
