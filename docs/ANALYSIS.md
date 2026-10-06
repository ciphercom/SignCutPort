# How SignCut Pro 2 works, and what this port changes

This was analysed from the official SignCut Pro 2 v0.1.490 installers (macOS `.dmg` and Windows `.exe`, August 2025), using:
- the user manual
- the bundled resources
- the driver definitions
- static inspection of the macOS binary

Nothing was executed, and no SignCut code is reused.

## Application structure

- **App:** C++ / wxWidgets 3.2 (Cocoa), with the Sparkle updater.
- **Statically linked libraries:**
  - `libusb`
  - wjwwood `serial`
  - asio
  - FreeType and HarfBuzz
  - PoDoFo (PDF)
  - clipper (offsets)
  - libcurl
- **Bundled helper tools:**
  - `potrace` (bitmap tracing)
  - `kabeja-dxf2svg.jar` (DXF, needs Java)
  - `dwg2SVG` (DWG)
- **Resources:** `res.pak` (UI images, help, XRC dialogs) and `drivers.pak` (cutter definitions). Both are zip archives.
- **Plug-in drivers:** three `.so` drivers (Roland, Summa, Vulcan) handle machine-specific protocols.
- **Native document format:** `.scpro2`. This is XML: layers → colours → paths → nodes with Bézier handles, plus tiles, weed lines and registration marks.

## Cutter definitions (`drivers.pak`)

There is one XML file per manufacturer (95 files, 1,616 models).

**Structure:**
- A `<Config>` and a `<Commands>` block hold manufacturer-wide defaults.
- Each `<Plotter>` can override any value from those blocks.
- The value `CLEAR` removes an inherited value.

**Command table:**

| Purpose | Fields |
|---|---|
| Job start and end | `Initialise`, `StartCmd`, `EndCmd`, `AfterCutCmd` |
| Pen moves | `Tool_Up`, `Tool_Down` |
| Media | `PageFeed` |
| Speed, force, tool | `Velocity`, `Force`, `SelectPen` |
| Syntax | `Delimiter`, `Terminator` |

**Placeholders:**
- `0x20` = space
- `%newline%` = CR LF
- `%space%` = space
- `\n` = line feed

**Statement format.** Every statement is `command + value(s) + terminator`, and every point is a separate statement:
- HPGL: `PU100,200;PD300,400;`
- VEVOR D-board: `U100,200 D300,400 `

**Speed, force and tool.** These are sent as `VS<n>;`, `FS<n>;` and `SP<n>;` (number + terminator).

**Resolution.** `XResolution` and `YResolution` are millimetres per device unit. 0.025 means 40 units per mm.

**Axis order.** It depends on two values:
- `Rotate90`: 0 when the carriage home is on the left, 1 when it is on the right (the default).
- `SwapAxis`.

**VEVOR**

| Boards | Models | Protocol |
|---|---|---|
| D-boards | KH-/KI-/SK-/…D | `;:H A L0 ECN U`, then `U x,y` / `D x,y` with space terminator; 9600 baud, RTS/CTS |
| ARM boards | …A/…S/…TS | `IN;PU…;PD…;`; 38400 baud; TS models add `SP`, `VS`, `FS` |

VEVOR models have no USB vendor ID in the definitions. They are driven through the USB-serial port (`/dev/cu.*`).

**Connection types in SignCut:** serial, libusb, file, LPT/printer, the SignCut Spooler, and TCP. TCP defaults to port 9100, and 8080 for some Skycut/Bannercut models. `ActAsPrinterDriver` only affects Windows dialogs; it has no transport meaning on macOS.

### Job stream (verified in the binary)

SignCut Port reproduces this sequence. The one intentional difference is that coordinates are rounded rather than truncated.

**Start of job**
1. `StartCmd` is sent when the port opens, then `Initialise`.
2. `SelectPen` is sent only when a job uses two or more tools, preceded by a bare `Tool_Up`.
3. `Velocity` and `Force` are sent only when "Use software force and speed" is on.

**Body**
- Every point is its own statement: a pen-up move to the start of each path, then a pen-down statement per point.
- There is no extra pen-up after the last point; the next pen-up move does that.

**End of job**
1. The head moves, depending on the setting:
   - "End after job" (the default) moves pen-up to (end of job + feed-forward, 0).
   - "Go back to beginning" moves pen-up to (0,0).
2. `PageFeed` is sent.
3. `AfterCutCmd` is sent, except after "Go back to beginning".
4. `EndCmd` is sent.
- Nothing else is appended. For VEVOR D-boards the `@` comes from their `PageFeed` (`U F @`).

**Coordinates**
- The first number is X, the feed/length axis.
- The order is swapped only when `Rotate90 = 0` or `SwapAxis = 1`.
- Values are not clamped.
- In "optimized" mode the origin is shifted by the blade offset.

**Example: VEVOR KH-720 with default settings**

`;:H A L0 ECN U ` → `U x,y D x,y D … ` → `U<job end>,0 ` → `U F @ `

## Why custom fonts break in SignCut on macOS

SignCut parses SVG itself and renders `<text>` with its own FreeType font index. That index has several gaps.

**It only scans four folders, and doesn't look inside subfolders:**
- `/System/Library/Fonts`
- `/System/Library/Fonts/Supplemental`
- `/Library/Fonts`
- `~/Library/Fonts`

Fonts are therefore missed when they are:
- in subfolders of those directories
- activated by Adobe Fonts / Creative Cloud
- installed by font managers
- in `/System/Library/AssetsV2` (downloadable system fonts)

**It indexes only the first face of each `.ttc` / `.otc` collection**, and only the default instance of variable fonts.

**It rejects Type 1 and other non-sfnt fonts.**

**Its name matching is crude.**
- It strips quotes, splits on spaces and hyphens, and matches against `"family style"`.
- PostScript names such as `OpenSans-Bold` (the way Illustrator writes them), comma-separated fallback lists, and numeric weights are not handled reliably.

**When matching fails, it substitutes Arial or Times.**

## What SignCut Port does instead

### Font handling

- **Font discovery** uses `fontdb` over all standard folders, recursively. On top of that it adds:
  - every font file CoreText knows about (`CTFontCollection`, which covers font managers and Adobe Fonts)
  - the Adobe CoreSync folder
  - `.dfont` / suitcase resources
- **Name matching** works on family, PostScript and "family + style" names. It ignores case and punctuation, honours CSS fallback lists, and uses CSS-style weight matching.
- **Text layout** is done by `usvg` with `rustybuzz` shaping, so kerning, ligatures, `letter-spacing`, `textPath` and similar features work.
- **Misses are reported, never silently replaced.** The UI offers a substitute per family, or you can load the font file and re-import.

### Cut planning

- **Coordinates:** sheet coordinates map to cutter coordinates as `X = x` and `Y = material_width − y`. Shapes cut where they are drawn and are not mirrored.
- **Ordering:** inner shapes are cut before the shapes that contain them, then by nearest-neighbour or by bands along the length.
- **Blade-offset compensation:** the tool axis leads the tip by the offset along the direction of travel, and swivels on an arc of that radius around every corner. The blade direction is carried over from one path to the next (tangential emulation).
- **Overcut:** closed paths continue past their start by the overcut length.

### Output

- Encoding is table-driven from the same driver definitions, so the bytes match what SignCut sends. Coordinates are rounded rather than truncated.
- Transports:
  - serial through `serialport` (IOKit `/dev/cu.*`)
  - USB through `nusb` (no libusb needed)
  - TCP
  - CUPS raw (`lp -o raw`)
  - file
