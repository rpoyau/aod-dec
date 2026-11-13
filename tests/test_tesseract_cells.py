"""Independent combinatorial and orientation checks for the declared 4-cube."""
from fractions import Fraction
from itertools import combinations
from math import comb
import unittest

from simulations.tesseract_cells import (
    FACETS, TESSERACT, boundary, boundary_chain, cells, dimension,
    facet_interfaces, facet_neighbors,
)


class TesseractCellsTests(unittest.TestCase):
    def test_complete_cell_inventory_and_euler_characteristics(self):
        counts = tuple(len(cells(d)) for d in range(5))
        self.assertEqual(counts, (16, 32, 24, 8, 1))
        # Choose the free axes, then assign each remaining endpoint bit.
        self.assertEqual(counts, tuple(comb(4, d) * 2 ** (4 - d) for d in range(5)))
        all_cells = tuple(c for d in range(5) for c in cells(d))
        self.assertEqual(len(set(all_cells)), 81)
        self.assertEqual(sum((-1) ** d * counts[d] for d in range(5)), 1)
        self.assertEqual(sum((-1) ** d * counts[d] for d in range(4)), 0)
        self.assertEqual(cells(3), FACETS)
        self.assertEqual(cells(4), (TESSERACT,))

    def test_independently_written_orientation_examples(self):
        self.assertEqual(boundary("0000"), {})
        self.assertEqual(boundary("*000"), {"0000": -1, "1000": 1})
        self.assertEqual(boundary("**00"), {
            "0*00": -1, "1*00": 1, "*000": 1, "*100": -1,
        })
        self.assertEqual(boundary(TESSERACT), {
            "0***": -1, "1***": 1, "*0**": 1, "*1**": -1,
            "**0*": -1, "**1*": 1, "***0": 1, "***1": -1,
        })

    def test_every_cell_has_correct_faces_and_zero_second_boundary(self):
        for d in range(5):
            for cell in cells(d):
                with self.subTest(cell=cell):
                    faces = boundary(cell)
                    self.assertEqual(len(faces), 2 * d)
                    for face, sign in faces.items():
                        self.assertEqual(dimension(face), d - 1)
                        self.assertIn(sign, (-1, 1))
                        changed = [i for i in range(4) if face[i] != cell[i]]
                        self.assertEqual(len(changed), 1)
                        self.assertEqual(cell[changed[0]], "*")
                        self.assertIn(face[changed[0]], "01")
                    self.assertEqual(boundary_chain(faces), {})

    def test_region_internal_cancellation_preserves_input_chain(self):
        orientation = boundary(TESSERACT)
        chain = {"0***": orientation["0***"], "*0**": orientation["*0**"]}
        retained = dict(chain)
        result = boundary_chain(chain)
        self.assertEqual(chain, retained)
        self.assertEqual(len(result), 10)  # two cubes, shared square removed twice
        self.assertNotIn("00**", result)
        self.assertEqual(boundary_chain(result), {})
        opposite = {"0***": orientation["0***"], "1***": orientation["1***"]}
        self.assertEqual(len(boundary_chain(opposite)), 12)
        self.assertEqual(boundary_chain(orientation), {})

    def test_every_facet_subset_boundary_closes(self):
        orientation = boundary(TESSERACT)
        # Every possible selected region in the eight-facet boundary complex.
        for mask in range(256):
            chain = {f: orientation[f] for i, f in enumerate(FACETS) if mask & (1 << i)}
            with self.subTest(mask=mask):
                self.assertEqual(boundary_chain(boundary_chain(chain)), {})

    def test_dual_adjacency_and_interface_orientation(self):
        interfaces = facet_interfaces()
        self.assertEqual(len(interfaces), 24)
        self.assertEqual({i.face for i in interfaces}, set(cells(2)))
        self.assertEqual(len({(i.left, i.right) for i in interfaces}), 24)
        for interface in interfaces:
            self.assertLess(FACETS.index(interface.left), FACETS.index(interface.right))
            self.assertIn(interface.face, boundary(interface.left))
            self.assertIn(interface.face, boundary(interface.right))
            self.assertEqual(interface.left_sign, -interface.right_sign)
        for left, right in combinations(FACETS, 2):
            fixed_left = next(i for i, c in enumerate(left) if c != "*")
            fixed_right = next(i for i, c in enumerate(right) if c != "*")
            # Distinct-axis facets meet; opposite facets do not.
            self.assertEqual(right in facet_neighbors(left), fixed_left != fixed_right)
            self.assertEqual(left in facet_neighbors(right), fixed_left != fixed_right)
        self.assertTrue(all(len(facet_neighbors(f)) == 6 for f in FACETS))

    def test_fractional_chains_are_exact_and_do_not_alias_results(self):
        chain = {"*000": Fraction(1, 3)}
        self.assertEqual(boundary_chain(chain), {"0000": Fraction(-1, 3), "1000": Fraction(1, 3)})
        first = boundary("*000")
        first["0000"] = 999
        self.assertEqual(boundary("*000")["0000"], -1)
        self.assertEqual(chain, {"*000": Fraction(1, 3)})

    def test_invalid_inputs_fail_explicitly(self):
        for value in (None, 3, "", "***", "*****", "a***", "0 1*"):
            with self.subTest(cell=value), self.assertRaises(ValueError):
                boundary(value)
        for value in (-1, 5, True, 2.0, "2"):
            with self.subTest(dimension=value), self.assertRaises(ValueError):
                cells(value)
        for value in (True, 1.0, "1/2", None):
            with self.subTest(coefficient=value), self.assertRaises(TypeError):
                boundary_chain({"*000": value})
        with self.assertRaises(TypeError):
            boundary_chain([("*000", 1)])
        with self.assertRaises(ValueError):
            facet_neighbors("****")


if __name__ == "__main__":
    unittest.main()
