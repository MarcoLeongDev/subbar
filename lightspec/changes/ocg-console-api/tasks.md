# Tasks

- [x] 1. Create lightspec change folder + proposal (this file)
- [x] 2. Implement console API fetch (orgs discovery + go/status parsing) in main.rs
- [x] 3. Cookie/workspace normalization helpers + unit tests
- [x] 4. Remove legacy scrapers/regexes, update error messages
- [x] 5. Update README credential guide
- [x] 6. cargo test green (23 tests)
- [x] 7. Bump version 1.0.37 → 1.0.38 (Cargo.toml, tauri.conf.json, Info.plist, README, index.html)
- [x] 8. Commit e79c090
- [x] 9. Build release, deploy to /Applications, launch, verify version + new code path live (v1.0.38 live, console 401 path verified live)
- [x] 10. Name __Host-console_session in error messages (v1.0.39 committed+deployed, verified live: legacy redirects to /console/login, console API ignores auth cookie)
- [ ] 11. Verify live: only auth cookie available -> dig for a working invocation (Zen /zen/go/v1/usage + Bearer Go API key WORKS live 2026-10-11)
- [ ] 12. Implement Go-API-key flow with auto-dispatch (sk-… -> Zen API; else -> console cookie flow)
- [ ] 13. Unit tests for Zen parsing + dispatch; live-gated API-key test via OCG_TEST_APIKEY
- [ ] 14. README + in-app hints for the Go API key
- [ ] 15. cargo test green, bump 1.0.39 -> 1.0.40, commit, rebuild, redeploy, verify live panel
- [ ] 16. Screenshots to /tmp
- [ ] 17. Clean-subagent regression review of this round's diff
