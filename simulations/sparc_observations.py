"""Optional, source-bound HALOGAS observation adapter for projected inverse fits.

This module loads observed HI moment maps. Its two sky axes and LOS velocity
are not three spatial coordinates. A WCS reference defines the coordinate
origin, never an asserted galaxy centre. NumPy/Astropy are optional application
dependencies and are imported only when observations are loaded.
"""
from __future__ import annotations

from dataclasses import dataclass
from fractions import Fraction
import hashlib
import io
import json
import math
from pathlib import Path
from typing import Any
import warnings


@dataclass(frozen=True)
class Observations:
    galaxy: str
    east_arcsec: Any
    north_arcsec: Any
    intensity: Any
    velocity_km_s: Any
    intensity_valid: Any
    velocity_valid: Any
    joint_positive_mask: Any
    metadata: dict


def _dependencies():
    try:
        import numpy as np
        from astropy.io import fits
        from astropy.wcs import WCS
        from astropy.wcs.utils import proj_plane_pixel_area, wcs_to_celestial_frame
        from astropy.coordinates import SkyCoord
        import astropy.units as units
    except ImportError as exc:
        raise RuntimeError("HALOGAS fitting requires optional NumPy and Astropy") from exc
    return np, fits, WCS, proj_plane_pixel_area, wcs_to_celestial_frame, SkyCoord, units


def _positive_exact(value, name):
    try:
        exact = Fraction(value)
    except (ValueError, TypeError, ZeroDivisionError, OverflowError) as exc:
        raise ValueError(f"invalid catalog {name}") from exc
    if isinstance(value, bool) or exact <= 0:
        raise ValueError(f"catalog {name} must be positive")
    return str(exact)


def _catalog(path, galaxy):
    content = path.read_bytes()
    document = json.loads(content)
    rows = [row for row in document["cohort"] if row["galaxy"] == galaxy]
    if len(rows) != 1:
        raise ValueError(f"expected one catalog context row for {galaxy}")
    row = rows[0]
    geometry, catalog = row["input_geometry"], row["catalog"]
    if geometry["galaxy"] != galaxy:
        raise ValueError("catalog geometry galaxy mismatch")
    inclination = Fraction(geometry["inclination_deg"])
    if isinstance(geometry["inclination_deg"], bool) or not 0 < inclination < 90:
        raise ValueError("inclination must lie strictly between 0 and 90 degrees")
    return {
        "distance_mpc_exact": _positive_exact(geometry["distance_mpc"], "distance"),
        "distance_error_mpc_exact": _positive_exact(catalog["distance_error_mpc"], "distance error"),
        "inclination_deg_exact": str(inclination),
        "inclination_error_deg_exact": _positive_exact(catalog["inclination_error_deg"], "inclination error"),
        "catalog_hi_mass_1e9_msun_exact": _positive_exact(catalog["hi_mass_1e9_msun"], "HI mass"),
        "catalog_row_source": geometry["source_reference"],
        "catalog_context_path": str(path.resolve()),
        "catalog_context_sha256": hashlib.sha256(content).hexdigest(),
        "catalog_source_sha256": document["catalog_sha256"],
        "catalog_mass_is_not_independent_by_default": True,
        "shared_source_flags": [r for r in document.get("shared_source_flags", []) if r["galaxy"] == galaxy],
    }


def load_halogas_observations(data_directory: str | Path, galaxy: str,
                              catalog_context_path: str | Path) -> Observations:
    """Load full-resolution moment-0/1 without clipping, filling or resampling.

    ``intensity`` retains Jy/beam.km/s, ``velocity_km_s`` retains source km/s.
    ``joint_positive_mask`` is an explicit fit-selection convenience: finite
    velocity AND finite positive intensity. It is not a physical-support claim.
    Negative intensity, finite zeros and missing pixels remain in the arrays.

    Sky offsets use an exact spherical rotation about the WCS reference. A
    downstream small-angle distance map must declare its own approximation.
    ``initial_center_arcsec`` and ``initial_pa_deg_east_of_north`` are intensity
    moment seeds for fitting, not measured dynamical centre/orientation.
    """
    if galaxy not in {"NGC2403", "NGC3198"}:
        raise ValueError("selected HALOGAS acquisition contains NGC2403 and NGC3198")
    np, fits, WCS, pixel_area, celestial_frame, SkyCoord, units = _dependencies()
    directory, context = Path(data_directory), Path(catalog_context_path)
    metadata = _catalog(context, galaxy)
    products = []
    all_warnings = []
    for suffix, expected in (("mom0m", "JY/BEAM.KM/S"), ("mom1m", "KM/S")):
        path = directory / f"{galaxy}-LR_{suffix}.fits"
        content = path.read_bytes()
        with warnings.catch_warnings(record=True) as caught:
            warnings.simplefilter("always")
            with fits.open(io.BytesIO(content), memmap=False) as hdus:
                hdus.verify("exception")
                header = hdus[0].header.copy()
                data = np.array(hdus[0].data, dtype=float, copy=True)
            if data.ndim != 2 or data.size == 0:
                raise ValueError("moment maps must be nonempty two-dimensional images")
            if np.isinf(data).any():
                raise ValueError("infinite moment-map samples are not admissible")
            if str(header.get("BUNIT", "")).replace(" ", "").upper() != expected:
                raise ValueError(f"unexpected source unit for {suffix}")
            object_name = str(header.get("OBJECT", "")).replace(" ", "").upper()
            if object_name != galaxy:
                raise ValueError("FITS object identity does not match requested galaxy")
            wcs = WCS(header).celestial
        all_warnings.extend(str(item.message) for item in caught)
        products.append((data, header, wcs, {
            "path": str(path.resolve()), "sha256": hashlib.sha256(content).hexdigest(),
            "bytes": len(content), "raw_bunit": header["BUNIT"],
            "raw_date_obs": header.get("DATE-OBS"),
            "raw_specsys": header.get("SPECSYS"),
        }))
    intensity, header, wcs, first_source = products[0]
    velocity, velocity_header, velocity_wcs, second_source = products[1]
    if intensity.shape != velocity.shape:
        raise ValueError("moment map shapes differ")
    if wcs.to_header(relax=True) != velocity_wcs.to_header(relax=True):
        raise ValueError("moment map celestial WCS differs")
    for key in ("BMAJ", "BMIN", "BPA", "SPECSYS"):
        if header.get(key) != velocity_header.get(key):
            raise ValueError(f"moment map {key} differs")
    if header.get("SPECSYS") != "BARYCENT":
        raise ValueError("selected HALOGAS source must declare BARYCENT")
    if wcs.pixel_n_dim != 2 or wcs.wcs.lngtyp.strip() != "RA":
        raise ValueError("expected RA/Dec celestial WCS")
    # CRVAL supplies a coordinate reference only. The data centroid below is
    # independently computed and explicitly labelled an optimizer seed.
    origin_ra, origin_dec = map(float, wcs.wcs.crval)
    yy, xx = np.indices(intensity.shape, dtype=float)
    ra, dec = wcs.all_pix2world(xx, yy, 0)
    frame = celestial_frame(wcs)
    reference = SkyCoord(origin_ra * units.deg, origin_dec * units.deg, frame=frame)
    coordinates = SkyCoord(ra * units.deg, dec * units.deg, frame=frame)
    east, north = reference.spherical_offsets_to(coordinates)
    east, north = east.to_value(units.arcsec), north.to_value(units.arcsec)
    if not np.isfinite(east).all() or not np.isfinite(north).all():
        raise ValueError("nonfinite sky coordinate in image footprint")
    intensity_valid, velocity_valid = np.isfinite(intensity), np.isfinite(velocity)
    selected = intensity_valid & velocity_valid & (intensity > 0)
    if not selected.any():
        raise ValueError("no joint positive-intensity/finite-velocity samples")
    weights = intensity[selected]
    weight = float(weights.sum())
    center_e = float(np.sum(east[selected] * weights) / weight)
    center_n = float(np.sum(north[selected] * weights) / weight)
    centered = np.array([east[selected] - center_e, north[selected] - center_n])
    covariance = (centered * weights) @ centered.T / weight
    eigenvalues, eigenvectors = np.linalg.eigh(covariance)
    major_vector = eigenvectors[:, -1]
    pa = math.degrees(math.atan2(float(major_vector[0]), float(major_vector[1]))) % 180
    bmaj, bmin, bpa = (float(header[key]) for key in ("BMAJ", "BMIN", "BPA"))
    if not all(math.isfinite(x) for x in (bmaj, bmin, bpa)) or not bmaj >= bmin > 0:
        raise ValueError("invalid finite positive beam axes")
    major_arcsec, minor_arcsec = bmaj * 3600, bmin * 3600
    angle = math.radians(bpa)
    major = np.array([math.sin(angle), math.cos(angle)])
    minor = np.array([math.cos(angle), -math.sin(angle)])
    beam_cov = (np.outer(major, major) * major_arcsec**2 +
                np.outer(minor, minor) * minor_arcsec**2) / (8 * math.log(2))
    pixel_area_arcsec2 = float(pixel_area(wcs)) * 3600**2
    beam_area_arcsec2 = math.pi * major_arcsec * minor_arcsec / (4 * math.log(2))
    signed_flux = float(intensity[intensity_valid].sum()) * pixel_area_arcsec2 / beam_area_arcsec2
    metadata.update({
        "sources": [first_source, second_source],
        "shape": list(intensity.shape),
        "intensity_unit": "Jy/beam.km/s", "velocity_unit": "km/s",
        "spectral_frame_raw": header["SPECSYS"],
        "velocity_convention_note": "Moment-1 source unit retained; source cube has legacy VELO-HEL interpreted VOPT by WCSLIB. No frame transformation performed.",
        "coordinate_origin_ra_dec_deg": [origin_ra, origin_dec],
        "coordinate_origin_role": "FITS celestial WCS reference; not assumed galaxy centre",
        "coordinate_map": "spherical sky-offset longitude/latitude in arcsec, east/north",
        "celestial_frame": str(frame),
        "celestial_frame_name": frame.name,
        "celestial_wcs_header": dict(wcs.to_header(relax=True)),
        "initial_center_arcsec": [center_e, center_n],
        "initial_pa_deg_east_of_north": pa,
        "initial_shape_covariance_arcsec2": covariance.tolist(),
        "initial_geometry_role": "positive-intensity joint-mask moments; optimizer seeds only, 180-degree PA ambiguity retained",
        "beam_major_arcsec": major_arcsec, "beam_minor_arcsec": minor_arcsec,
        "beam_pa_deg_east_of_north": bpa,
        "beam_covariance_east_north_arcsec2": beam_cov.tolist(),
        "beam_area_arcsec2": beam_area_arcsec2,
        "projected_pixel_area_arcsec2": pixel_area_arcsec2,
        "integrated_signed_flux_jy_km_s": signed_flux,
        "flux_scope": "sum of finite source moment-0 values, including negatives and zeros; constant projected-plane pixel area/beam area; missing footprint not extrapolated",
        "flux_area_approximation": "projected-plane pixel solid-angle approximation, not exact curved-sky per-pixel area",
        "finite_intensity_count": int(intensity_valid.sum()),
        "finite_velocity_count": int(velocity_valid.sum()),
        "zero_intensity_count": int(np.count_nonzero(intensity_valid & (intensity == 0))),
        "negative_intensity_count": int(np.count_nonzero(intensity_valid & (intensity < 0))),
        "joint_positive_count": int(selected.sum()),
        "mask_scope": "source missingness preserved; joint positive selection is not an observed physical boundary",
        "column_density_maps_used": False,
        "column_density_note": "coldens BUNIT/HISTORY conflict is not resolved by this intensity-only adapter",
        "warnings": sorted(set(all_warnings)),
    })
    for array in (east, north, intensity, velocity, intensity_valid, velocity_valid, selected):
        array.flags.writeable = False
    return Observations(galaxy, east, north, intensity, velocity, intensity_valid,
                        velocity_valid, selected, metadata)
