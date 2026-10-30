import unittest

import gen


class Classify(unittest.TestCase):
    def kind(self, body):
        return gen.classify("", body)

    def test_the_clients_own_read_methods(self):
        # xw.eg, xw.jo and xw.ed as decompiled
        self.assertEqual(self.kind("return ((this.ah[-1851950461 * this.ab - 1] & 0xFF) << 8) + (this.ah[-1851950461 * this.ab - 2] - 128 & 0xFF);"), "u16leadd")
        self.assertEqual(
            self.kind("return (var0.ah[var0.ab * -1851950461 - 2] & 0xFF) + ((var0.ah[-1851950461 * var0.ab - 4] & 0xFF) << 16) + ((var0.ah[var0.ab * -1851950461 - 3] & 0xFF) << 24) + ((var0.ah[-1851950461 * var0.ab - 1] & 0xFF) << 8);"),
            "i32:2.3.0.1",
        )
        self.assertEqual(self.kind("return this.ah[(this.ab += -1933171669) * -1851950461 - 1] - 128 & 0xFF;"), "u8add")

    def test_a_decoys_junk_mask_is_not_a_read(self):
        self.assertIsNone(self.kind("int v = (this.ah[this.ab * 1 - 2] & -1073139048) + ((this.ah[this.ab * 1 - 1] & 255) << 8); return v;"))

    def test_signed_shorts_need_their_own_sign_test(self):
        body = "int v = ((this.ah[this.ab * 3 - 2] & 255) << 8) + (this.ah[this.ab * 3 - 1] & 255); if (v > 32767) { v -= 65536; } return v;"
        self.assertEqual(self.kind(body), "i16be")
        self.assertIsNone(self.kind(body.replace("32767", "12345")))


if __name__ == "__main__":
    unittest.main()
