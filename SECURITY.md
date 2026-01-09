# Security Policy

## Supported Versions

| Version | Supported          |
| ------- | ------------------ |
| 0.1.x   | :white_check_mark: |

## Reporting a Vulnerability

If you discover a security vulnerability in AAS-ΔSync, please report it responsibly:

1. **Do NOT** open a public GitHub issue for security vulnerabilities
2. Email security concerns to: [security@example.com]
3. Include:
   - Description of the vulnerability
   - Steps to reproduce
   - Potential impact
   - Suggested fix (if any)

We will acknowledge receipt within 48 hours and provide a detailed response within 7 days.

## Security Considerations

### Network Security

- MQTT communication should use TLS in production
- FA³ST adapter requires HTTPS (per AAS v3.0 specification)
- Consider network segmentation between agents

### Data Integrity

- Delta signatures (Ed25519) can be enabled for tamper detection
- CRDT merge is deterministic and idempotent
- Local persistence uses SQLite with WAL mode

### Authentication

- BaSyx/FA³ST authentication tokens should be provided via environment variables
- Never commit credentials to the repository
