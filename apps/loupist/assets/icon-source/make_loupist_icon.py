"""Regenerate the Loupist app icons and the provider sign-in page's embedded copy."""

import base64
from io import BytesIO
import json
from pathlib import Path
import re
import struct

from PIL import Image, ImageColor, ImageDraw, ImageFilter


APP = Path(__file__).resolve().parents[2]
CANVAS = 1024
SUPERSAMPLE = 4
TILE_ORIGIN, TILE_SIZE, TILE_RADIUS = 100, 824, 186
TILE_GRADIENTS = {
    "light": ("#f7f3ea", "#ece6d8"),
    "dark": ("#201d33", "#121019"),
}
PNG_SIZES = {
    "assets/loupist-icon.png": 1024,
    "assets/loupist-favicon.png": 32,
    "src-tauri/icons/32x32.png": 32,
    "src-tauri/icons/128x128.png": 128,
    "src-tauri/icons/128x128@2x.png": 256,
}
ICNS_FRAMES = (
    (b"ic11", 32),
    (b"ic12", 64),
    (b"ic07", 128),
    (b"ic13", 256),
    (b"ic08", 256),
    (b"ic14", 512),
    (b"ic09", 512),
    (b"ic10", 1024),
)
ICNS_LEGACY_FRAMES = ((b"is32", b"s8mk", 16), (b"il32", b"l8mk", 32))
ICO_SIZES = (16, 24, 32, 48, 64, 256)
SIGNIN_PAGE = "agent/provider-signin-page.mjs"
SIGNIN_ICON = r'(?<=data:image/png;base64,)[^"]+'


def css_properties(source, selector):
    source = re.sub(r"/\*.*?\*/", "", source, flags=re.S)
    rule = re.search(
        rf"^{re.escape(selector)}\s*(?:,[^{{}}]+)?\{{([^{{}}]*)\}}", source, re.M
    )
    if not rule:
        raise ValueError(f"Missing CSS rule for {selector}")
    return dict(re.findall(r"([\w-]+)\s*:\s*([^;]+);", rule[1]))


def read_face(app=APP):
    source = (app / "src/lib/loupistAvatar.ts").read_text(encoding="utf-8")
    definition = re.search(
        r"export const LOUPIST_FACE = (\{.*?\}) as const;", source, re.S
    )
    if not definition:
        raise ValueError("Missing LOUPIST_FACE in loupistAvatar.ts")
    # The drawing is a data literal with bare keys and trailing commas.
    quoted = re.sub(r"(\b\w+):", r'"\1":', definition[1])
    geometry = json.loads(re.sub(r",\s*}", "}", quoted))
    face_css = (app / "src/styles/loupist-face.css").read_text(encoding="utf-8")
    styles = {
        part: css_properties(face_css, f".loupist-face .loupist-face-{part}")
        for part in ("lens", "handle", "mouth")
    }
    tokens = (app / "src/styles/tokens.css").read_text(encoding="utf-8")
    palettes = {
        theme: {
            part: props[f"--gg-color-{token}"].strip()
            for part, token in (
                ("body", "face"),
                ("ink", "face-ink"),
                ("detail", "face-detail"),
            )
        }
        for theme, props in (
            ("light", css_properties(tokens, ":root")),
            ("dark", css_properties(tokens, ':root[data-theme="dark"]')),
        )
    }
    return geometry, styles, palettes


def flatten_path(path):
    """Sample the SVG commands used by LOUPIST_FACE, including implicit repetitions."""
    tokens = iter(re.findall(r"[A-Za-z]|[-+]?(?:\d*\.\d+|\d+)|[^\s,]", path))
    command = None
    points = []
    current = (0, 0)
    previous_control = None
    for token in tokens:
        if token.isalpha():
            command = token
            if command == "z":
                points.append(points[0])
                break
            token = next(tokens)
        arity = {"M": 2, "l": 2, "c": 6, "S": 4, "q": 4}.get(command)
        if not arity:
            raise ValueError(f"Unsupported avatar path command: {command}")
        coords = [float(token), *(float(next(tokens)) for _ in range(arity - 1))]
        pairs = list(zip(coords[::2], coords[1::2]))
        x, y = current
        if command.islower():
            pairs = [(x + dx, y + dy) for dx, dy in pairs]
        end = pairs[-1]
        if command in ("c", "S"):
            if command == "c":
                first, second = pairs[:2]
            else:
                cx, cy = previous_control or current
                first, second = (2 * x - cx, 2 * y - cy), pairs[0]
            controls = (current, first, second, end)
            previous_control = second
        elif command == "q":
            controls = (current, pairs[0], end)
            previous_control = None
        else:
            controls = ()
            previous_control = None

        if controls:
            for step in range(1, 64):
                t = step / 64
                u = 1 - t
                weights = (u**3, 3 * u**2 * t, 3 * u * t**2, t**3)
                if command == "q":
                    weights = (u**2, 2 * u * t, t**2)
                points.append(
                    tuple(
                        sum(w * p for w, p in zip(weights, axis))
                        for axis in zip(*controls)
                    )
                )
        points.append(end)
        current = end
    if next(tokens, None) is not None:
        raise ValueError("Unexpected data after avatar path closes")
    return points


def draw_tile(theme):
    edge = CANVAS * SUPERSAMPLE
    start, span, radius = (
        n * SUPERSAMPLE for n in (TILE_ORIGIN, TILE_SIZE, TILE_RADIUS)
    )
    shadow = Image.new("L", (edge, edge))
    offset = 14 * SUPERSAMPLE
    ImageDraw.Draw(shadow).rounded_rectangle(
        (start, start + offset, start + span - 1, start + span + offset - 1),
        radius=radius,
        fill=round(255 * 0.37),
    )
    icon = Image.new("RGBA", (edge, edge))
    icon.putalpha(shadow.filter(ImageFilter.GaussianBlur(20 * SUPERSAMPLE)))

    top, bottom = map(ImageColor.getrgb, TILE_GRADIENTS[theme])
    strip = Image.new("RGB", (1, span))
    strip.putdata(
        [
            tuple(round(a + (b - a) * (row + 0.5) / span) for a, b in zip(top, bottom))
            for row in range(span)
        ]
    )
    tile_mask = Image.new("L", (span, span))
    ImageDraw.Draw(tile_mask).rounded_rectangle(
        (0, 0, span - 1, span - 1),
        radius=radius,
        fill=255,
    )
    icon.paste(
        strip.resize(tile_mask.size, Image.Resampling.NEAREST),
        (start, start),
        tile_mask,
    )
    return icon


def draw_face(icon, geometry, styles, colors, scale):
    offset = (CANVAS - 48 * scale) / 2
    pixels_per_unit = scale * SUPERSAMPLE
    inset = offset * SUPERSAMPLE

    def point(x, y):
        return (inset + x * pixels_per_unit, inset + y * pixels_per_unit)

    def ellipse(x, y, rx, ry):
        return (*point(x - rx, y - ry), *point(x + rx, y + ry))

    def stroke(path, width, color):
        points = [point(*pair) for pair in flatten_path(path)]
        draw.line(
            points, fill=color, width=round(width * pixels_per_unit), joint="curve"
        )
        radius = width * pixels_per_unit / 2
        # Cover the caps and the joins, where short raster segments can leave gaps.
        for x, y in points:
            draw.ellipse((x - radius, y - radius, x + radius, y + radius), fill=color)

    draw = ImageDraw.Draw(icon)
    draw.polygon(
        [point(*p) for p in flatten_path(geometry["body"])], fill=colors["body"]
    )
    lens = geometry["lens"]
    glass = styles["lens"]
    opacity = float(glass["fill-opacity"])
    # The glass sits entirely on the opaque body. Blend its fill before its stroke.
    body_rgb = ImageColor.getrgb(colors["body"])
    detail_rgb = ImageColor.getrgb(colors["detail"])
    glass_rgb = tuple(
        round(a * (1 - opacity) + b * opacity) for a, b in zip(body_rgb, detail_rgb)
    )
    draw.ellipse(ellipse(lens["cx"], lens["cy"], lens["r"], lens["r"]), fill=glass_rgb)
    width = float(glass["stroke-width"])
    outer = lens["r"] + width / 2
    draw.ellipse(
        ellipse(lens["cx"], lens["cy"], outer, outer),
        outline=colors["detail"],
        width=round(width * pixels_per_unit),
    )
    stroke(
        geometry["handle"], float(styles["handle"]["stroke-width"]), colors["detail"]
    )
    for eye in geometry["eyes"]:
        draw.ellipse(
            ellipse(eye["cx"], eye["cy"], **geometry["eye"]), fill=colors["ink"]
        )
    stroke(geometry["mouth"], float(styles["mouth"]["stroke-width"]), colors["ink"])


def render_sizes(face, theme, sizes):
    geometry, styles, palettes = face
    tile = draw_tile(theme)
    images = {}
    for scale, group in (
        (14, [size for size in sizes if size > 32]),
        (17, [size for size in sizes if size <= 32]),
    ):
        if group:
            master = tile.copy()
            draw_face(master, geometry, styles, palettes[theme], scale)
            for size in group:
                images[size] = master.resize((size, size), Image.Resampling.LANCZOS)
    return images


def png_bytes(image):
    output = BytesIO()
    image.save(output, format="PNG")
    return output.getvalue()


def icns_chunk(kind, data):
    return struct.pack(">4sI", kind, len(data) + 8) + data


def icns_rgb(image):
    # The classic ICNS RGB format uses separate channels of run-length encoded
    # bytes. Literal blocks cover up to 128 pixels each.
    data = bytearray()
    for channel in "RGB":
        values = image.getchannel(channel).tobytes()
        for start in range(0, len(values), 128):
            block = values[start : start + 128]
            data.append(len(block) - 1)
            data.extend(block)
    return bytes(data)


def encode_icns(images):
    # Retina and larger frames use PNG. At 16 and 32 points, macOS uses classic
    # RGB data with a separate alpha mask for the non-Retina frames.
    chunks = [icns_chunk(kind, png_bytes(images[size])) for kind, size in ICNS_FRAMES]
    for rgb, alpha, size in ICNS_LEGACY_FRAMES:
        image = images[size]
        chunks.append(icns_chunk(rgb, icns_rgb(image)))
        chunks.append(icns_chunk(alpha, image.getchannel("A").tobytes()))
    return icns_chunk(b"icns", b"".join(chunks))


def generate(app=APP):
    face = read_face(app)
    sizes = (
        set(PNG_SIZES.values())
        | {size for _, size in ICNS_FRAMES}
        | {size for _, _, size in ICNS_LEGACY_FRAMES}
        | set(ICO_SIZES)
    )
    icons = render_sizes(face, "light", sizes)
    for path, size in PNG_SIZES.items():
        icons[size].save(app / path)
    dark = render_sizes(face, "dark", [CANVAS])[CANVAS]
    dark.save(app / "assets/loupist-icon-dark.png")
    (app / "src-tauri/icons/icon.icns").write_bytes(encode_icns(icons))
    icons[256].save(
        app / "src-tauri/icons/icon.ico",
        sizes=[(size, size) for size in ICO_SIZES],
        append_images=[icons[size] for size in ICO_SIZES if size != 256],
    )
    page = app / SIGNIN_PAGE
    encoded = base64.b64encode(
        (app / "src-tauri/icons/128x128.png").read_bytes()
    ).decode("ascii")
    source, replaced = re.subn(SIGNIN_ICON, encoded, page.read_text(encoding="utf-8"))
    if replaced != 1:
        raise ValueError("Expected one embedded icon in the provider sign-in page")
    page.write_text(source, encoding="utf-8")


if __name__ == "__main__":
    generate()
