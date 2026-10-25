# 0079: Render-to-PNG over the control channel

- Status: proposed
- Date: 2026-10-25
- Task: P9-01

## Context

KICKOFF.md Phase 9: render-to-PNG, "so an agent can drive the app and
verify its own work". The renderer (`crates/render`, ADR 0025) already
builds a scene from a page and encodes it as PNG or SVG at a scale in
pixels per inch; the CLI's `render` and the `file.render` command write
a page to a file. A model reads images inline (MCP image content); a
script would rather have a file. A careless or hostile caller can ask
for a page at a scale that allocates gigabytes.

## Decision

### The method

`render/page` (params `Render`, result `Rendered`):

| Param | Meaning | Default |
|---|---|---|
| `document` | handle (ADR 0076) | the active document |
| `page` | 1-based index or name | the active page |
| `region` | `{x, y, width, height}` in inches, page coordinates | the whole page |
| `scale` | pixels per inch | 96 (ADR 0025) |
| `format` | `png` or `svg` | `png` |
| `background` | `"none"` (transparent) or `"#RRGGBB"` | the page's own (ADR 0026) |
| `output` | a file path to write instead of returning bytes | inline |
| `overwrite` | allow `output` to replace a file | false |

The result gives the format, the pixel `width` and `height`, the byte
size, and either `data_base64` (inline, standard base64) or `path` (the
file written). MCP's `render_page` tool returns the inline PNG as image
content (`type: "image"`, `mimeType: "image/png"`) beside the
structured result (ADR 0080).

Rendering reads the document as it is now, the unsaved edits included,
through the same scene build as the canvas, so an agent sees what the
user would see. It is a read: it never changes the document and is not
a registry command (ADR 0080: reads go to the model, writes go through
the registry). Writing the file is a write and goes through the policy
(ADR 0081: inside a root, `.png` or `.svg`, no overwrite without
`overwrite`).

### Regions

A region renders only that rectangle of the page, at the scale, with
the rectangle's bottom left at the image's bottom left. It must lie
inside the page and have a positive size (`InvalidParams` otherwise).
The renderer gains region entry points (task P9-05, in
`crates/render`): `render_png_region(scene, options, region)` and
`render_svg_region(scene, options, region)`, which translate and clip
the scene to the rectangle. The existing functions and their options
structs stay as they are (other crates construct them with literals).

### Limits, checked before any pixel

`render::image_size` computes the image size from the page or region
and the scale, then checks, before anything is allocated:

| Limit | Value |
|---|---|
| scale | finite, positive, at most the renderer's own bound |
| `max_render_side` | 8192 pixels on either side |
| `max_render_pixels` | 16 777 216 pixels (4096 × 4096) |
| `max_response_bytes` | 48 MiB per response frame (inline base64 included) |

A request over a cap is `LimitExceeded` with the limit, the asked value
and the maximum, so the caller can lower the scale. A US Letter page at
the default 96 dpi is 816 × 1056 pixels; the pixel cap allows the same
page at about 420 dpi. An encoded image over `max_response_bytes`
(inline) is `LimitExceeded` as well; a file output has no byte cap
beyond the pixel caps.

The renderer's own caps (`MAX_DEVICE_EXTENT`, the 256 MiB pixmap bound)
stay as a second line of defence.

### Bytes or a file

Inline is the default: an agent that wants to look at its work should
not need a writable directory, and base64 in one frame is simple. A
file is for large images and for scripts that keep the result; it
returns the absolute path written.

## Consequences

- One code path renders for the canvas, the CLI and remote callers;
  render goldens keep covering it.
- `crates/control/src/render.rs` owns the size check and the format
  dispatch; `crates/render` owns the pixels (task P9-05 owns both).
- Large inline images cost a third more than their bytes (base64); the
  frame cap keeps a single response bounded.
- Rendering in the attached app runs on the UI thread (ADR 0076); a
  render at the cap takes a noticeable fraction of a second and the
  window waits. Rendering off the UI thread (from a document clone) is
  a later improvement.

## Alternatives considered

- **Render through `file.render`.** Rejected: it needs an output path,
  cannot return bytes and cannot do regions; rendering is a read, not
  a user action to undo.
- **Crop a full-page raster to the region.** Rejected: a small region
  of a large page at a high scale would allocate the whole page first.
- **Return a `file://` resource link in MCP.** Rejected for the alpha:
  inline image content is what clients show the model directly.
- **No caps, trust the caller.** Rejected by the never-crash rules
  (AGENTS.md section 6, ADR 0081).
