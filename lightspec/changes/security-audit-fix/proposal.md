# Proposal: Security Audit Fixes

## Why
Security audit identified issues that need remediation before the next release.

## What
Fix items 1, 3, 5, 6, 9 from the security audit:
1. Validate URLs in `open_external` to prevent arbitrary protocol handler abuse
3. Sanitize workspace ID in URL path to prevent log leakage
5. Improve `.gitignore` coverage for secrets
6. Remove full key length from debug logs
9. Raise `redact_api_key` threshold to avoid showing 8/9 chars of short keys
