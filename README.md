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

# Preserve the slide aspect ratio from one dimension
ppt2images slides.pptx -o frames --width 1920
ppt2images slides.pptx -o frames --height 1080

# Render at 150 DPI using the slide's physical size
ppt2images slides.pptx -o frames --dpi 150

# Export selected pages: 1-3, page 6, and page 9 through the end
ppt2images slides.pptx -o frames --pages '1-3,6,9-'

# Export every second page from 1 through 10
ppt2images slides.pptx -o frames --pages '1-10@2'

# Use LibreOffice for maximum layout fidelity
ppt2images slides.pptx --backend libreoffice -o frames

# Custom page names from 0007 onward: slide-007.png, slide-008.png, ...
ppt2images slides.pptx --page-format 'slide-{page:03}' --start-index 7

# Replace files from an earlier run
ppt2images slides.pptx -o frames --overwrite
```

By default the output directory is `./output`, the image size is
`1920x1080`, and files are named `0001.png`, `0002.png`, ... (four digits,
starting at one).

## Options

```text
Usage: ppt2images [OPTIONS] <INPUT>

  <INPUT>                                  Input presentation
  -o, --output-dir <DIR>                   Output directory [default: output]
  -r, --resolution <WIDTHxHEIGHT>          Exact output size
      --width <PX>                         Width; height follows slide aspect ratio
      --height <PX>                        Height; width follows slide aspect ratio
      --dpi <DPI>                          Output DPI from physical slide size
      --page-format <FORMAT>               Page-number pattern [default: {page:04}]
      --start-index <N>                    First page number [default: 1]
      --pages <RANGES>                     Pages to export; all pages by default
  -b, --backend <auto|native|libreoffice|powerpoint>
                                           Conversion backend [default: auto]
      --overwrite                          Replace existing PNG files
  -h, --help                               Show help
  -V, --version                            Show version
```

The four size options `--resolution`, `--width`, `--height`, and `--dpi` are
mutually exclusive. When none is supplied, `1920x1080` is used.

### Resolution

There are four ways to select the output size:

- `--resolution 3840x2160` sets exact pixel dimensions. Slides are scaled to
  those dimensions even when the target aspect ratio differs from the slide
  aspect ratio.
- `--width 1920` sets the width and derives the height from the slide aspect
  ratio.
- `--height 1080` sets the height and derives the width from the slide aspect
  ratio.
- `--dpi 150` multiplies the slide's physical dimensions by the requested DPI.
  For example, a 13.333 by 7.5 inch widescreen slide at 150 DPI produces
  `2000x1125` pixels.

Derived dimensions are rounded to the nearest pixel and are at least one
pixel. Limits: at most 16,384 pixels per side, 67,108,864 pixels per image,
and 2,400 DPI.

### Progress

An interactive stderr shows a live bar with elapsed time, completion count,
percentage, and the image currently being staged:

```text
Converting 'slides.pptx' with native (size: 1920px wide, auto height)
00:00:03 [████████████████░░░░] 20/25 (80%) 0020.png
Wrote 25 images at 1920x1080 to 'frames' in 2.34s
```

The bar is drawn with `indicatif`, the progress-bar library beneath
`tracing-indicatif`. It updates in place rather than appending one line per
slide. When stderr is redirected to a file or is not a terminal, the bar is
hidden so logs stay clean.

The PowerPoint backend streams its export count from the COM automation
script, so the bar also advances while PowerPoint is still exporting. Warnings
temporarily suspend and redraw the bar. The final `Wrote` summary, including
total elapsed time, is printed on stdout.

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
`1`, so the first file is `0001.png`.

### Page selection

Use `--pages` to export only selected slides. Numbers use the same indexing as
`--start-index`, so with `--start-index 10` the first available page is 10.

```text
--pages '1-3,6,9-'
```

| Syntax | Meaning |
| --- | --- |
| `5` | Page 5 only |
| `a-b` | Pages a through b, both inclusive |
| `a-` | Page a through the last page |
| `-b` | The first available page through b. With the default `start-index` this is `1-b` |
| `a-b@t` | Pages a, a+t, a+2t, ... while the page is at most b |
| `a-@t` | Page a, a+t, ... through the last page |
| `-b@t` | The first available page, then every t-th page through b |

Comma-separated ranges may overlap. Overlapping pages are merged and exported
once, in ascending page order. The progress total and final image count reflect
only the selected pages.

Examples:

```bash
ppt2images slides.pptx --pages '2,4,6'
ppt2images slides.pptx --pages '5-'
ppt2images slides.pptx --pages '-4'
ppt2images slides.pptx --pages '1-20@3'
ppt2images slides.pptx --start-index 10 --pages '10-14'
```

Selection numbers outside the valid `start-index` through last-page range are
rejected.

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
Sans, and DejaVu Sans. Set `PPT2IMAGES_NATIVE_FONT` to a `.ttf`, `.otf`, or
TrueType collection to override this choice.

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

The `just` command runner is required for these recipes.

```sh
just lint
just test
```

`just lint` runs `cargo fmt --all --check` followed by
`cargo clippy --all-targets --all-features -- -D warnings`.
`just test` runs `cargo test --all-targets`.

Production code denies `clippy::unwrap_used`; unit tests are exempt so their
setup code can stay concise. The codebase does not suppress warnings with
`#[allow(...)]`. Comments and commit messages are written in English.
