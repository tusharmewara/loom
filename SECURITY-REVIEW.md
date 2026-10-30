# Security Review Findings

## Summary

| # | Severity | File | Lines | Vulnerability | Confidence |
|---|----------|------|-------|---------------|------------|
| 1 | 🟡 MEDIUM | crates/loom-core/src/plugin_registry.rs | 20-96, 399-402, 545-547 | Plugin-fetch SSRF guard can be bypassed | 8/10 |
| 2 | 🟡 MEDIUM | crates/loom-sso/src/oidc.rs | 27-32 | OIDC discovery is not pinned to validated DNS answers and redirects | 8/10 |

**Status:** Finding 1 is fixed in `crates/loom-core/src/plugin_registry.rs`. Finding 2 remains open.

## Details

The original plugin URL check validated a hostname separately from the request `ureq` made, leaving DNS rebinding and redirects able to reach private or loopback addresses. The plugin fetch path now pins validated DNS answers and checks each redirect.

OIDC discovery calls `validate_outbound_url`, then uses `openidconnect::reqwest::async_http_client` for the actual metadata request. That separate client is not pinned to the checked addresses and can follow redirects, leaving a DNS-rebinding/redirect path to private or loopback addresses.

## Remediation and remaining work

- Plugin downloads and updates parse URLs with `ureq`, reject private/loopback DNS answers, pin validated answers for each request, and revalidate each HTTPS redirect with a five-redirect limit.
- Tests cover noncanonical and query-only private IP URLs, IPv6 local addresses, and mixed public/private DNS answers.
- OIDC discovery still needs a client that pins each validated destination and validates each redirect (or disables redirects and validates them explicitly).

## Proposed security rule

Every outbound URL fetch must use the same validated and pinned destination for the network connection, and must revalidate each redirect before following it. A preflight URL/DNS check alone is not sufficient. Apply this rule to every caller, including OIDC discovery.
