# Proposal: OpenCode Go console API migration

## Why
OpenCode migrated its web Console from the legacy server-rendered
dashboard (`opencode.ai/workspace/<id>/go`) to a new SPA at
`opencode.ai/console` with a JSON API under `/console/api/*`
(changelog v1.18.26, 2026-09-01: "Updated the default Console URL used
by account connection flows"). Migrated workspaces no longer render the
legacy page — it redirects to the console login route and serves an empty
SPA shell, while unrecognized `/workspace/*` routes return the site's
404 page. SubBar's Go endpoint scraped that legacy HTML for the embedded
`rollingUsage`/`weeklyUsage`/`monthlyUsage` Seroval payload, so since the
migration the fetch fails (HTTP 404 from the dead route, or a payload-less
shell) and the tray falls back to `unauth`.

## What
Read Go usage from the OpenCode console surface. No legacy fallback is kept
(per requirements):

1. Prefer the Zen usage API when the stored token is a Go API key
   (`sk-…`): `GET https://opencode.ai/zen/go/v1/usage` with
   `Authorization: Bearer <key>` returns
   `{usage:{rolling|weekly|monthly:{status,percent,resetsAt}}}` — verified
   live 2026-10-11. No workspace id or session cookie needed; the key is
   workspace-bound. The key comes from the console API Keys page (or the
   `opencode-go` entry in `~/.local/share/opencode/auth.json`).
2. Otherwise treat the stored token as a console session cookie and read
   usage via `GET /console/api/go/status` with the `x-org-id` header,
   discovering the workspace via `GET /console/api/orgs` when the field is
   empty (the console session cookie is `__Host-console_session`; the
   legacy `auth` cookie is rejected by the console API with 401).
3. Accept a bare workspace id or a pasted console/legacy dashboard URL in
   the workspace field; send a pasted session cookie under every accepted
   session name (`auth`, `__Host-auth`, `__Host-console_session`).
4. Map failures to clear, actionable errors (401/403 naming the expected
   credential, 404 workspace not found, missing windows/subscription).
5. Drop the dead regex scrapers and the legacy URL template; keep
   keychain storage and the settings UI shape unchanged (the existing
   token field now also accepts the Go API key).
6. Update the README credential guide + in-app hints.
7. Bump patch version, build, deploy to /Applications, verify.
