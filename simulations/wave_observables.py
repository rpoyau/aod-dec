"""Explicit recurrence-to-SI observation map; never an intrinsic engine clock.

The native trace supplies ordered completion separations. A caller must supply
the time realization, phase cycles per beat, phase speed, scope and provenance
before a wavelength in metres can be formed. No target sets those operands.
"""
from dataclasses import dataclass
from fractions import Fraction


def _positive(value, name):
    if type(value) not in (int, Fraction) or value <= 0:
        raise ValueError(name + " requires a positive exact integer or Fraction")
    return Fraction(value)


@dataclass(frozen=True)
class WavelengthMap:
    seconds_per_native_index: Fraction
    phase_cycles_per_beat: Fraction
    phase_speed_m_s: Fraction
    native_scope: str
    provenance: str
    role: str

    def __post_init__(self):
        for name in ("seconds_per_native_index", "phase_cycles_per_beat", "phase_speed_m_s"):
            object.__setattr__(self, name, _positive(getattr(self, name), name))
        if any(type(v) is not str or not v.strip() for v in (self.native_scope, self.provenance)):
            raise ValueError("a native scope and provenance are required")
        if self.role not in ("declared_calibration", "synthetic_control"):
            raise ValueError("map role must distinguish declared calibration from synthetic control")


def wavelength_report(cadence, *, native_scope, observation_map=None):
    cadence = tuple(cadence)
    if type(native_scope) is not str or not native_scope.strip():
        raise ValueError("bound native scope required")
    if any(type(n) is not int or n <= 0 for n in cadence):
        raise ValueError("native cadence requires positive integer completion separations")
    if observation_map is None:
        return {"status": "NOT_INSTANTIATED", "native_scope": native_scope,
                "cadence_native_indices": cadence, "wavelength_m": None,
                "reason": "No time realization, phase cycles per beat and phase-speed map supplied.",
                "physical_proton_correspondence": "NOT_INSTANTIATED"}
    if type(observation_map) is not WavelengthMap:
        raise TypeError("typed WavelengthMap required")
    observation_map.__post_init__()
    if observation_map.native_scope != native_scope:
        raise ValueError("observation map belongs to another native scope")
    rows = []
    for index, separation in enumerate(cadence, 1):
        elapsed = separation * observation_map.seconds_per_native_index
        frequency = observation_map.phase_cycles_per_beat / elapsed
        wavelength = observation_map.phase_speed_m_s / frequency
        rows.append({"beat_index": index, "native_separation": separation,
                     "elapsed_s": elapsed, "phase_frequency_hz": frequency,
                     "wavelength_m": wavelength})
    return {"status": "CONDITIONAL_MAP", "native_scope": native_scope,
            "map": observation_map, "rows": rows,
            "law": "elapsed_s = native_separation * seconds_per_index; f = cycles_per_beat / elapsed_s; lambda = phase_speed / f",
            "coordinate_role": "Octal support coordinates and origin-qualified beat counts remain distinct; no frequency doubling is inferred.",
            "measured_target": None, "target_residual": None,
            "uncertainty": {"status": "unknown", "standard_uncertainty_m": None},
            "physical_proton_correspondence": "NOT_INSTANTIATED"}
