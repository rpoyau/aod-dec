"""Exact, source-bound SPARC radial inputs with comparison targets separated.

The retained eight-column tables supply radial component proxies and surface
brightness. They do not supply a three-dimensional initial condition, a native
octave span, a time window, or a field evolution law.
"""

from dataclasses import dataclass
from decimal import Decimal, localcontext
from fractions import Fraction
import hashlib
from pathlib import Path
import re
from types import MappingProxyType
from typing import Mapping


COLUMNS = ("Rad", "Vobs", "errV", "Vgas", "Vdisk", "Vbul", "SBdisk", "SBbul")
UNITS = ("kpc", "km/s", "km/s", "km/s", "km/s", "km/s", "L/pc^2", "L/pc^2")
_DECIMAL = re.compile(r"[+-]?(?:[0-9]+(?:\.[0-9]*)?|\.[0-9]+)(?:[eE][+-]?[0-9]+)?\Z")
_DISTANCE = re.compile(r"Distance\s*=\s*(\S+)\s+Mpc\Z")


@dataclass(frozen=True, order=True)
class BinKey:
    galaxy: str
    radius_kpc: Fraction


@dataclass(frozen=True)
class PreparationRow:
    key: BinKey
    vgas_km_s: Fraction
    vdisk_km_s: Fraction
    vbul_km_s: Fraction
    sbdisk_lsun_pc2: Fraction
    sbbul_lsun_pc2: Fraction
    distance_mpc: Fraction


@dataclass(frozen=True)
class TargetRow:
    key: BinKey
    vobs_km_s: Fraction
    errv_km_s: Fraction

    @property
    def u_obs_km2_s2(self) -> Fraction:
        return self.vobs_km_s ** 2

    @property
    def sigma_u_linear_km2_s2(self) -> Fraction:
        """Observation-only first-order propagation; not complete uncertainty."""
        return 2 * self.vobs_km_s * self.errv_km_s


@dataclass(frozen=True)
class SourceFile:
    path: str
    sha256: str
    galaxy: str
    distance_mpc: Fraction
    rows: tuple[tuple[BinKey, int], ...]


@dataclass(frozen=True)
class SparcInputs:
    preparation: Mapping[BinKey, PreparationRow]
    targets: Mapping[BinKey, TargetRow]
    sources: tuple[SourceFile, ...]

    def __post_init__(self) -> None:
        # Own copies prevent a caller retaining a mutable backing dictionary.
        object.__setattr__(self, "preparation", MappingProxyType(dict(self.preparation)))
        object.__setattr__(self, "targets", MappingProxyType(dict(self.targets)))
        object.__setattr__(self, "sources", tuple(self.sources))
        if not self.preparation or self.preparation.keys() != self.targets.keys():
            raise ValueError("preparation and targets require identical nonempty bin identities")
        if any(key != row.key for key, row in self.preparation.items()):
            raise ValueError("preparation key does not match its row")
        if any(key != row.key for key, row in self.targets.items()):
            raise ValueError("target key does not match its row")


def _number(token: str, context: str) -> Fraction:
    if not _DECIMAL.fullmatch(token):
        raise ValueError(f"{context}: expected finite decimal, got {token!r}")
    return Fraction(token)


def load_inputs(directory: str | Path) -> SparcInputs:
    """Read all immediate ``*_rotmod.dat`` files, preserving exact decimals.

    Required headers bind column order, units, and assumed distance. Source
    hashes bind the bytes parsed in this call. Preparation intentionally omits
    observation targets and full-file hashes: changing only targets changes
    the source binding but cannot change the preparation values.
    """
    directory = Path(directory)
    if not directory.is_dir():
        raise ValueError(f"SPARC input directory does not exist: {directory}")
    paths = sorted(directory.glob("*_rotmod.dat"))
    if not paths:
        raise ValueError(f"no SPARC *_rotmod.dat files in {directory}")
    preparation: dict[BinKey, PreparationRow] = {}
    targets: dict[BinKey, TargetRow] = {}
    sources = []
    for path in paths:
        galaxy = path.name.removesuffix("_rotmod.dat")
        if not galaxy or not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9_.+-]*", galaxy):
            raise ValueError(f"invalid galaxy identity: {path.name}")
        content = path.read_bytes()
        try:
            lines = content.decode("utf-8").splitlines()
        except UnicodeDecodeError as exc:
            raise ValueError(f"{path}: expected UTF-8 data") from exc
        distance = None
        columns_seen = units_seen = False
        previous_radius = None
        row_sources = []
        for line_number, raw_line in enumerate(lines, 1):
            line = raw_line.strip()
            context = f"{path}:{line_number}"
            if not line:
                continue
            if line.startswith("#"):
                comment = line[1:].strip()
                match = _DISTANCE.fullmatch(comment)
                if match:
                    if distance is not None or row_sources:
                        raise ValueError(f"{context}: duplicate or late distance header")
                    distance = _number(match.group(1), context)
                    if distance <= 0:
                        raise ValueError(f"{context}: distance must be positive")
                elif comment.startswith("Distance"):
                    raise ValueError(f"{context}: invalid distance header or units")
                elif comment.split() and comment.split()[0] == "Rad":
                    if columns_seen or row_sources or tuple(comment.split()) != COLUMNS:
                        raise ValueError(f"{context}: duplicate, late, or invalid column header")
                    columns_seen = True
                elif comment.split() and comment.split()[0] == "kpc":
                    if units_seen or row_sources or tuple(comment.split()) != UNITS:
                        raise ValueError(f"{context}: duplicate, late, or invalid unit header")
                    units_seen = True
                continue
            if distance is None or not columns_seen or not units_seen:
                raise ValueError(f"{context}: distance, column and unit headers required before rows")
            tokens = line.split()
            if len(tokens) != len(COLUMNS):
                raise ValueError(f"{context}: expected exactly eight columns")
            radius, vobs, errv, gas, disk, bulge, sbdisk, sbbul = (
                _number(token, f"{context} {name}") for name, token in zip(COLUMNS, tokens)
            )
            if radius <= 0 or vobs <= 0 or errv <= 0:
                raise ValueError(f"{context}: radius, Vobs and errV must be positive")
            if previous_radius is not None and radius <= previous_radius:
                raise ValueError(f"{context}: radii must be strictly ascending without duplicates")
            if sbdisk < 0 or sbbul < 0:
                raise ValueError(f"{context}: surface brightness must be nonnegative")
            key = BinKey(galaxy, radius)
            if key in preparation:
                raise ValueError(f"{context}: duplicate bin identity {key}")
            preparation[key] = PreparationRow(key, gas, disk, bulge, sbdisk, sbbul, distance)
            targets[key] = TargetRow(key, vobs, errv)
            row_sources.append((key, line_number))
            previous_radius = radius
        if not row_sources:
            raise ValueError(f"{path}: no input rows")
        sources.append(SourceFile(str(path.resolve()), hashlib.sha256(content).hexdigest(),
                                  galaxy, distance, tuple(row_sources)))
    return SparcInputs(preparation, targets, tuple(sources))


def _floor_log10(value: Fraction) -> int:
    """Exact integer n with 10**n <= value < 10**(n+1)."""
    if value <= 0:
        raise ValueError("logarithm requires a positive rational")
    n = len(str(value.numerator)) - len(str(value.denominator))
    while value < Fraction(10) ** n:
        n -= 1
    while value >= Fraction(10) ** (n + 1):
        n += 1
    return n


def physical_range(values: tuple[Fraction, ...], unit: str) -> dict:
    """Positive physical range; ratios/brackets exact, log display approximate.

    Scaling all values by any positive unit-conversion factor leaves the ratio,
    decade span, and bracket unchanged. No octal/metric bridge is inferred.
    """
    if not values or any(type(value) is not Fraction or value <= 0 for value in values):
        raise ValueError("physical_range requires nonempty positive Fraction values")
    if not isinstance(unit, str) or not unit:
        raise ValueError("physical_range requires an explicit unit")
    lower, upper = min(values), max(values)
    ratio = upper / lower
    exponent = _floor_log10(ratio)
    with localcontext() as context:
        context.prec = 50
        log_span = (Decimal(ratio.numerator) / Decimal(ratio.denominator)).log10()
        minimum_display = Decimal(lower.numerator) / Decimal(lower.denominator)
        maximum_display = Decimal(upper.numerator) / Decimal(upper.denominator)
    return {
        "unit": unit, "minimum_exact": str(lower), "maximum_exact": str(upper),
        "minimum_display": str(minimum_display), "maximum_display": str(maximum_display),
        "max_over_min_exact": str(ratio), "log10_span_display": str(log_span),
        "log10_display_precision": 50,
        "decade_bracket": {"lower_exponent_inclusive": exponent,
                           "upper_exponent_exclusive": exponent + 1},
        "native_octave_span": None,
    }


def _signed_range(values: tuple[Fraction, ...], unit: str) -> dict:
    positive_magnitudes = tuple(abs(value) for value in values if value)
    return {
        "unit": unit, "minimum_exact": str(min(values)), "maximum_exact": str(max(values)),
        "negative_count": sum(value < 0 for value in values),
        "zero_count": sum(value == 0 for value in values),
        "nonzero_absolute_range": physical_range(positive_magnitudes, unit)
        if positive_magnitudes else None,
    }


def scale_inventory(data: SparcInputs) -> dict:
    """Report radial/velocity/proxy scales without preparing a field model.

    This diagnostic includes target ranges; it must not be passed to evolution
    as preparation. Consumers use ``data.preparation`` for that boundary.
    """
    galaxies = []
    for source in data.sources:
        keys = tuple(key for key, _line in source.rows)
        prep = tuple(data.preparation[key] for key in keys)
        targets = tuple(data.targets[key] for key in keys)
        radii = tuple(key.radius_kpc for key in keys)
        columns = {
            "Vobs": _signed_range(tuple(row.vobs_km_s for row in targets), "km/s"),
            "errV": _signed_range(tuple(row.errv_km_s for row in targets), "km/s"),
            "Vgas": _signed_range(tuple(row.vgas_km_s for row in prep), "km/s"),
            "Vdisk": _signed_range(tuple(row.vdisk_km_s for row in prep), "km/s"),
            "Vbul": _signed_range(tuple(row.vbul_km_s for row in prep), "km/s"),
            "SBdisk": _signed_range(tuple(row.sbdisk_lsun_pc2 for row in prep), "L/pc^2"),
            "SBbul": _signed_range(tuple(row.sbbul_lsun_pc2 for row in prep), "L/pc^2"),
        }
        gaps = tuple(right - left for left, right in zip(radii, radii[1:]))
        galaxies.append({
            "galaxy": source.galaxy, "bin_count": len(keys),
            "source_path": source.path, "source_sha256": source.sha256,
            "assumed_distance_mpc_exact": str(source.distance_mpc),
            "radius": physical_range(radii, "kpc"),
            "radial_sampling_gap": physical_range(gaps, "kpc") if gaps else None,
            "columns": columns,
            "comparison_only": {
                "U_obs": physical_range(tuple(row.u_obs_km2_s2 for row in targets), "(km/s)^2"),
                "sigma_U_first_order": physical_range(
                    tuple(row.sigma_u_linear_km2_s2 for row in targets), "(km/s)^2"),
                "errV_over_Vobs": physical_range(
                    tuple(row.errv_km_s / row.vobs_km_s for row in targets), "dimensionless"),
            },
        })
    return {
        "schema": "aod.sparc.physical_input_inventory.v1",
        "support_class": "G0 radial component proxies",
        "bin_count": len(data.preparation), "galaxies": galaxies,
        "native_octave_span": None,
        "native_metric_bridge": None,
        "observed_radial_extent_is_not_a_model_boundary": True,
        "comparison_targets_are_not_evolution_inputs": True,
    }
