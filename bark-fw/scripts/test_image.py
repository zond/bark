"""The image header as image.py packs it, and its table hash, which the
firmware's `bark_image::pieces` test `the_table_hash_is_the_scripts`
holds to the same vectors. Run with python3 -m unittest in scripts/."""

import struct
import unittest

import image
import uf2


def firmware(n: int) -> bytes:
    return bytes(i % 251 for i in range(n))


class TableHash(unittest.TestCase):
    def test_the_vectors_the_firmware_checks(self):
        for n, want in [
            (40_000, "60157232e6c3bf5dec13d14b5770636e0047f1d46b93521762a5db6a55817bb3"),
            (2 * 64 * 247 - 256, "838fcb1a622ac4ac1e1f104a28fbf8e7626d45465169447342cc09d85948b312"),
            (1, "42c4462d671587364a6d739226b8c42ff1b1aa9f53399a8b25f723bb7b420efa"),
        ]:
            self.assertEqual(uf2.table_hash(firmware(n)).hex(), want, n)


class Header(unittest.TestCase):
    def test_format_2_signs_80_bytes_and_has_two_slots(self):
        body = firmware(40_000)
        h = image.header(9, body)
        self.assertEqual(len(h), image.HEADER_LEN)
        self.assertEqual(h[:5], b"bark\x02")
        self.assertEqual(struct.unpack_from("<II", h, 8), (9, len(body)))
        self.assertEqual(h[48:80], uf2.table_hash(body))
        self.assertEqual(image.SIGNED_LEN + image.SLOTS * image.SLOT_LEN, 210)
        self.assertEqual(h[80:], bytes(image.HEADER_LEN - 80))
        self.assertEqual([k for k, _ in image.slots(h)], [0, 0])


if __name__ == "__main__":
    unittest.main()
