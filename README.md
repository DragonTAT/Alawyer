# Alawyer

Alawyer is a local-first macOS legal-assistant prototype for labor arbitration workflows.

It combines:
- A Rust core (`alawyer-core`) for session storage, agent runtime, retrieval, and safety checks.
- A SwiftUI app (`Alawyer`) for onboarding, chat workflow, settings, and report export.

## Current Scope (v0.1.x)

- Single scenario: labor arbitration.
- Guided intake Q&A flow.
- Report generation with citations and disclaimer.
- Safety interception for high-risk legal phrasing.
- Markdown export, copy-full-report, regenerate report.

## Project Structure

- `alawyer-core/`: Rust core library + tests
- `Alawyer/`: Swift Package app target + tests
- `scripts/generate_swift_bindings.sh`: UniFFI binding generation helper

## Build and Test

### Rust core

```bash
cd alawyer-core
cargo test --lib
```

### Swift app

```bash
cd Alawyer
swift build
swift test
```

## OpenRouter Configuration

The app expects an OpenRouter API key configured in Settings.
By default, it can use `openrouter/free` and other free-tier model IDs.

## Safety and Disclaimer

This project does **not** provide legal advice.
All generated content is for reference only and should be validated by a licensed lawyer.

## License

MIT. See `LICENSE`.
