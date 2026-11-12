# ppt2images

A command-line tool that turns presentation slides into numbered PNG images.
It offers three conversion engines:

| Backend | Engine | Input | Platform |
| --- | --- | --- | --- |
| `native` | Pure Rust renderer (`betteroffice-pptx` + `tiny-skia`) | PPTX, PPTM, PPSX, PPSM, POTX, POTM | Any Rust target |
| `libreoffice` | LibreOffice converts to PDF; Rust (`hayro`) rasterizes it | Everything LibreOffice can open | Any platform with LibreOffice |
| `powerpoint` | Microsoft PowerPoint COM automation | Everything PowerPoint can open | Windows with PowerPoint installed |

## Build

```sh
cargo build --release
```

The binary is `target/release/ppt2images`.

## Quick start

```sh
# Pure-Rust conversion (default for PPTX-family inputs)
ppt2images slides.pptx

# Choose an output directory and exact image size
ppt2images slides.pptx -o frames -r 3840x2160

# Use LibreOffice for maximum layout fidelity
ppt2images slides.pptx --backend libreoffice -o frames

# Custom page names from 0007 onward: slide-007.png, slide-008.png, ...
ppt2images slides.pptx --page-format 'slide-{page:03}' --start-index 7

# Replace files from an earlier run
ppt2images slides.pptx -o frames --overwrite
```

By default the output directory is `./output`, the image size is
`1920x1080`, and files are named `0000.png`, `0001.png`, ... (four digits,
starting at zero).

## Options

```text
Usage: ppt2images [OPTIONS] <INPUT>

  <INPUT>                                  Input presentation
  -o, --output-dir <DIR>                   Output directory [default: output]
  -r, --resolution <WIDTHxHEIGHT>          Exact output size [default: 1920x1080]
      --page-format <FORMAT>               Page-number pattern [default: {page:04}]
      --start-index <N>                    First page number [default: 0]
  -b, --backend <auto|native|libreoffice|powerpoint>
                                           Conversion backend [default: auto]
      --overwrite                          Replace existing PNG files
  -h, --help                               Show help
  -V, --version                            Show version
```

### Resolution

`--resolution` sets the exact pixel dimensions of every output image. Slides
are scaled to those dimensions even when the target aspect ratio differs from
the slide aspect ratio.

Limits: at most 16,384 pixels per side and 67,108,864 pixels per image.

### Page numbering

`--page-format` contains one page-number placeholder. Supported forms are:

| Format | Result for page 7 |
| --- | --- |
| `{page:04}` (default) | `0007` |
| `{:04}` | `0007` |
| `%04d` | `0007` |
| `0000` (shorthand) | `0007` |
| `4` or `04` (digit count) | `0007` |
| `slide-{page:03}` | `slide-007` |
| `slide-%03d-final` | `slide-007-final` |
| `{page}` / `%d` | `7` |

Literal braces and percent signs can be escaped with `{{`, `}}`, and `%%`.
The PNG extension is always added, so a format should not include `.png`.
Formats must not contain path separators.

`--start-index` sets the number assigned to the first slide. The default is
`0`, so the first file is `0000.png`.

### Existing files

Output is written to a hidden staging directory inside the destination and is
published only after every slide has rendered successfully. If a destination
file already exists, conversion fails unless `--overwrite` is passed.

## Backend selection

`--backend auto` (the default) is deterministic:

- PPTX-family inputs use the native backend.
- Legacy `.ppt`/`.pps`/`.pot` and `.odp` inputs use LibreOffice when it is
  installed.
- On Windows, legacy inputs fall back to PowerPoint when LibreOffice is not
  installed.

Choose a backend explicitly when you need a particular fidelity/runtime
tradeoff. Backend aliases are `rust` for `native`, `lo` and `libre-office` for
`libreoffice`, and `ppt`, `microsoft-powerpoint`, and `ms-powerpoint` for
`powerpoint`. Values are case-insensitive.

### Native

The native backend parses PPTX PresentationML and rasterizes it in Rust. It
does not launch an office suite and does not require a display server. It
supports common slides with text, shapes, pictures, tables, charts, fills,
lines, shadows, and grouped content.

Native text uses one Unicode-capable system font family as the fallback for
fonts named in the presentation. It prefers Noto Sans CJK, Source Han Sans,
Droid Sans Fallback, Microsoft YaHei, PingFang SC, Segoe UI, Arial, Liberation
Sans, and DejaVu Sans. Set `PPT2IMAGES_NATIVE_FONT` to a `.ttf`, `.otf`, or TrueType collection to
override this choice.

Native output is a static slide image. Unsupported media or images that cannot
be decoded are skipped with a warning instead of failing the whole deck. To
bound memory use, native rendering is capped at 16 megapixels internally and
resampled when a larger image is requested. For pixel-perfect fidelity or
unusual content, use the LibreOffice or PowerPoint backend.

### LibreOffice

The LibreOffice backend runs an isolated headless `soffice` profile, converts
the presentation to PDF, and rasterizes the PDF with the pure-Rust `hayro`
renderer. It does not require Poppler, ImageMagick, or a display server.

Requirements:

- LibreOffice installed, with `soffice` or `libreoffice` on `PATH`, or
- `PPT2IMAGES_SOFFICE` set to the LibreOffice executable.

If LibreOffice is already running with the same profile, the isolated profile
used here avoids most lock conflicts. If conversion still fails, close
LibreOffice and retry.

### Microsoft PowerPoint

The PowerPoint backend drives PowerPoint through COM automation from
PowerShell. It is available only on Windows and requires Microsoft PowerPoint
to be installed. It exports each slide at the requested pixel size.

Requirements:

- Windows.
- Microsoft PowerPoint installed.
- Windows PowerShell or PowerShell 7.

Set `PPT2IMAGES_POWERSHELL` to a specific PowerShell executable when it is not
on `PATH`. Automation uses a temporary export directory; the final files are
still named by `--page-format`.

## Environment variables

| Variable | Purpose |
| --- | --- |
| `PPT2IMAGES_SOFFICE` | Path to the LibreOffice executable |
| `PPT2IMAGES_POWERSHELL` | Path to PowerShell for the PowerPoint backend |
| `PPT2IMAGES_NATIVE_FONT` | Font file used by the native backend |

## Supported inputs

- OOXML: `.pptx`, `.pptm`, `.ppsx`, `.ppsm`, `.potx`, `.potm`
- Legacy PowerPoint: `.ppt`, `.pps`, `.pot`
- OpenDocument: `.odp`

The native backend accepts only OOXML packages. Use `--backend libreoffice`
for legacy and OpenDocument inputs.

## Development

```sh
cargo fmt --all --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets
```

The codebase does not suppress warnings with `#[allow(...)]`. Comments and
commit messages are written in English.
