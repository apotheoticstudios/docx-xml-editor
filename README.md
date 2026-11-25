<p align="center"><img src="assets/logo.png" width="112" alt="DOCX XML Editor logo"></p>

# DOCX XML Editor

A local desktop app for inspecting and editing the properties and XML inside Word documents. Written in Rust, with a shared native interface for macOS, Windows and Linux.

Open a `.docx`, change its labelled properties or individual XML parts, then save a validated, rebuilt copy. Documents are processed locally.

[![Build and test](https://github.com/apotheoticstudios/docx-xml-editor/actions/workflows/build.yml/badge.svg)](https://github.com/apotheoticstudios/docx-xml-editor/actions/workflows/build.yml)

## Features

- Properties grouped into document details, people and editing statistics.
- Editable title, subject, author, keywords, description, last editor, revision counter and editing time.
- Editing time displayed in minutes and converted to hours for reference.
- Drag-and-drop opening and native file dialogs.
- XML part filtering, line wrapping, and search with previous/next match selection.
- Clear unsaved-change, validation and save status.
- Keyboard shortcuts and accessibility support through egui’s AccessKit integration.
- An unsaved-changes prompt with save, discard and cancel actions.
- Validation before saving, automatic backups when replacing files, and verification of rebuilt part contents.

## Getting started

1. Open the app and choose **Open…**, or drop a `.docx` into the window.
2. Use **Properties** to edit document metadata.
3. Use **XML editor** for advanced source edits. Choose a part from the list and use the filter or search field to find what you need.
4. Choose **Check document** to validate your current edits.
5. Choose **Save a copy…**. The app validates again automatically and proposes a separate filename.
6. Open the saved copy in Word to review its content and layout.

Close a file in Word before replacing that same file through the editor. If you select an existing destination, the editor creates a uniquely named `.backup-<timestamp>.docx` beside it before replacement.

| Action | macOS | Windows / Linux |
| --- | --- | --- |
| Open a document | `⌘O` | `Ctrl+O` |
| Save a copy | `⌘S` | `Ctrl+S` |
| Find in XML | `⌘F` | `Ctrl+F` |

Search applies to the selected XML part. The previous and next buttons select matching text in the editor. The package filter searches part filenames.

## Builds and platform support

The same Rust source targets modern desktop macOS, Windows and Linux. Each operating system needs its own compiled executable.

The [build workflow](.github/workflows/build.yml) builds the release executable and runs the tests on macOS, Windows and Ubuntu. Download an artifact from a successful workflow run to try that platform’s build. Native runtime and visual checks have been performed on macOS Apple Silicon. Automated build/test results do not cover every desktop environment, graphics driver or operating-system version.

The locally packaged Mac `.app` is Apple Silicon and requires macOS 27 or later. It is locally signed rather than Apple-notarized. The Rust source can also be built on other supported targets.

### Build from source

Install the stable Rust toolchain from [rustup.rs](https://rustup.rs/), then run these commands from the repository root.

```sh
cargo build --release --locked
cargo test --locked
```

Run `target/release/docx-xml-editor` on macOS/Linux, or `target\release\docx-xml-editor.exe` on Windows.

**macOS** requires Xcode Command Line Tools.

```sh
xcode-select --install
```

**Windows** requires Visual Studio Build Tools with the C++ desktop workload and Windows SDK. Use the standard MSVC Rust toolchain.

**Debian / Ubuntu** needs a graphical desktop and these build dependencies.

```sh
sudo apt-get update
sudo apt-get install -y build-essential pkg-config libx11-dev libxi-dev \
  libgl1-mesa-dev libxrandr-dev libxcursor-dev libxinerama-dev \
  libxkbcommon-dev libwayland-dev
```

Linux file dialogs use the desktop portal. Install your desktop environment’s `xdg-desktop-portal` backend if file dialogs do not appear.

## Command line

Validate a DOCX without opening the interface.

```sh
docx-xml-editor --check input.docx
```

Rebuild and verify a copy without changing its XML values.

```sh
docx-xml-editor --roundtrip input.docx output.docx
```

## Validation and preservation

The editor checks the following before saving.

- ZIP checksums, duplicate entries and unsafe package paths.
- Required package-root parts.
- XML syntax and namespace references in `xsi:type` values.
- Internal relationship targets and duplicate relationship IDs.
- Nonnegative whole-number editing-time and revision values.

Property edits operate on the relevant XML ranges, preserving existing namespace declarations. Unedited package-part contents remain byte-for-byte identical. Rebuilt archives are reopened and compared part-by-part before the destination is written. ZIP container bytes and compression metadata can change during rebuilding.

This is not a complete OOXML schema or Word layout validator. A syntactically valid XML edit can still change document behaviour or produce content Word does not support.

## Limits

- XML parts must be UTF-8.
- Only `.docx` ZIP packages using stored or deflate compression are supported.
- Encrypted files, duplicate entries, symbolic links, unsafe paths and DTD-based XML are rejected.
- Archives are limited to 10,000 entries and 512 MB of expanded content.
- Edits to digitally signed packages are blocked because they would invalidate the signature.
- Optional metadata fields are disabled when their corresponding metadata part is absent.
- Word may update editing time and revision counters when it saves. These properties do not establish actual working time or a detailed editing history.

## Development

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --locked
```

`src/main.rs` contains the interface and edit-state handling. `src/package.rs` handles package loading, validation, XML-property edits, backups and verified saves.

Regression tests cover XML escaping, namespace preservation, invalid XML, invalid counter values, broken relationships, unsafe paths, archive rebuilding and backup contents.

The logo is included in `assets/`, along with its generation prompt. It was created using OpenAI’s image-generation tool. No network requests are required to load it.

## License

[MIT](LICENSE).
