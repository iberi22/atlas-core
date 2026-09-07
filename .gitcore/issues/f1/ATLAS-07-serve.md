# ATLAS-07: Local dev dashboard (serve)

Goal (L6): dev-only single-file HTML over websocket.
Stories: C-007. Branch: `mod/serve`.

AC:
- [ ] `atlas serve` renders task panel + history from local DB.
- [ ] Live updates without reload; zero external assets.
- [ ] Refuses public bind without explicit flag.

DoD: smoke test (serve, fetch page, websocket frame) green.
