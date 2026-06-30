# Security Policy

`note` is a local, single-user CLI/TUI: no network surface, no server, no auth —
your notes live in a SQLite file on your own machine. The realistic attack
surface is the handling of untrusted input (imported `.md` / `.json` files, note
bodies, and `[[wikilinks]]`).

## Supported versions

The project is pre-1.0; only the latest `master` / most recent release gets fixes.

## Reporting a vulnerability

Please report security issues **privately**, not as a public issue:

- open a [private security advisory](https://github.com/LLawli/note/security/advisories/new), or
- email **contato@lukakuuhaku.dev**.

Include steps to reproduce, the impact, and your `note --version`. You'll get an
acknowledgement within a few days; please allow a reasonable window for a fix
before public disclosure.
