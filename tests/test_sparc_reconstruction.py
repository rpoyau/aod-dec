from dataclasses import replace
from fractions import Fraction as Q
import math
import unittest

from simulations.sparc_inputs import BinKey, PreparationRow
from simulations.sparc_reconstruction import DiskGeometry, ReconstructedDisk


def disk():
    geo = DiskGeometry('A', Q(4), Q(2), Q(60), 'test catalog row')
    rows = tuple(PreparationRow(BinKey('A', Q(r)), Q(0), Q(1), Q(0), Q(sb), Q(0), Q(4))
                 for r, sb in ((1, 6), (3, 2)))
    return ReconstructedDisk(geo, rows)


class ReconstructionTests(unittest.TestCase):
    def test_projection_at_data_and_intermediate_radii(self):
        model = disk()
        self.assertEqual(model.project_column(Q(1)), 6)
        self.assertEqual(model.project_column(Q(2)), 4)
        self.assertEqual(model.project_column(Q(3)), 2)
        self.assertEqual(model.column_mass(Q(2)), 2)
        # Independent quadrature of the 3D density, including the tiny omitted
        # tail, checks the pc/kpc conversion rather than repeating the accessor.
        h = model.geometry.estimated_height_kpc
        steps = 20000
        dz = 20*h/steps
        numeric = 2*sum(model.volume_density(Q(2), (j+.5)*dz)*1000*dz for j in range(steps))
        self.assertAlmostEqual(numeric, 2, places=6)

    def test_photometry_does_not_identify_height(self):
        model = disk()
        h = model.geometry.estimated_height_kpc
        self.assertAlmostEqual(model.volume_density(Q(2), 0, height_kpc=2*h),
                               model.volume_density(Q(2), 0)/2)
        # For either height the analytic normalization is one, so density
        # changes are not new photometric predictions.
        for height in (h, 2*h):
            self.assertAlmostEqual(model.volume_density(Q(2), 0, height_kpc=height)*2000*height, 2)

    def test_tail_budget_not_renormalized_inside_band(self):
        model = disk()
        budget = model.band_accounting(Q(2), model.geometry.estimated_height_kpc)
        self.assertAlmostEqual(budget['inside']+budget['above']+budget['below'], 2)
        self.assertAlmostEqual(budget['above']+budget['below'], 2/math.e)
        zero = model.band_accounting(Q(2), 0)
        self.assertEqual(zero['inside'], 0)
        self.assertEqual(zero['above'], 1)

    def test_exact_radial_mass_and_separate_outer_extrapolation(self):
        model = disk()
        # Sigma=4-R on [1,3]; 2 int_1^3 R(4-R)dR = 44/3.
        self.assertEqual(model.annular_mass_over_pi(), Q(44, 3)*10**6)
        self.assertEqual(model.outer_extrapolated_mass_over_pi(), 20*10**6)
        self.assertIsNone(model.report()['central_mass'])
        for r in (Q(1, 2), Q(4)):
            with self.assertRaisesRegex(ValueError, 'outside tabulated'):
                model.project_column(r)

    def test_rounded_zero_endpoint_does_not_erase_inferred_tail(self):
        model = disk()
        changed = ReconstructedDisk(model.geometry,
            (model.rows[0], replace(model.rows[1], sbdisk_lsun_pc2=Q(0))))
        self.assertEqual(changed.outer_extrapolated_mass_over_pi(), 0)
        extension = changed.positive_anchor_extension()
        self.assertGreater(extension['mass_over_pi_msun'], 0)
        self.assertIsNone(extension['physical_tail_uncertainty'])
        self.assertEqual(extension['anchor_radius_kpc'], 1)

    def test_bad_geometry_domain_or_units_binding_rejected(self):
        model = disk()
        for geometry in (replace(model.geometry, galaxy='B'), replace(model.geometry, distance_mpc=Q(5))):
            with self.assertRaises(ValueError):
                ReconstructedDisk(geometry, model.rows)
        for value in (True, 1.0, Q(-1), Q(0)):
            with self.assertRaises(ValueError):
                replace(model.geometry, disk_scale_kpc=value)
        for value in (float('nan'), float('inf'), -1, True):
            with self.assertRaises(ValueError):
                model.volume_density(Q(2), 0, height_kpc=value)
        with self.assertRaises(ValueError):
            ReconstructedDisk(model.geometry, tuple(reversed(model.rows)))

    def test_signed_gas_and_rotation_targets_do_not_become_disk_density(self):
        model = disk()
        changed = ReconstructedDisk(model.geometry,
            tuple(replace(row, vgas_km_s=Q(-999), vdisk_km_s=Q(432)) for row in model.rows))
        self.assertEqual(model.report(), changed.report())


if __name__ == '__main__':
    unittest.main()
