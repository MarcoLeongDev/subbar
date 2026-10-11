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
Replace the legacy HTML scraping with the console JSON API. No legacy
fallback is kept (per requirements):

1. Discover the workspace id via `GET /console/api/orgs` when the user
   has not entered one; accept a bare id (`wrk_…`/`org_…`) or a pasted
   console/legacy dashboard URL in the workspace field.
2. Read Go usage via `GET /console/api/go/status` with the
   `x-org-id: <workspace-id>` header and the console session cookie.
   Parse `access.meters.fiveHour|week|month` (`usedMicroCents`,
   `limitMicroCents`, `resetsAt`); the month meter falls back to
   `access.endsAt` for its reset time.
3. Send the pasted credential under every session cookie name the console
   accepts (`auth`, `__Host-auth`, `__Host-console_session`) so a value
   copied from the browser works regardless of which cookie the user
   picked.
4. Map failures to clear, actionable errors (401/403 → re-copy the
   cookie; 404 → workspace not found; missing `access`/meters → no Go
   subscription).
5. Drop the now-dead regex scrapers and the legacy URL template; keep
   keychain storage and the settings UI shape unchanged.
6. Update the README credential guide (console URL, cookie name).
7. Bump patch version, build, deploy to /Applications, verify.
