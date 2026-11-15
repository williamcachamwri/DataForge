# Security Policy

## Supported Versions

Security fixes are applied to the `main` branch and the latest release builds of TableMinus.

| Version | Supported |
| :--- | :--- |
| `0.1.x` (`main`) | Yes |

## Security Architecture & Guarantees

- **Local-First AI Privacy**: The Schema-Aware AI Assistant sends **only schema metadata** (table names, column names, types) when generating or explaining SQL. Table row data is never transmitted to LLM providers.
- **Credential Protection**: Database passwords and connection secrets are stored locally using OS-level protection rather than plaintext logs.
- **Safe Mode & Read-Only Guards**: Connections can be configured with Read-Only or Safe Mode (`ConfirmMutations` / `Strict`) to prevent accidental `UPDATE`, `DELETE`, or `DROP` execution.
- **Parameterized Grid Filters**: Filter predicates generated from the Data Grid UI are parameterized and dialect-aware rather than raw unescaped SQL concatenation.

## Reporting a Vulnerability

If you discover a security vulnerability in TableMinus:

1. **Do not** open a public issue with exploit details.
2. Report the issue privately via GitHub Security Advisories or contact the repository maintainers directly with:
   - Affected crate/file and reproduction steps
   - Impact assessment (e.g., credential exposure, SQL injection, local privilege escalation)
   - Suggested remediation if available
3. We aim to acknowledge reports within **48 hours** and provide a fix or mitigation plan as quickly as possible.
