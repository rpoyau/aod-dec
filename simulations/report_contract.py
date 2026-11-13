"""Typed comparisons for the declared cell transport application.

Exact transport quantities, numerical reference diagnostics, integer-fixture
residuals, and SI target comparisons have separate records. These functions do
not supply native wave/RCD admission, physical calibration, or a probability law.
Manual binding: delivered local candidate 8c3149b9d3d924b80b9385b178da4ba2c39175e8;
its status is not promoted to an accepted upstream release.
"""
from __future__ import annotations

from copy import deepcopy
from decimal import Decimal, localcontext
from fractions import Fraction
import re

from simulations.cell_transport import STATE_NAMES, flatten_state
from simulations.transport_reference import flatten_reference


MANUAL_BINDING = {
    "commit": "8c3149b9d3d924b80b9385b178da4ba2c39175e8",
    "tree": "b2b55cbcf18354828f7b34418ab3fa26d747a927",
    "status": "delivered_local_candidate",
    "anchors": ["manual:calculation-spine", "manual:exact-arithmetic-presentation",
                "manual:scoped-errors-data-support", "manual:m3-mapping-error",
                "eq:manual-m3-cycle-bridge", "eq:manual-m3-residual-dependence"],
}
ERROR_COMPONENTS = ("arithmetic", "resolution", "window", "model_discrepancy",
                    "calibration", "propagation", "solver", "comparison")
MODEL_UNIT = "model_mass"
# This small adapter implements only the two SI report kinds used by the
# Manual's temporal example. Dimensions use its (length, time, count) basis.
SI_TYPES = {"Hz": ("frequency", (0, -1, 0)),
            "s": ("time_interval", (0, 1, 0))}


def _exact(value):
    if isinstance(value, bool) or not isinstance(value, (int, Fraction)):
        raise TypeError("exact integer or Fraction required; no float/decimal coercion")
    return Fraction(value)


def _text(value, name):
    if not isinstance(value, str) or not value.strip():
        raise ValueError(name + " must be a nonempty string")
    return value


def _decimal(value):
    if not isinstance(value, Decimal) or not value.is_finite():
        raise TypeError("finite Decimal numerical reference required")
    return value


def _precision(value):
    if type(value) is not int or value < 25:
        raise ValueError("diagnostic precision must be an integer of at least 25")


def _as_decimal(value):
    return Decimal(value.numerator) / Decimal(value.denominator)


def integer_delta3(predicted, comparator):
    """The Manual integer-fixture convention is Delta_Z = T_predicted - O."""
    if type(predicted) is not int or type(comparator) is not int:
        raise TypeError("delta3 requires integer comparator bins, excluding bool/Fraction")
    residual = predicted - comparator
    return {"predicted": predicted, "comparator": comparator, "signed_residual": residual,
            "ternary_sign": (residual > 0) - (residual < 0),
            "integer_magnitude": abs(residual), "unit": "declared_integer_bin"}


def transport_error_components(unit=MODEL_UNIT):
    """A conditional numerical study's per-output budget, not a physical budget.

    The observed refinement discrepancy is not inserted as a certified bound or
    a standard uncertainty. Applicable unestablished physical terms stay unknown.
    """
    _text(unit, "output unit")
    choices = {
        "arithmetic": ("exact", "Rational Euler state evaluation is exact."),
        "resolution": ("unknown", "Finite-step discrepancy is a numerical diagnostic; no uniform discretization bound is supplied."),
        "window": ("not_applicable", "The mathematical terminal window is fixed exactly; no uncertain acquisition window is instantiated."),
        "model_discrepancy": ("unknown", "No physical correspondence or discrepancy model is instantiated."),
        "calibration": ("unknown", "No physical observation or SI calibration map is instantiated."),
        "propagation": ("not_applicable", "This comparison conditions on one exact realized schedule; no uncertain-input law is propagated here."),
        "solver": ("unknown", "The reference Taylor tail excludes Decimal rounding and prior-interval error; precision agreement is not a full enclosure."),
        "comparison": ("not_applicable", "No measured evaluation target is joined in this conditional numerical comparison."),
    }
    result = {}
    for component in ERROR_COMPONENTS:
        kind, reason = choices[component]
        row = {"kind": kind, "unit": unit, "reason": reason, "included_sources": {}}
        if kind == "exact":
            row["value"] = Fraction(0)
        result[component] = row
    return result


def numerical_comparison(value, reference, *, name, quantity_kind,
                         reference_id, denominator=None, denominator_kind=None,
                         precision=80):
    """Compare an exact model coordinate with a finite numerical approximation.

    A relative diagnostic requires an explicitly named positive denominator.
    None denotes an unavailable relative diagnostic, never a substituted zero.
    """
    value, reference = _exact(value), _decimal(reference)
    for text, label in ((name, "name"), (quantity_kind, "quantity kind"),
                        (reference_id, "reference identity")):
        _text(text, label)
    _precision(precision)
    if denominator is not None:
        denominator = _decimal(denominator) if isinstance(denominator, Decimal) else _exact(denominator)
        if denominator <= 0:
            raise ValueError("relative diagnostic requires a positive declared denominator")
        _text(denominator_kind, "denominator kind")
    elif denominator_kind is not None:
        raise ValueError("denominator kind supplied without a denominator")
    with localcontext() as context:
        context.prec = precision
        signed = _as_decimal(value) - reference
        divisor = denominator if isinstance(denominator, Decimal) else (
            _as_decimal(denominator) if denominator is not None else None)
        fractional = signed / divisor if divisor is not None else None
        absolute = abs(signed)
    return {"name": name, "quantity_kind": quantity_kind, "unit": MODEL_UNIT,
            "exact_value": value, "reference_approximation": reference,
            "reference_id": reference_id, "signed_discrepancy": signed,
            "absolute_discrepancy": absolute, "fractional_discrepancy": fractional,
            "relative_denominator": None if denominator is None else {
                "kind": denominator_kind, "value": denominator, "unit": MODEL_UNIT},
            "fractional_unit": "1", "diagnostic_precision": precision,
            "relative_status": "unavailable" if denominator is None else "declared_diagnostic",
            "scope": "Finite-step versus numerical-reference discrepancy in one declared transport coordinate; not a bound, standard uncertainty, kernel error, path-history error, or physical accuracy.",
            "semantic_exclusions": ["normalized_AFC_memory", "field_population_density", "RCD"],
            "error_components": transport_error_components()}


def terminal_report(result, reference, *, reference_id, precision=80):
    """Report all terminal coordinates and retain the old aggregate by its law.

    The caller binds the reference's model inputs; this function additionally
    rejects unequal realized-schedule identities and wrong state dimensions.
    """
    _text(reference_id, "reference identity")
    _precision(precision)
    if result["schedule"]["sha256"] != reference["schedule_sha256"]:
        raise ValueError("terminal comparison requires the same realized jitter schedule")
    values, references = flatten_state(result), flatten_reference(reference)
    if len(values) != len(STATE_NAMES) or len(references) != len(STATE_NAMES):
        raise ValueError("terminal comparison requires all twelve declared coordinates")
    outputs = {}
    kinds = {"output": "cumulative_boundary_output", "loss": "cumulative_loss",
             "injected": "cumulative_injection"}
    for name, value, comparator in zip(STATE_NAMES, values, references):
        comparator = _decimal(comparator)
        # copy_abs preserves the represented reference without ambient-context rounding.
        divisor = comparator.copy_abs() if comparator != 0 else None
        outputs[name] = numerical_comparison(value, comparator, name=name,
            quantity_kind=kinds.get(name, "transport_storage"), reference_id=reference_id,
            denominator=divisor,
            denominator_kind="absolute_numerical_reference" if divisor is not None else None,
            precision=precision)
    supplied = _exact(result["initial_total"]) + _exact(result["final"]["injected"])
    if supplied < 0:
        raise ValueError("supplied transport mass must be nonnegative")
    with localcontext() as context:
        context.prec = precision
        l1 = sum((row["absolute_discrepancy"] for row in outputs.values()), Decimal(0))
        normalized = l1 / _as_decimal(supplied) if supplied > 0 else None
    return {"format": "aod-dec.terminal-comparison.v1", "outputs": outputs,
            "manual_binding": deepcopy(MANUAL_BINDING),
            "physical_observation_map": "uninstantiated",
            "aggregate": {"name": "supplied-mass-normalized terminal L1 diagnostic",
                "coordinate_names": STATE_NAMES, "l1_discrepancy": l1,
                "normalizer": {"exact_value": supplied, "unit": MODEL_UNIT,
                    "law": "initial_total + cumulative_injected_mass"},
                "normalized_l1_diagnostic": normalized, "unit": "1",
                "status": "declared_diagnostic" if supplied > 0 else "undefined_zero_normalizer",
                "law": "sum(abs(exact_terminal - numerical_reference)) / supplied_mass",
                "scope": "Historical aggregate display coordinate; not an individual output's relative error, kernel distance, probability, history fidelity, or physical accuracy."}}


def _source(record, name):
    _text(record.get("source_id"), name + " source identity")
    digest = record.get("source_sha256")
    if not isinstance(digest, str) or re.fullmatch(r"[0-9a-f]{64}", digest) is None:
        raise ValueError(name + " requires a SHA-256 source binding")


def _reject_evaluation(value, target_id):
    if isinstance(value, dict):
        if value.get("role") == "evaluation" or value.get("id") == target_id:
            raise ValueError("evaluation target cannot enter prediction or map dependencies")
        for item in value.values():
            _reject_evaluation(item, target_id)
    elif isinstance(value, (list, tuple)):
        for item in value:
            _reject_evaluation(item, target_id)


def si_comparison(prediction, target, *, mapping, include_percent=False):
    """Compare supplied, separately bound Hz/s quantities after an explicit map.

    Quantity fields: id, role, value (int/Fraction), unit, kind, dimension,
    source_id, source_sha256. Prediction also carries map_id and dependencies.
    Mapping fields: id, source_id, source_sha256, output_unit, output_kind,
    output_dimension, dependencies. Target may carry standard_uncertainty and
    uncertainty_category. Source authentication and physical map validation are
    external obligations; syntactic hash bindings do not establish either.
    """
    if not all(isinstance(row, dict) for row in (prediction, target, mapping)):
        raise ValueError("prediction, evaluation target, and explicit report map are required")
    if type(include_percent) is not bool:
        raise TypeError("include_percent must be a bool")
    map_id = _text(mapping.get("id"), "map identity")
    target_id = _text(target.get("id"), "target identity")
    _text(prediction.get("id"), "prediction identity")
    if prediction.get("role") != "prediction" or target.get("role") != "evaluation":
        raise ValueError("prediction and evaluation roles must be separate")
    if prediction.get("map_id") != map_id:
        raise ValueError("prediction does not bind the supplied observation map")
    for record, name in ((prediction, "prediction"), (target, "target"), (mapping, "map")):
        _source(record, name)
    for record in (prediction, mapping):
        if not isinstance(record.get("dependencies"), (list, tuple)):
            raise ValueError("prediction and map require an explicit dependency inventory")
        _reject_evaluation(record, target_id)
    unit = prediction.get("unit")
    if unit not in SI_TYPES:
        raise ValueError("this adapter supports only the declared Hz/s quantity kinds")
    kind, dimension = SI_TYPES[unit]
    for record in (prediction, target):
        if (record.get("unit") != unit or record.get("kind") != kind
                or record.get("dimension") not in (dimension, list(dimension))
                or any(type(x) is not int for x in record["dimension"])):
            raise ValueError("SI quantity unit, kind or dimension mismatch")
    if (mapping.get("output_unit") != unit or mapping.get("output_kind") != kind
            or mapping.get("output_dimension") not in (dimension, list(dimension))
            or any(type(x) is not int for x in mapping["output_dimension"])):
        raise ValueError("report map output type differs from compared quantity")
    model_value, target_value = _exact(prediction.get("value")), _exact(target.get("value"))
    if target_value <= 0:
        raise ValueError("fractional SI comparison requires a positive evaluation target")
    residual = model_value - target_value
    fractional = residual / target_value
    pull = {"status": "unavailable", "reason": "No positive target standard uncertainty was supplied."}
    uncertainty = target.get("standard_uncertainty")
    if uncertainty is not None:
        uncertainty = _exact(uncertainty)
        if uncertainty < 0:
            raise ValueError("standard uncertainty must be nonnegative")
        category = target.get("uncertainty_category")
        if category not in ("source_reported_standard_uncertainty", "declared_standard_uncertainty",
                            "exact_defining_constant"):
            raise ValueError("target uncertainty category must be explicit")
        if category == "exact_defining_constant" and uncertainty != 0:
            raise ValueError("an exact defining constant has zero defining uncertainty")
        if uncertainty > 0:
            pull = {"status": "target_only_diagnostic", "label": "z_target",
                    "value": residual / uncertainty, "unit": "1",
                    "scope": "Uses only the target's declared standard uncertainty; not a joint standardized residual or physical significance test."}
    report = {"format": "aod-dec.si-comparison.v1", "prediction": deepcopy(prediction),
              "target": deepcopy(target), "mapping": deepcopy(mapping),
              "signed_residual": residual, "residual_unit": unit,
              "fractional_residual": fractional, "fractional_unit": "1",
              "relative_denominator": {"value": target_value, "unit": unit, "id": target_id},
              "target_only_pull": pull,
              "joint_standardized_residual": {"status": "unavailable",
                  "reason": "No complete justified joint variance and covariance model is supplied by this comparison utility."},
              "scope": "Arithmetic comparison of supplied source-bound quantities; physical observation-map validity and source authentication remain separate.",
              "manual_binding": deepcopy(MANUAL_BINDING)}
    if include_percent:
        report["percent_display"] = {"value": 100 * fractional, "unit": "%",
                                     "law": "100 * fractional_residual", "role": "presentation_only"}
    return report
