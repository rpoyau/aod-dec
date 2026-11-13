"""Exact Manual observed-cycle fixture and separately typed physical targets.

This is a downstream observation-map example from the delivered Manual I
candidate. It does not turn cell-transport mass into microwave phase or provide
a physical hydrogen prediction. Exact rational encoding preserves printed
measurement tokens without asserting that the measured quantity is exact.
"""

from copy import deepcopy
from fractions import Fraction


CS_HZ = Fraction(9_192_631_770)
HYDROGEN_HZ = Fraction("1420405751.768")
HYDROGEN_REPORTED_U_HZ = Fraction("0.002")
MANUAL_COMMIT = "8c3149b9d3d924b80b9385b178da4ba2c39175e8"
MANUAL_TREE = "b2b55cbcf18354828f7b34418ab3fa26d747a927"
MANUAL_PATH = "manual/sections/03_m3_temporal.tex"
BIPM_URL = "https://www.bipm.org/en/si-base-units/second"
HYDROGEN_URL = "https://tf.nist.gov/general/pdf/13.pdf"


def _exact(value):
    if isinstance(value, bool) or not isinstance(value, (int, str, Fraction)):
        raise ValueError("an integer, rational string or Fraction is required")
    try:
        return Fraction(value)
    except (ValueError, ZeroDivisionError) as exc:
        raise ValueError("invalid exact rational") from exc


def _text(value, label):
    if not isinstance(value, str) or not value.strip():
        raise ValueError(label + " must be a nonempty string")


def phase_advance(record):
    """Evaluate signed net winding plus endpoint fractions in a typed window."""
    fields = {"id", "kind", "unit", "coordinate", "epoch", "window",
              "orientation", "unwrapping", "winding", "phase_start", "phase_end"}
    if not isinstance(record, dict) or set(record) != fields:
        raise ValueError("complete observed-phase record required")
    for name in ("id", "coordinate", "epoch"):
        _text(record[name], name)
    if record["kind"] != "oscillator_phase_advance" or record["unit"] != "turn":
        raise ValueError("oscillator phase in turns required; mass/events are not turns")
    if record["orientation"] not in ("forward", "reverse"):
        raise ValueError("phase orientation required")
    if record["unwrapping"] != "signed_net_winding":
        raise ValueError("signed net winding convention required")
    window = record["window"]
    if (not isinstance(window, (list, tuple)) or len(window) != 2
            or any(type(x) is not int for x in window) or window[0] >= window[1]):
        raise ValueError("increasing integer observation-window endpoints required")
    if type(record["winding"]) is not int:
        raise ValueError("net winding must be an integer")
    start, end = _exact(record["phase_start"]), _exact(record["phase_end"])
    if not (0 <= start < 1 and 0 <= end < 1):
        raise ValueError("phase fractions must lie in [0,1)")
    return record["winding"] + end - start


def conditional_prediction(compared, reference, realization):
    """Apply the Manual's conditional cycle-equivalent SI map, without fitting."""
    advance, reference_advance = phase_advance(compared), phase_advance(reference)
    for name in ("coordinate", "epoch", "window", "orientation"):
        if compared[name] != reference[name]:
            raise ValueError("compared/reference " + name + " must match")
    expected_fields = {"kind", "reference_id", "physical_correspondence", "source_anchor"}
    if not isinstance(realization, dict) or set(realization) != expected_fields:
        raise ValueError("separate reference-realization declaration required")
    if (realization["kind"] != "Cs133_equivalent_reference"
            or realization["reference_id"] != reference["id"]
            or realization["physical_correspondence"] != "declared_fixture"):
        raise ValueError("this finite fixture requires its declared Cs-equivalent reference")
    if realization["source_anchor"] != "eq:manual-m3-accumulated-si":
        raise ValueError("reference-realization fixture source anchor differs")
    if reference_advance <= 0 or reference["orientation"] != "forward":
        raise ValueError("positive forward reference phase advance required")
    return {
        "evidence_class": "conditional_exact_mathematical_fixture",
        "compared_record": deepcopy(compared),
        "reference_record": deepcopy(reference),
        "reference_realization": deepcopy(realization),
        "compared_advance_turns": advance,
        "reference_advance_turns": reference_advance,
        "ratio": advance / reference_advance,
        "duration_seconds": reference_advance / CS_HZ,
        "mean_frequency_hz": CS_HZ * advance / reference_advance,
        "physical_correspondence": "unestablished",
        "complete_physical_uncertainty": None,
        "universal_seconds_per_cut": False,
        "independent_atomic_prediction": False,
    }


def _phase(identity, window, winding, start, end):
    return {"id": identity, "kind": "oscillator_phase_advance", "unit": "turn",
            "coordinate": "manual-observer-turn-coordinate", "epoch": "fixture-origin",
            "window": list(window), "orientation": "forward", "unwrapping": "signed_net_winding",
            "winding": winding, "phase_start": start, "phase_end": end}


def target_records(hydrogen_target_override=None):
    """Bind actual source targets, or an explicitly synthetic target-only control."""
    value = HYDROGEN_HZ if hydrogen_target_override is None else _exact(hydrogen_target_override)
    if value <= 0:
        raise ValueError("positive hydrogen frequency target required")
    synthetic = hydrogen_target_override is not None
    return {
        "caesium_definition": {
            "kind": "exact_defining_frequency", "isotope": "Cs133", "unit": "Hz",
            "value": CS_HZ, "role": "unit_definition", "source_url": BIPM_URL,
            "definition_uncertainty_hz": Fraction(0), "realization_uncertainty_hz": None,
            "realization_status": "not_supplied",
        },
        "hydrogen": {
            "kind": "unperturbed_ground_state_hyperfine_frequency", "isotope": "H1",
            "unit": "Hz", "value": value,
            "role": "synthetic_target_control" if synthetic else "measured_comparison_target",
            "source_status": "modified_control" if synthetic else "bound_published_decimal",
            "source_url": HYDROGEN_URL, "source_locator": "Hellwig et al. (1970), pp. 200 and 207",
            "source_pdf_sha256": "e8bba435fbb9e6b54f41fa1d60f9c02df8e06b6a73ce8fab323afb3ea7d02dc4",
            "published_value_hz": HYDROGEN_HZ,
            "value_representation": "exact rational encoding of the printed decimal; measurement is not exact",
            "reported_standard_uncertainty": {
                "value": HYDROGEN_REPORTED_U_HZ, "unit": "Hz", "coverage": "one_sigma",
                "role": "published_source_summary", "finite_distribution": None,
                "applies_to": "published_value_hz",
            },
            "shared_reference": "Both reported evaluations share NBS-III caesium reference",
            "prior_target_exposure": True, "complete_physical_uncertainty": None,
        },
    }


def hydrogen_reduction_fixture():
    """Replay printed correction arithmetic, without inventing raw observations.

    Table VI's corrected beat already contains the reference-offset and wall
    corrections. They are retained as input ownership, not added again or
    represented as physically zero. All uncertainty values remain source
    summaries rather than an inferred joint law.
    """
    specifications = (
        ("table_iv", "Table IV, printed p. 205", "SAO portable hydrogen maser",
         [("synthesizer", "405794.4200"), ("signed_beat", "-0.1193"),
          ("UTC_to_atomic_time", "-42.6115"), ("NP3_reference_offset", "-0.0015"),
          ("wall", "0.0202"), ("second_order_Doppler", "0.0627"), ("Zeeman", "-0.0015")],
         [], "0.0024"),
        ("table_vi", "Table VI, printed p. 207", "NBS-H2 hydrogen maser",
         [("synthesizer", "405795.0000"), ("NP3_corrected_zero_wall_beat", "-0.6798"),
          ("UTC_to_atomic_time", "-42.6115"), ("second_order_Doppler", "0.0581"),
          ("Zeeman", "-0.0001")],
         ["NP3_reference_offset", "wall"], "0.0018"),
    )
    tables = {}
    reference_hz, multiplier = Fraction(5_000_000), 284
    for identity, locator, apparatus, tokens, already_in_input, uncertainty in specifications:
        terms = [{"name": name, "printed_value_hz": token, "value_hz": Fraction(token)}
                 for name, token in tokens]
        frequency = multiplier * reference_hz + sum(term["value_hz"] for term in terms)
        tables[identity] = {
            "source_locator": locator, "apparatus": apparatus,
            "nominal_reference_hz": reference_hz, "reference_multiplier": multiplier,
            "printed_added_terms": terms, "corrected_frequency_hz": frequency,
            "corrections_already_in_input": already_in_input,
            "already_in_input_owner": "NP3_corrected_zero_wall_beat" if already_in_input else None,
            "reported_standard_uncertainty_hz": Fraction(uncertainty),
            "raw_observations_available": False,
        }
    mean = (tables["table_iv"]["corrected_frequency_hz"]
            + tables["table_vi"]["corrected_frequency_hz"]) / 2
    return {
        "evidence_class": "measured_input_reduction",
        "calculation_scope": "exact arithmetic on published decimal correction terms and corrected table outputs",
        "source": {"url": HYDROGEN_URL,
                   "pdf_sha256": "e8bba435fbb9e6b54f41fa1d60f9c02df8e06b6a73ce8fab323afb3ea7d02dc4",
                   "manual_anchor": "eq:manual-m3-hydrogen-reduction"},
        "source_equation": "nu_H = 284*nu_R + nu_S + nu_B + delta_UTC + delta_N + delta_W + delta_D + delta_Z",
        "tables": tables,
        "printed_input_mean_hz": mean,
        "published_rounded_mean_hz": HYDROGEN_HZ,
        "published_minus_printed_mean_hz": HYDROGEN_HZ - mean,
        "display_difference_class": "source_reporting_precision",
        "reported_mean_uncertainty": {
            "value_hz": HYDROGEN_REPORTED_U_HZ, "category": "published_standard_uncertainty",
            "coverage": "one_sigma", "source_locator": "Printed pp. 200 and 207",
            "finite_distribution": None, "recomputed_from_components": False,
        },
        "shared_reference": "Both evaluations use NBS-III; independence is not assumed",
        "missing_raw_inputs": ["unrounded wall-fit inputs", "individual tuning/flux observations",
                               "raw microwave phase stream", "complete joint covariance"],
        "complete_physical_uncertainty": None,
        "independent_atomic_prediction": False,
    }


def build_fixture(*, hydrogen_target_override=None):
    """Build the Manual fixture; comparison targets never enter its computation.

    The optional override is a labelled synthetic control for target-isolation
    tests. It cannot become an authenticated measured target or alter prediction.
    """
    compared = _phase("compared", (10, 22), 4, "2/3", "1/6")
    reference = _phase("reference", (10, 22), 8, "1/4", "3/4")
    realization = {"kind": "Cs133_equivalent_reference", "reference_id": "reference",
                   "physical_correspondence": "declared_fixture",
                   "source_anchor": "eq:manual-m3-accumulated-si"}
    prediction = conditional_prediction(compared, reference, realization)
    split = [_phase("compared-left", (10, 16), 2, "2/3", "5/6"),
             _phase("compared-right", (16, 22), 2, "5/6", "1/6")]
    advances = [phase_advance(part) for part in split]
    if sum(advances) != prediction["compared_advance_turns"]:
        raise ValueError("shared phase endpoint cancellation failed")
    return {
        "format": "aod-dec-manual-si-fixture", "version": 1,
        "source": {"repository": "rpoyau/aod", "commit": MANUAL_COMMIT, "tree": MANUAL_TREE,
                   "status": "delivered_detached_candidate", "path": MANUAL_PATH,
                   "anchors": ["eq:manual-m3-cycle-bridge", "eq:manual-m3-observed-advance",
                               "eq:manual-m3-accumulated-si", "eq:manual-m3-hydrogen-reduction"]},
        "prediction": prediction,
        "split_window": {"records": split, "advances_turns": advances,
                         "sum_turns": sum(advances), "shared_endpoint": Fraction(5, 6)},
        "targets": target_records(hydrogen_target_override),
        "hydrogen_reduction": hydrogen_reduction_fixture(),
        "transport_to_hydrogen": {
            "status": "missing_observation_law_and_calibration", "prediction_hz": None,
            "residual_hz": None, "complete_uncertainty_hz": None,
            "reason": "Cell-transport mass has no bound preparation/emission/phase observation map to the hydrogen apparatus.",
            "manual_fixture_scored_against_hydrogen": False,
        },
    }
