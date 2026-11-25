"""Public API checks for an installed wheel; no FPGA hardware is accessed."""

import subprocess
import sys
import textwrap
import unittest

from nifpga_fast_fifo_recv import NifpgaFastFifoRecv


class LifecycleTests(unittest.TestCase):
    def test_stop_without_start_is_quiet_in_both_debug_modes(self):
        # Capture OS-level stdout/stderr, including Rust's println!/eprintln!.
        result = subprocess.run(
            [sys.executable, "-c", textwrap.dedent("""
                from nifpga_fast_fifo_recv import NifpgaFastFifoRecv
                for debug in (False, True):
                    reader = NifpgaFastFifoRecv(debug=debug)
                    reader.thread_stop()
                    reader.thread_stop()
                    assert not reader.thread_is_running()
                    assert reader.get_status()['last_error'] is None
            """)],
            capture_output=True,
            text=True,
            timeout=10,
        )
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertEqual(result.stdout, "")
        self.assertEqual(result.stderr, "")

    def test_invalid_sizes_raise_before_start(self):
        for options, message in [
            ({"min_packet": 0}, "min_packet"),
            ({"fifo_reading_buffer": 0}, "fifo_reading_buffer"),
            ({"fifo_reading_buffer": 3, "min_packet": 4}, "fifo_reading_buffer"),
            ({"dma_buffer_size": 0}, "dma_buffer_size"),
            ({"dma_buffer_size": 3, "min_packet": 4}, "dma_buffer_size"),
            ({"fifo_reading_buffer": sys.maxsize}, "fifo_reading_buffer"),
            ({"max_queue_chunks": 0}, "max_queue_chunks"),
            ({"max_queue_chunks": sys.maxsize}, "max_queue_chunks"),
        ]:
            with self.subTest(options=options):
                with self.assertRaisesRegex(ValueError, message):
                    NifpgaFastFifoRecv(**options)

    def test_invalid_strings_raise_before_start(self):
        for field in ("bitfile", "signature", "ni_address"):
            for value in ("", "   ", "bad\0value"):
                with self.subTest(field=field, value=value):
                    with self.assertRaisesRegex(ValueError, field):
                        NifpgaFastFifoRecv(**{field: value})

    def test_configuration_includes_defaults_and_optional_limit(self):
        reader = NifpgaFastFifoRecv()
        conf = reader.get_conf()
        self.assertEqual(conf["ni_address"], "RIO0")
        self.assertFalse(conf["debug"])
        self.assertIsNone(conf["max_queue_chunks"])
        self.assertEqual(len(reader.get_conf_as_tuple()), 10)
        reader = NifpgaFastFifoRecv(debug=True, max_queue_chunks=3)
        self.assertTrue(reader.get_conf()["debug"])
        self.assertEqual(reader.get_conf()["max_queue_chunks"], 3)
        self.assertFalse(reader.get_status()["queue_overflow"])
        self.assertEqual(reader.get_status()["elements_dropped"], 0)

    def test_valid_packet_rounding_and_empty_data(self):
        # A nonmultiple buffer remains supported and is rounded down by the worker.
        reader = NifpgaFastFifoRecv(
            min_packet=4, fifo_reading_buffer=7, max_queue_chunks=1
        )
        self.assertEqual(reader.get_data(), [])
        self.assertEqual(reader.get_data_as_list(), [])
        self.assertEqual(reader.get_data_as_ntuple(), ())
        self.assertEqual(reader.get_data_as_numpy().tolist(), [])


if __name__ == "__main__":
    unittest.main()
