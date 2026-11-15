"""Input-role and exact scale checks, independent of field evolution."""

from dataclasses import FrozenInstanceError
from fractions import Fraction as Q
import hashlib
from pathlib import Path
from tempfile import TemporaryDirectory
import unittest

from simulations.sparc_inputs import BinKey, load_inputs, physical_range, scale_inventory


HEADER = ("# Distance = 4.04 Mpc\n"
          "# Rad Vobs errV Vgas Vdisk Vbul SBdisk SBbul\n"
          "# kpc km/s km/s km/s km/s km/s L/pc^2 L/pc^2\n")
ROWS = "0.10 10.25 0.20 -3.5 8.20 0 2.3 0\n2.50 11.5 0.25 4.5 6.20 0 0.0 0\n"


class SparcInputTests(unittest.TestCase):
    def setUp(self):
        self.temporary = TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.directory = Path(self.temporary.name)

    def write(self, content=HEADER + ROWS, galaxy="Demo"):
        path = self.directory / f"{galaxy}_rotmod.dat"
        path.write_text(content)
        return path

    def test_exact_decimal_source_hash_and_target_role(self):
        path = self.write()
        data = load_inputs(self.directory)
        key = BinKey("Demo", Q(1, 10))
        self.assertEqual(data.preparation[key].vdisk_km_s, Q(41, 5))
        self.assertEqual(data.preparation[key].vgas_km_s, Q(-7, 2))
        self.assertEqual(data.preparation[key].distance_mpc, Q(101, 25))
        self.assertEqual(data.targets[key].vobs_km_s, Q(41, 4))
        self.assertEqual(data.targets[key].u_obs_km2_s2, Q(1681, 16))
        self.assertEqual(data.targets[key].sigma_u_linear_km2_s2, Q(41, 10))
        self.assertEqual(data.sources[0].sha256, hashlib.sha256(path.read_bytes()).hexdigest())
        self.assertEqual(data.sources[0].rows, ((key, 4), (BinKey("Demo", Q(5, 2)), 5)))

    def test_target_change_cannot_change_preparation(self):
        self.write()
        original = load_inputs(self.directory)
        self.write(HEADER + ROWS.replace("10.25 0.20", "999.5 39.25"))
        changed = load_inputs(self.directory)
        self.assertEqual(original.preparation, changed.preparation)
        self.assertNotEqual(original.targets, changed.targets)
        self.assertNotEqual(original.sources[0].sha256, changed.sources[0].sha256)
        self.assertFalse(hasattr(next(iter(original.preparation.values())), "vobs_km_s"))

    def test_rows_and_mappings_are_immutable(self):
        self.write()
        data = load_inputs(self.directory)
        key = next(iter(data.preparation))
        with self.assertRaises(TypeError):
            data.preparation[key] = data.preparation[key]
        with self.assertRaises(FrozenInstanceError):
            data.preparation[key].vgas_km_s = Q(0)

    def test_malformed_and_nonfinite_rows_are_rejected(self):
        invalid_rows = (
            "1 2 3 4 5 6 7", "1 2 3 4 5 6 7 8 9", "1 nan 3 4 5 6 7 8",
            "1 2 inf 4 5 6 7 8", "1 2 3 -Infinity 5 6 7 8", "1 2 3 4/3 5 6 7 8",
            "1 2 3 True 5 6 7 8", "0 2 3 4 5 6 7 8", "1 0 3 4 5 6 7 8",
            "1 2 0 4 5 6 7 8", "1 2 -1 4 5 6 7 8", "1 2 3 4 5 6 -1 8",
        )
        for row in invalid_rows:
            with self.subTest(row=row):
                self.write(HEADER + row + "\n")
                with self.assertRaises(ValueError):
                    load_inputs(self.directory)

    def test_duplicate_and_descending_radii_are_rejected(self):
        for second_radius in ("0.100", "0.09"):
            with self.subTest(radius=second_radius):
                self.write(HEADER + ROWS.replace("2.50", second_radius))
                with self.assertRaisesRegex(ValueError, "strictly ascending"):
                    load_inputs(self.directory)

    def test_missing_or_wrong_units_and_schema_are_rejected(self):
        headers = (
            HEADER.replace("# Distance = 4.04 Mpc\n", ""),
            HEADER.replace("Mpc", "kpc"), HEADER.replace("4.04", "0"),
            HEADER.replace("# Rad Vobs errV Vgas Vdisk Vbul SBdisk SBbul\n", ""),
            HEADER.replace("Vobs errV", "errV Vobs"),
            HEADER.replace("# kpc km/s km/s km/s km/s km/s L/pc^2 L/pc^2\n", ""),
            HEADER.replace("L/pc^2 L/pc^2", "L/kpc^2 L/pc^2"),
            HEADER + "# Distance = 4.04 Mpc\n",
        )
        for header in headers:
            with self.subTest(header=header):
                self.write(header + ROWS)
                with self.assertRaises(ValueError):
                    load_inputs(self.directory)

    def test_empty_directory_and_empty_file_rejected(self):
        with self.assertRaises(ValueError):
            load_inputs(self.directory)
        self.write(HEADER)
        with self.assertRaises(ValueError):
            load_inputs(self.directory)

    def test_exact_decade_boundaries_and_unit_invariance(self):
        for ratio, exponent in ((Q(1), 0), (Q(999, 100), 0), (Q(10), 1),
                                (Q(100), 2), (Q(10000000000000000001, 10**16), 3)):
            with self.subTest(ratio=ratio):
                values = (Q(1, 10), ratio / 10)
                original = physical_range(values, "kpc")
                rescaled = physical_range(tuple(v * 1000 for v in values), "pc")
                self.assertEqual(original["max_over_min_exact"], str(ratio))
                self.assertEqual(original["decade_bracket"], {
                    "lower_exponent_inclusive": exponent, "upper_exponent_exclusive": exponent + 1})
                for key in ("max_over_min_exact", "log10_span_display", "decade_bracket"):
                    self.assertEqual(original[key], rescaled[key])
                self.assertIsNone(original["native_octave_span"])

    def test_inventory_keeps_signed_proxy_and_zero_photometry(self):
        self.write()
        inventory = scale_inventory(load_inputs(self.directory))
        galaxy = inventory["galaxies"][0]
        self.assertEqual(galaxy["radius"]["max_over_min_exact"], "25")
        self.assertEqual(galaxy["columns"]["Vgas"]["negative_count"], 1)
        self.assertEqual(galaxy["columns"]["SBdisk"]["zero_count"], 1)
        self.assertIsNone(galaxy["columns"]["Vbul"]["nonzero_absolute_range"])
        self.assertIsNone(inventory["native_metric_bridge"])
        self.assertTrue(inventory["observed_radial_extent_is_not_a_model_boundary"])

    def test_same_radius_in_distinct_galaxies_has_distinct_identity(self):
        self.write(galaxy="Alpha")
        self.write(galaxy="Beta")
        data = load_inputs(self.directory)
        self.assertEqual(len(data.preparation), 4)
        self.assertIn(BinKey("Alpha", Q(1, 10)), data.preparation)
        self.assertIn(BinKey("Beta", Q(1, 10)), data.preparation)

    def test_positive_fraction_domain_is_explicit(self):
        for values in ((), (Q(0),), (Q(-1),), (0.1,), (True,)):
            with self.subTest(values=values):
                with self.assertRaises(ValueError):
                    physical_range(values, "kpc")


if __name__ == "__main__":
    unittest.main()
