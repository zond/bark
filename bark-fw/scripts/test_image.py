"""The image header as image.py packs it, and its table hash, which the
firmware's `bark_image::pieces` test `the_table_hash_is_the_scripts`
holds to the same vectors. Run with python3 -m unittest in scripts/."""

import importlib.util
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


class Forged(unittest.TestCase):
    def test_one_bit_of_the_first_signature_flipped_its_key_still_named(self):
        body = firmware(40_000)
        head = bytearray(image.header(9, body))
        head[image.SIGNED_LEN] = 255
        head[image.SIGNED_LEN + 1 : image.SIGNED_LEN + image.SLOT_LEN] = bytes(range(64))
        data = bytes(head) + body
        forged = image.forge_signature(data)
        self.assertEqual(len(forged), len(data))
        differ = [i for i in range(len(data)) if data[i] != forged[i]]
        self.assertEqual(differ, [image.SIGNED_LEN + 1])
        self.assertEqual(data[differ[0]] ^ forged[differ[0]], 1)
        self.assertEqual(image.slots(forged[: image.HEADER_LEN])[0][0], 255)

    def test_an_unsigned_image_is_refused(self):
        with self.assertRaises(SystemExit):
            image.forge_signature(image.header(9, firmware(100)) + firmware(100))

    @unittest.skipUnless(importlib.util.find_spec("cryptography"), "needs the cryptography package")
    def test_a_signature_that_verified_no_longer_does(self):
        from cryptography.exceptions import InvalidSignature
        from cryptography.hazmat.primitives.asymmetric import ed25519

        key = ed25519.Ed25519PrivateKey.from_private_bytes(bytes(32))
        body = firmware(40_000)
        head = image.header(9, body)
        signed = head[: image.SIGNED_LEN]
        slot = bytes([255]) + key.sign(signed)
        head = signed + slot + head[image.SIGNED_LEN + image.SLOT_LEN :]
        forged = image.forge_signature(head + body)
        self.assertEqual(forged[: image.SIGNED_LEN], signed)
        public = key.public_key()
        public.verify(image.slots(head)[0][1], signed)
        with self.assertRaises(InvalidSignature):
            public.verify(image.slots(forged[: image.HEADER_LEN])[0][1], signed)


if __name__ == "__main__":
    unittest.main()
