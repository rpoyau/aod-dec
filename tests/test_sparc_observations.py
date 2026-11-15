"""Observation-boundary controls, requiring optional Astropy/NumPy."""
from __future__ import annotations

import json
import math
from pathlib import Path
import tempfile
import unittest

from simulations.sparc_observations import load_halogas_observations


class ObservationTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        try:
            import numpy as np
            from astropy.io import fits
        except ImportError:
            raise unittest.SkipTest("optional NumPy/Astropy unavailable")
        cls.np, cls.fits = np, fits

    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.directory = Path(self.temp.name)
        self.context = self.directory / "catalog-context.json"
        self.context.write_text(json.dumps({
            "catalog_sha256": "a" * 64,
            "cohort": [{"galaxy": "NGC3198", "input_geometry": {
                "galaxy": "NGC3198", "distance_mpc": "69/5",
                "inclination_deg": "73", "source_reference": "fixture-row"},
                "catalog": {"distance_error_mpc": "7/5", "inclination_error_deg": "3",
                            "hi_mass_1e9_msun": "10869/1000"}}],
        }))
        header = self.fits.Header()
        for key, value in {"CTYPE1": "RA---SIN", "CTYPE2": "DEC--SIN",
                           "CUNIT1": "deg", "CUNIT2": "deg",
                           "CRPIX1": 3., "CRPIX2": 3.,
                           "CRVAL1": 154., "CRVAL2": 45.,
                           "CDELT1": -4/3600, "CDELT2": 4/3600,
                           "BMAJ": 30/3600, "BMIN": 20/3600, "BPA": 0.,
                           "RADESYS": "FK5", "EQUINOX": 2000.,
                           "SPECSYS": "BARYCENT", "OBJECT": "NGC3198"}.items():
            header[key] = value
        self.header = header
        self.intensity = self.np.zeros((5, 5), dtype=float)
        self.intensity[1, 1] = 2
        self.intensity[2, 1] = 4
        self.intensity[3, 1] = 2
        self.intensity[0, 0] = -1
        self.intensity[4, 4] = self.np.nan
        self.velocity = self.np.full((5, 5), self.np.nan)
        self.velocity[1:4, 1] = [90, 100, 110]
        self.write_maps()

    def write_maps(self, intensity_header=None, velocity_header=None):
        for suffix, array, custom_header, unit in (
            ("mom0m", self.intensity, intensity_header, "JY/BEAM.km/s"),
            ("mom1m", self.velocity, velocity_header, "km/s"),
        ):
            header = (self.header if custom_header is None else custom_header).copy()
            header["BUNIT"] = unit
            self.fits.PrimaryHDU(array, header).writeto(
                self.directory / f"NGC3198-LR_{suffix}.fits", overwrite=True)

    def load(self):
        return load_halogas_observations(self.directory, "NGC3198", self.context)

    def test_missing_zero_negative_and_origin_are_distinct(self):
        observed = self.load()
        self.assertEqual(observed.metadata["finite_intensity_count"], 24)
        self.assertEqual(observed.metadata["zero_intensity_count"], 20)
        self.assertEqual(observed.metadata["negative_intensity_count"], 1)
        self.assertEqual(observed.metadata["joint_positive_count"], 3)
        self.assertEqual(observed.intensity[0, 0], -1)
        self.assertTrue(math.isnan(observed.intensity[4, 4]))
        self.assertFalse(observed.velocity_valid[0, 1])
        self.assertTrue(observed.intensity_valid[0, 1])
        self.assertAlmostEqual(observed.east_arcsec[2, 2], 0, places=7)
        self.assertAlmostEqual(observed.north_arcsec[2, 2], 0, places=7)
        self.assertGreater(observed.metadata["initial_center_arcsec"][0], 3.9)
        self.assertLess(observed.east_arcsec[2, 3], -3.9)
        self.assertIn("not assumed galaxy", observed.metadata["coordinate_origin_role"])
        self.assertEqual(observed.metadata["celestial_frame_name"], "fk5")
        self.assertIn("2000", observed.metadata["celestial_frame"])
        with self.assertRaises(ValueError):
            observed.intensity[0, 0] = 0

    def test_beam_orientation_and_signed_flux_use_source_values(self):
        observed = self.load()
        covariance = observed.metadata["beam_covariance_east_north_arcsec2"]
        self.assertAlmostEqual(covariance[0][0], 20**2 / (8*math.log(2)))
        self.assertAlmostEqual(covariance[1][1], 30**2 / (8*math.log(2)))
        self.assertAlmostEqual(covariance[0][1], 0)
        area = math.pi * 30 * 20 / (4*math.log(2))
        self.assertAlmostEqual(observed.metadata["integrated_signed_flux_jy_km_s"], 7*16/area)
        self.assertFalse(observed.metadata["column_density_maps_used"])

    def test_same_shape_shifted_coordinate_grid_rejected(self):
        shifted = self.header.copy()
        shifted["CRVAL1"] += .01
        self.write_maps(velocity_header=shifted)
        with self.assertRaisesRegex(ValueError, "WCS differs"):
            self.load()

    def test_units_and_reference_frame_are_checked(self):
        path = self.directory / "NGC3198-LR_mom1m.fits"
        with self.fits.open(path, mode="update") as hdus:
            hdus[0].header["BUNIT"] = "m/s"
        with self.assertRaisesRegex(ValueError, "unexpected source unit"):
            self.load()
        changed = self.header.copy()
        changed["SPECSYS"] = "LSRK"
        self.write_maps(changed, changed)
        with self.assertRaisesRegex(ValueError, "BARYCENT"):
            self.load()

    def test_actual_parsed_file_bytes_bind_source_identity(self):
        original = self.load()
        self.intensity[2, 1] += 1
        self.write_maps()
        changed = self.load()
        self.assertNotEqual(original.metadata["sources"][0]["sha256"],
                            changed.metadata["sources"][0]["sha256"])
        self.assertEqual(original.metadata["sources"][1]["sha256"],
                         changed.metadata["sources"][1]["sha256"])
        self.assertEqual(changed.metadata["distance_mpc_exact"], "69/5")

    def test_wrong_identity_and_infinite_samples_rejected(self):
        other = self.header.copy()
        other["OBJECT"] = "NGC2403"
        self.write_maps(intensity_header=other)
        with self.assertRaisesRegex(ValueError, "object identity"):
            self.load()
        self.intensity[1, 1] = self.np.inf
        self.write_maps()
        with self.assertRaisesRegex(ValueError, "infinite"):
            self.load()


if __name__ == "__main__":
    unittest.main()
