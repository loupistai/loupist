import base64
import json
import re
import unittest

from PIL import Image, ImageChops

from make_loupist_icon import (
    APP,
    ICNS_FRAMES,
    ICO_SIZES,
    PNG_SIZES,
    SIGNIN_ICON,
    SIGNIN_PAGE,
    read_face,
    render_sizes,
)


class AppIconTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        face = read_face()
        sizes = (
            set(PNG_SIZES.values()) | set(ICO_SIZES) | {size for _, size in ICNS_FRAMES}
        )
        cls.light = render_sizes(face, "light", sizes)
        cls.dark = render_sizes(face, "dark", [1024])[1024]
        cls.bundle = APP / "src-tauri"

    def assert_pixels(self, actual, expected):
        self.assertEqual(actual.size, expected.size)
        self.assertIsNone(
            ImageChops.difference(actual.convert("RGBA"), expected).getbbox(
                alpha_only=False
            ),
            "Icon pixels differ from the current face or theme tokens. Regenerate the icons.",
        )

    def test_pngs_match_the_avatar_and_theme_tokens(self):
        for path, size in PNG_SIZES.items():
            with self.subTest(path=path), Image.open(APP / path) as image:
                self.assert_pixels(image, self.light[size])
        with Image.open(APP / "assets/loupist-icon-dark.png") as image:
            self.assert_pixels(image, self.dark)

    def test_bundle_icon_frames_match_the_generated_pngs(self):
        config = json.loads(
            (self.bundle / "tauri.conf.json").read_text(encoding="utf-8")
        )
        self.assertCountEqual(
            config["bundle"]["icon"],
            [
                "icons/32x32.png",
                "icons/128x128.png",
                "icons/128x128@2x.png",
                "icons/icon.icns",
                "icons/icon.ico",
            ],
        )
        with Image.open(self.bundle / "icons/icon.icns") as image:
            self.assertTrue(
                {b"is32", b"s8mk", b"il32", b"l8mk"} <= image.icns.dct.keys(),
                "Native 16 and 32 point frames require classic RGB and alpha chunks.",
            )
            sizes = [
                (size, size, scale)
                for size in (16, 32, 128, 256, 512)
                for scale in (1, 2)
            ]
            self.assertCountEqual(image.info["sizes"], sizes)
            for width, height, scale in sizes:
                with self.subTest(icns=(width, height, scale)):
                    self.assert_pixels(
                        image.icns.getimage((width, height, scale)),
                        self.light[width * scale],
                    )
        with Image.open(self.bundle / "icons/icon.ico") as image:
            self.assertEqual(image.ico.sizes(), {(size, size) for size in ICO_SIZES})
            for size in ICO_SIZES:
                with self.subTest(ico=size):
                    self.assert_pixels(
                        image.ico.getimage((size, size)), self.light[size]
                    )

    def test_sign_in_page_embeds_the_128_pixel_png(self):
        embedded = re.findall(
            SIGNIN_ICON, (APP / SIGNIN_PAGE).read_text(encoding="utf-8")
        )
        self.assertEqual(len(embedded), 1)
        self.assertEqual(
            base64.b64decode(embedded[0], validate=True),
            (self.bundle / "icons/128x128.png").read_bytes(),
        )


if __name__ == "__main__":
    unittest.main()
