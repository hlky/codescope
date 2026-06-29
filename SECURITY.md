# Security Policy

## Supported Versions

Security fixes target the latest `main` branch until tagged releases begin. After the first tagged release, supported versions will be documented here.

## Reporting a Vulnerability

Report vulnerabilities privately to the repository owner before opening a public issue. Include:

- affected command and backend,
- a minimal input fixture when possible,
- expected impact,
- platform and `codescope --version` output.

Do not include private source code or local project paths in public reports.

## Scope

Relevant issues include command execution risks, unsafe archive handling, malicious-source crashes that affect ordinary use, dependency vulnerabilities, and incorrect handling of release artifacts or checksums.
