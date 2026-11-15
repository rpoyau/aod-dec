from fractions import Fraction as Q
from pathlib import Path
from tempfile import TemporaryDirectory
import unittest

from scripts.reconstruct_sparc import run, write_report
from simulations.sparc_reconstruction import load_geometries


DATA = Path(__file__).resolve().parents[1]/'data/sparc'


class SparcPipelineTests(unittest.TestCase):
    def test_official_catalog_field_alignment_and_distance_join(self):
        galaxies = ('DDO154', 'NGC2403', 'NGC2841', 'NGC3198', 'NGC6503')
        geometry, binding = load_geometries(DATA/'SPARC_Lelli2016c.mrt', galaxies)
        self.assertEqual(tuple(geometry[g].disk_scale_kpc for g in galaxies),
                         tuple(Q(s) for s in ('.37', '1.39', '3.64', '3.14', '2.16')))
        self.assertEqual(tuple(geometry[g].inclination_deg for g in galaxies), (64,63,76,73,74))
        self.assertEqual(binding['rows']['NGC3198']['rotation_source_references'], 'Da06,Be91,Be87')
        self.assertEqual(binding['rows']['NGC3198']['distance_error_mpc'], Q('1.4'))

    def test_full_cohort_control_inverse_and_missing_history_status(self):
        result = run(DATA, DATA/'SPARC_Lelli2016c.mrt')
        control = result['frozen_g0_control']
        self.assertEqual(control['bins'], 209)
        self.assertEqual(control['galaxy_count'], 5)
        self.assertAlmostEqual(float(control['primary_mape_percent']), 26.570553423860428, places=12)
        self.assertAlmostEqual(float(control['equal_galaxy_mape_percent']), 28.44053183847862, places=12)
        self.assertEqual(result['display']['negative_required_contributions'], 3)
        self.assertLess(result['target_conditioned_scalar_fit']['comparison']['primary_mape_percent'],
                        control['primary_mape_percent'])
        self.assertIn('in-sample', result['target_conditioned_scalar_fit']['exposure'])
        self.assertTrue(result['stage_status']['aod_evolution'].startswith('not run:'))
        self.assertEqual(sum(len(g['rows']) for g in result['stellar_reconstruction']), 209)
        self.assertTrue(all(row['projection_residual_lsun_pc2'] == 0
            for galaxy in result['stellar_reconstruction'] for row in galaxy['rows']))
        self.assertTrue(all(g['rounded_endpoint_continuation_mass_over_pi_msun'] == 0
                            for g in result['stellar_reconstruction']))
        self.assertTrue(all(g['outer_positive_anchor_extension']['mass_over_pi_msun'] > 0
                            for g in result['stellar_reconstruction']))
        with TemporaryDirectory() as tmp:
            out = Path(tmp)
            write_report(result, out)
            self.assertEqual(len((out/'stellar-density.csv').read_text().splitlines()), 210)
            self.assertEqual(len((out/'required-current.csv').read_text().splitlines()), 210)
            self.assertIn('has not run', (out/'REPORT.md').read_text())

    def test_missing_duplicate_and_truncated_catalog_rows_rejected(self):
        content = (DATA/'SPARC_Lelli2016c.mrt').read_text()
        row = next(line for line in content.splitlines() if line.split() and line.split()[0] == 'DDO154')
        with TemporaryDirectory() as tmp:
            path = Path(tmp)/'catalog.mrt'
            for broken in (content.replace(row, ''), content+'\n'+row,
                           content.replace(row, '     DDO154 10   4.04')):
                path.write_text(broken)
                with self.assertRaises(ValueError):
                    load_geometries(path, ('DDO154',))


if __name__ == '__main__':
    unittest.main()
