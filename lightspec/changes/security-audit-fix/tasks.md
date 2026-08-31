# Tasks

- [x] 1. Validate URL scheme in `open_external` — allowlist `https://` only
- [x] 2. Redact workspace ID in OCG URL construction to prevent log leakage (already handled by `safe_url`)
- [x] 3. Improve `.gitignore` patterns for `.env.local`, subdirectory certs, nested config.json
- [x] 4. Remove `key.len()` from debug log in `set_api_key`
- [x] 5. Raise `redact_api_key` threshold from 8 to 12
- [x] 6. Bump patch version to 1.0.36
- [ ] 7. Build and verify deployment
