#!/usr/bin/env python3
"""Exercise the non-test PDF worker binary without initializing the app UI."""

import argparse
import json
from pathlib import Path
import subprocess
import tempfile
import unittest


FIXTURES = Path(__file__).resolve().parent.parent / "fixtures" / "pdf"
ART = "geometry-fonts-transparency-images.pdf"


class PdfWorkerTests(unittest.TestCase):
    def worker(self, request, fixture=ART, expect_error=False):
        with tempfile.TemporaryDirectory(prefix="lime-pdf-check-") as directory:
            root = Path(directory)
            (root / "source.pdf").write_bytes((FIXTURES / fixture).read_bytes())
            (root / "request.json").write_text(json.dumps(request), encoding="utf-8")
            result = subprocess.run(
                [str(BINARY), "--lime-pdf-worker", str(root)],
                capture_output=True,
                text=True,
                timeout=45,
            )
            self.assertEqual(result.returncode, 0, result.stderr[-4000:])
            reply_path = root / "reply.json"
            self.assertLessEqual(reply_path.stat().st_size, 2 * 1024 * 1024)
            reply = json.loads(reply_path.read_text(encoding="utf-8"))
            if expect_error:
                self.assertTrue(reply["error"], reply)
                self.assertEqual(reply["pages"], [])
                self.assertEqual(list(root.glob("page-*.rgba")), [])
                return reply, {}
            self.assertIsNone(reply["error"], reply)
            pages = {}
            for page in reply["pages"]:
                self.assertIsNone(page["error"], page)
                self.assertGreater(page["width"], 0)
                self.assertGreater(page["height"], 0)
                self.assertNotIn(page["number"], pages)
                pixels = (root / f"page-{page['number']}.rgba").read_bytes()
                self.assertEqual(len(pixels), page["width"] * page["height"] * 4)
                pages[page["number"]] = pixels
            return reply, pages

    @staticmethod
    def options(pages, background="Transparent", dpi=72):
        return {"Pages": {
            "path": "", "pages": pages, "dpi": dpi, "mode": "Layers",
            "background": background, "canvas": "FitAllPages",
        }}

    def test_inspection_retains_cropbox_and_rotation_geometry(self):
        reply, pages = self.worker("Inspect")
        self.assertEqual(reply["info"], {"pages": 2, "sizes": [[60.0, 80.0], [100.0, 100.0]]})
        self.assertEqual(pages, {})

    def test_rendering_preserves_alpha_fonts_images_and_background(self):
        reply, pages = self.worker(self.options([1, 2]))
        self.assertEqual([(p["number"], p["width"], p["height"]) for p in reply["pages"]],
                         [(1, 60, 80), (2, 100, 100)])
        self.assertIn(0, pages[1][3::4])
        self.assertTrue(any(0 < alpha < 255 for alpha in pages[2][3::4]))
        for number in (1, 2):
            self.assertTrue(any(pages[number][i] > 200 and pages[number][i + 1] < 20
                                and pages[number][i + 3] > 0
                                for i in range(0, len(pages[number]), 4)))
        _, white = self.worker(self.options([1, 2], background="White"))
        for pixels in white.values():
            self.assertTrue(all(alpha == 255 for alpha in pixels[3::4]))
        clear = next(i for i in range(0, len(pages[1]), 4) if pages[1][i + 3] == 0)
        self.assertEqual(white[1][clear:clear + 4], bytes([255, 255, 255, 255]))

    def test_page_subset_and_dpi_keep_source_page_numbers(self):
        reply, pages = self.worker(self.options([2], dpi=144))
        self.assertEqual([(p["number"], p["width"], p["height"]) for p in reply["pages"]],
                         [(2, 200, 200)])
        self.assertEqual(set(pages), {2})

    def test_thumbnails_are_bounded_and_respect_background(self):
        for background in ("Transparent", "White"):
            with self.subTest(background=background):
                reply, pages = self.worker({"Thumbnails": {
                    "pages": [1, 2], "background": background,
                }})
                self.assertEqual(set(pages), {1, 2})
                self.assertTrue(all(1 <= p["width"] <= 144 and 1 <= p["height"] <= 144
                                    for p in reply["pages"]))
                if background == "Transparent":
                    self.assertIn(0, pages[1][3::4])
                else:
                    self.assertTrue(all(alpha == 255 for pixels in pages.values() for alpha in pixels[3::4]))

    def test_invalid_requests_and_unsupported_sources_fail_without_pages(self):
        for request in (self.options([2, 1]), self.options([1, 1]), self.options([3]),
                        self.options([1], dpi=0)):
            with self.subTest(request=request):
                self.worker(request, expect_error=True)
        for fixture in ("malformed-truncated.pdf", "encrypted-rejected.pdf"):
            with self.subTest(fixture=fixture):
                self.worker("Inspect", fixture=fixture, expect_error=True)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", required=True, type=Path)
    args = parser.parse_args()
    BINARY = args.binary.resolve(strict=True)
    unittest.main(argv=[__file__], verbosity=2)
