"""Parser-only regression checks; importing the harness starts no engine."""
import unittest

from cache_replay import phase_events, validate_phase_events


class CacheReplayBoundaryTests(unittest.TestCase):
    entry = "/book/main.typ"
    compiled = "[INFO] /book/main.typ: compilation succeeded in 1s"
    started = "automatic cache sweep queued for 1ms"
    swept = "evict comemo cache in 1ms"

    def check(self, lines, **kwargs):
        return validate_phase_events(phase_events(lines, self.entry), **kwargs)

    def test_fresh_worker_after_compile_is_accepted(self):
        self.assertEqual(self.check([self.compiled, self.started, self.swept]), 2)

    def test_incomplete_worker_keeps_waiting(self):
        for lines in ([], [self.compiled], [self.compiled, self.started]):
            with self.subTest(lines=lines):
                self.assertIsNone(self.check(lines))

    def test_older_or_unidentified_sweep_is_rejected(self):
        for lines in ([self.started, self.compiled, self.swept],
                      [self.compiled, self.swept]):
            with self.subTest(lines=lines), self.assertRaises(ValueError):
                self.check(lines)

    def test_extra_or_other_document_work_is_rejected(self):
        chain = [self.compiled, self.started, self.swept]
        for suffix in ([self.swept], chain,
                       ["[INFO] /book/other.typ: compilation succeeded in 1s"],
                       ["compile timing outcome=discarded-after-execution"]):
            with self.subTest(suffix=suffix), self.assertRaises(ValueError):
                self.check(chain + suffix)

    def test_warmup_accepts_only_its_final_fresh_chain(self):
        chain = [self.compiled, self.started, self.swept]
        self.assertEqual(self.check(chain + chain, warm=True), 5)
        with self.assertRaises(ValueError):
            self.check(chain + [self.started, self.compiled, self.swept], warm=True)


if __name__ == "__main__":
    unittest.main()
