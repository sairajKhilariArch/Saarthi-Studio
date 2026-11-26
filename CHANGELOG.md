# Changelog

## 4.0.0

### Reliability
- Reject malformed JSON from successful router responses instead of converting it into a fake assistant answer.
- Reject successful JSON responses that do not contain supported chat message content.
- Return a clear gateway error for malformed one-shot streaming responses.
- Add regression tests for invalid JSON, missing content, plain text and structured text parts.

### Release
- Start the Sarthi Studio desktop 4.x major release line.
- Keep Android, VS Code and website versioning independent.
- Move historical 3.x release notes to [the archived changelog](docs/release-history/CHANGELOG-3.x.md).

## Previous releases

Historical 3.x release notes are preserved in [CHANGELOG-3.x.md](docs/release-history/CHANGELOG-3.x.md).
