# Security policy

## Reporting a vulnerability

Report security issues privately through GitHub's private vulnerability
reporting at https://github.com/larsakerlund/llmock/security/advisories/new (or
the repository's Security tab, then "Report a vulnerability"). This keeps the
report confidential until a fix is released. Please do not open a public issue
for a security problem.

Include enough detail to reproduce: the version or commit, the configuration
(flags and environment), and the steps that trigger the issue.

## Supported versions

llmock is pre-1.0. Only the latest release receives security fixes. Upgrade to
the latest release before reporting, and expect fixes to land on the next
release rather than as backports.

## Scope notes

Record mode forwards your real provider API key to the configured upstream and
is itself unauthenticated. Anyone who can reach the server can spend your key
by sending requests that miss the cassettes. Bind record mode to loopback
(`127.0.0.1`), or pass `--record-allow-remote` only when you have placed your
own access control in front of it. Replay mode needs no key and no network.

Recorded cassettes store only the matched model, a substring of the last user
message, and the upstream response body. They never store request auth headers
or request bodies. Review a cassette before committing it, since the response
body is saved verbatim and may carry content you would rather not publish.
