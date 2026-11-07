# Security

Security fixes target the current `main` branch. There is no maintenance
commitment for older snapshots yet.

## Reporting a vulnerability

Use GitHub's **Security → Report a vulnerability** on
[Hank-dev/lector](https://github.com/Hank-dev/lector/security/advisories/new)
for a private report. If private reporting is unavailable, open an issue
asking for a private contact channel without including exploit details or
the affected document.

Include the Lector commit, OS, GTK and Poppler versions, impact, reproduction
steps, and a minimal synthetic PDF when possible. Do not send passwords,
personal documents, or your session/annotation files. There is no guaranteed
response time.

## Security boundaries

Lector parses PDFs using the system Poppler library. It is a native application
with the current user's permissions, not a document sandbox. Keep Poppler,
GTK, and the operating system updated. The Rust dependency audit in CI does
not audit those system libraries.

Reading history contains document paths; sidecar annotations can contain
document text and notes. Both are written atomically with mode `0600` on
Unix, but are not encrypted. PDF passwords are held in memory for reopening
the current document and are not stored in reading history.

External links are opened through the default desktop application after
scheme checks. PDF JavaScript, XFA, and cryptographic signing are not supported.
