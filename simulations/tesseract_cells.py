"""Exact oriented cells of a declared combinatorial four-cube.

A cell ID is a four-character word in ``0``, ``1`` and ``*``. Fixed bits
select endpoints; stars select interval factors. The number of stars is the
cell's dimension. Orient free axes in ascending coordinate order and use the
product boundary d(I) = [1] - [0]. This construction declares the whole cubical
cell complex; the Q4 vertex/edge graph alone would not supply its filled faces.

No edge lengths, Euclidean embedding, physical clock or transition law follows
from this incidence data. A downstream jitter model can change declared weights
without silently changing these cell identities or boundary incidences.
"""
from __future__ import annotations

from collections.abc import Mapping
from dataclasses import dataclass
from fractions import Fraction
from itertools import product


TESSERACT = "****"
AMBIENT_DIMENSION = 4
FACETS = ("0***", "1***", "*0**", "*1**", "**0*", "**1*", "***0", "***1")
ExactCoefficient = int | Fraction


def _validate_cell(cell: str) -> None:
    if not isinstance(cell, str) or len(cell) != AMBIENT_DIMENSION:
        raise ValueError("a tesseract cell ID must be a four-character string")
    if any(character not in "01*" for character in cell):
        raise ValueError("a tesseract cell ID uses only 0, 1 and *")


def dimension(cell: str) -> int:
    """Return topological cell dimension, independent of any embedding."""
    _validate_cell(cell)
    return cell.count("*")


def cells(cell_dimension: int) -> tuple[str, ...]:
    """Return all cells of one dimension in stable 0/1/* product order."""
    if type(cell_dimension) is not int or not 0 <= cell_dimension <= AMBIENT_DIMENSION:
        raise ValueError("cell dimension must be an integer from 0 through 4")
    return tuple("".join(word) for word in product("01*", repeat=AMBIENT_DIMENSION)
                 if word.count("*") == cell_dimension)


def boundary(cell: str) -> dict[str, int]:
    """Return canonical oriented codimension-one incidence coefficients.

    For free axes a_0 < ... < a_(k-1), the contribution at axis a_r is
    (-1)^r (upper face - lower face). Vertices have an empty boundary.
    The returned dictionary is fresh and may be owned by the caller.
    """
    _validate_cell(cell)
    result = {}
    free_axes = [axis for axis, value in enumerate(cell) if value == "*"]
    for position, axis in enumerate(free_axes):
        sign = 1 if position % 2 == 0 else -1
        result[cell[:axis] + "0" + cell[axis + 1:]] = -sign
        result[cell[:axis] + "1" + cell[axis + 1:]] = sign
    return result


def boundary_chain(chain: Mapping[str, ExactCoefficient]) -> dict[str, ExactCoefficient]:
    """Sum a finite exact chain boundary, retaining the caller's input.

    Zero totals disappear from this projection only. The input mapping is never
    modified, so a caller can retain the original oriented contributions even
    when their internal faces cancel. Mixed dimensions are allowed as a direct
    sum of chain groups. Floating-point and Boolean coefficients are rejected.
    """
    if not isinstance(chain, Mapping):
        raise TypeError("a chain must map cell IDs to exact coefficients")
    totals = {}
    for cell, coefficient in chain.items():
        _validate_cell(cell)
        if isinstance(coefficient, bool) or not isinstance(coefficient, (int, Fraction)):
            raise TypeError("chain coefficients must be exact integers or fractions")
        for face, sign in boundary(cell).items():
            totals[face] = totals.get(face, 0) + coefficient * sign
    return {face: totals[face] for face in sorted(totals) if totals[face] != 0}


@dataclass(frozen=True, slots=True)
class Interface:
    """A square shared by two outward-oriented cubic boundary facets.

    ``left`` precedes ``right`` in FACETS. The signs are incidence coefficients
    after inducing each facet's orientation from the four-cell's boundary.
    They therefore oppose. A transport model may separately choose left-to-right
    as its positive flow convention; that choice is not supplied by these signs.
    """

    face: str
    left: str
    right: str
    left_sign: int
    right_sign: int


def facet_interfaces() -> tuple[Interface, ...]:
    """Return the 24 shared square interfaces of the eight boundary cubes."""
    outer_orientation = boundary(TESSERACT)
    owners = {face: [] for face in cells(2)}
    for facet in FACETS:
        for face, sign in boundary(facet).items():
            owners[face].append((facet, outer_orientation[facet] * sign))
    result = []
    for face, incidents in owners.items():
        if len(incidents) != 2:
            raise RuntimeError("a boundary square must have exactly two cubic owners")
        (left, left_sign), (right, right_sign) = incidents
        if left_sign + right_sign != 0:
            raise RuntimeError("induced shared-square orientations must cancel")
        result.append(Interface(face, left, right, left_sign, right_sign))
    return tuple(result)


def facet_neighbors(facet: str) -> tuple[str, ...]:
    """Return the six square-sharing neighbors of a cubic boundary facet."""
    if facet not in FACETS:
        raise ValueError("facet must be one of the eight cubic boundary cells")
    neighbors = set()
    for interface in facet_interfaces():
        if interface.left == facet:
            neighbors.add(interface.right)
        elif interface.right == facet:
            neighbors.add(interface.left)
    return tuple(candidate for candidate in FACETS if candidate in neighbors)
