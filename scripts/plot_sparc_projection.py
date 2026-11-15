#!/usr/bin/env python3
"""Render source-bound HI projected-fit maps and conditional density profiles."""
from __future__ import annotations

import argparse
from fractions import Fraction
import hashlib
import json
from pathlib import Path


def sha256(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def plot_galaxy(directory, output):
    import numpy as np
    import matplotlib
    matplotlib.use("Agg")
    import matplotlib.pyplot as plt
    from matplotlib.colors import Normalize, TwoSlopeNorm, LogNorm

    report_path, arrays_path = directory / "results.json", directory / "reconstruction.npz"
    report = json.loads(report_path.read_text())
    if sha256(arrays_path) != report["arrays_sha256"]:
        raise ValueError("reconstruction array hash differs from results")
    arrays = np.load(arrays_path, allow_pickle=False)
    name = report["galaxy"]
    e, n = arrays["east_arcsec"], arrays["north_arcsec"]
    imask, vmask = arrays["intensity_fit_mask"], arrays["velocity_fit_mask"]
    geometry = report["selected_fit"]["geometry"]
    ce, cn = geometry["center_east_arcsec"], geometry["center_north_arcsec"]
    plt.rcParams.update({"font.size": 9, "axes.titlesize": 11, "axes.labelsize": 9,
                         "savefig.facecolor": "white", "font.family": "DejaVu Sans"})
    sequential = plt.get_cmap("viridis").copy()
    residual_cmap = plt.get_cmap("RdBu_r").copy()
    sequential.set_bad("#e1e4e8")
    residual_cmap.set_bad("#e1e4e8")
    mask_union = imask | vmask
    padding = report["observation_metadata"]["beam_major_arcsec"]
    extent = (float(e[mask_union].max()) + padding, float(e[mask_union].min()) - padding,
              float(n[mask_union].min()) - padding, float(n[mask_union].max()) + padding)
    figure, axes = plt.subplots(2, 3, figsize=(12.4, 8.2), constrained_layout=True)
    for row, (observed_key, fitted_key, mask, unit, label) in enumerate((
        ("observed_intensity", "fitted_intensity", imask, "Jy beam⁻¹ km s⁻¹", "Integrated HI intensity"),
        ("observed_velocity_km_s", "fitted_velocity_km_s", vmask, "km s⁻¹", "LOS mean velocity"),
    )):
        observed, fitted = arrays[observed_key], arrays[fitted_key]
        residual = fitted - observed
        low = min(float(observed[mask].min()), float(fitted[mask].min()))
        high = max(float(observed[mask].max()), float(fitted[mask].max()))
        normal = Normalize(low, high if high > low else low + 1)
        limit = max(float(np.max(np.abs(residual[mask]))), 1e-12)
        difference_normal = TwoSlopeNorm(vmin=-limit, vcenter=0, vmax=limit)
        for column, (values, title) in enumerate(((observed, "Observed"), (fitted, "Projected fit"),
                                                 (residual, "Fit − observed"))):
            ax = axes[row, column]
            picture = ax.pcolormesh(e, n, np.ma.array(values, mask=~mask), shading="nearest",
                                   cmap=sequential if column < 2 else residual_cmap,
                                   norm=normal if column < 2 else difference_normal, rasterized=True)
            ax.plot(ce, cn, marker="+", color="white" if column < 2 else "black", markersize=7,
                    markeredgewidth=.9)
            ax.set(xlim=extent[:2], ylim=extent[2:], aspect="equal",
                   xlabel="East offset (arcsec)", ylabel="North offset (arcsec)",
                   title=f"{label}\n{title}")
            figure.colorbar(picture, ax=ax, shrink=.78, label=unit, pad=.025)
    figure.suptitle(f"{name}: observed projection and fitted 3D HI distribution", fontsize=15)
    figure.supxlabel("Same source mask in each row; gray = excluded/unknown. White/black cross = fitted centre.\n"
                       "Offsets refer to the source WCS reference. These are fit residuals on reused observations.",
                       fontsize=9)
    paths = []
    for suffix in ("png", "svg", "pdf"):
        path = output / f"{name}-projection.{suffix}"
        figure.savefig(path, dpi=160)
        paths.append(path)
    plt.close(figure)

    figure, axes = plt.subplots(1, 2, figsize=(11.8, 4.4), constrained_layout=True)
    density = arrays["density_msun_kpc3"]
    positive = density > 0
    density_cmap = sequential.copy()
    density_cmap.set_bad("white")
    picture = axes[0].pcolormesh(arrays["radius_kpc"], arrays["z_kpc"],
                                np.ma.array(density, mask=~positive), shading="nearest", cmap=density_cmap,
                                norm=LogNorm(float(density[positive].min()), float(density[positive].max())),
                                rasterized=True)
    axes[0].set(xlabel="Cylindrical radius R (kpc)", ylabel="Vertical coordinate z (kpc)",
                title="Inferred axisymmetric HI density\nAxes scaled independently")
    figure.colorbar(picture, ax=axes[0], label="M☉ kpc⁻³", shrink=.85, pad=.025)
    rows = report["geometry_profile"]
    distance = float(Fraction(report["observation_metadata"]["distance_mpc_exact"]))
    kpc_per_arcsec = distance * 1000 * np.pi / (180 * 3600)
    for inclination in sorted({x["geometry"]["inclination_deg"] for x in rows}):
        selected = sorted((x for x in rows if x["geometry"]["inclination_deg"] == inclination),
                          key=lambda x: x["geometry"]["height_arcsec"])
        axes[1].plot([x["geometry"]["height_arcsec"]*kpc_per_arcsec for x in selected],
                     [x["score"] for x in selected], marker="o", markersize=4,
                     label=f"inclination {inclination:g}°")
    axes[1].scatter([geometry["height_arcsec"]*kpc_per_arcsec], [report["selected_fit"]["score"]],
                    marker="*", s=110, color="black", label="refined, higher-order fit")
    axes[1].set(xscale="log", xlabel="Exponential height h (kpc)", ylabel="Declared combined fit loss",
                title="Thickness / inclination alternatives")
    axes[1].grid(alpha=.2)
    axes[1].legend(frameon=False, fontsize=8)
    figure.suptitle(f"{name}: conditional 3D reconstruction", fontsize=14)
    boundary = report["boundary_accounting"]
    outside_fraction = boundary["outside_dense_radial_and_vertical_view_mass_msun"] / report["model"]["total_hi_mass_msun"]
    radius_limit = boundary["dense_radial_view_radius_kpc"]
    figure.supxlabel(f"Density view: R ≤ {radius_limit:.3g} kpc, |z| ≤ 6h; accounted outside mass fraction = {outside_fraction:.4g}; white = model zero.\n"
                       "Curves use seeded centre/PA and baseline quadrature; refitted alternatives are not confidence intervals.",
                       fontsize=9)
    for suffix in ("png", "svg", "pdf"):
        path = output / f"{name}-density-profile.{suffix}"
        figure.savefig(path, dpi=160)
        paths.append(path)
    plt.close(figure)
    arrays.close()
    return {"galaxy": name, "results_sha256": sha256(report_path), "arrays_sha256": sha256(arrays_path),
            "figures": [{"file": str(path), "sha256": sha256(path)} for path in paths],
            "mask": "same retained observation fit mask for data, model and residual",
            "normalization": "shared full-range observed/model limits per row; symmetric full residual limits",
            "density_scope": "conditional axisymmetric HI model only; catalog HI mass fixed; dense inner radial/vertical view with exterior mass retained",
            "height_profile_is_confidence_interval": False}


def plot_detail(directory, output):
    """Supplemental view; preserve observations/scores and full-frame figures."""
    import numpy as np
    import matplotlib
    matplotlib.use("Agg")
    import matplotlib.pyplot as plt
    from matplotlib.colors import Normalize, TwoSlopeNorm
    plt.rcParams.update({"font.size": 9, "axes.titlesize": 11, "axes.labelsize": 9,
                         "savefig.facecolor": "white", "font.family": "DejaVu Sans"})

    report_path, arrays_path = directory / "results.json", directory / "reconstruction.npz"
    report = json.loads(report_path.read_text())
    if sha256(arrays_path) != report["arrays_sha256"]:
        raise ValueError("reconstruction array hash differs from results")
    arrays = np.load(arrays_path, allow_pickle=False)
    e, n = arrays["east_arcsec"], arrays["north_arcsec"]
    name, g = report["galaxy"], report["selected_fit"]["geometry"]
    ce, cn = g["center_east_arcsec"], g["center_north_arcsec"]
    radius_fraction = .5
    radius = radius_fraction * report["model"]["concentration_radius_arcsec"]
    pa, inclination = np.deg2rad([g["pa_deg"], g["inclination_deg"]])
    padding = report["observation_metadata"]["beam_major_arcsec"]
    vertical = 6*g["height_arcsec"]*np.sin(inclination)
    half_e = np.hypot(radius*np.sin(pa), radius*np.cos(inclination)*np.cos(pa)) + vertical*abs(np.cos(pa)) + padding
    half_n = np.hypot(radius*np.cos(pa), radius*np.cos(inclination)*np.sin(pa)) + vertical*abs(np.sin(pa)) + padding
    extent = [float(ce+half_e), float(ce-half_e), float(cn-half_n), float(cn+half_n)]
    in_view = (e >= extent[1]) & (e <= extent[0]) & (n >= extent[2]) & (n <= extent[3])
    sequential, diverging = plt.get_cmap("viridis").copy(), plt.get_cmap("RdBu_r").copy()
    sequential.set_bad("#e1e4e8")
    diverging.set_bad("#e1e4e8")
    figure, axes = plt.subplots(2, 3, figsize=(12.4, 8.2), constrained_layout=True)
    scale_records = []
    for row, (obs_key, fit_key, mask_key, label, unit) in enumerate((
        ("observed_intensity", "fitted_intensity", "intensity_fit_mask", "Integrated HI intensity", "Jy beam⁻¹ km s⁻¹"),
        ("observed_velocity_km_s", "fitted_velocity_km_s", "velocity_fit_mask", "LOS mean velocity", "km s⁻¹"),
    )):
        observed, fitted, mask = arrays[obs_key], arrays[fit_key], arrays[mask_key]
        visible = mask & in_view
        if not visible.any():
            raise ValueError("no scored observation in declared detail view")
        residual = fitted-observed
        low = min(float(observed[visible].min()), float(fitted[visible].min()))
        high = max(float(observed[visible].max()), float(fitted[visible].max()))
        limit = max(float(abs(residual[visible]).max()), 1e-12)
        normal, residual_normal = Normalize(low, high), TwoSlopeNorm(vmin=-limit, vcenter=0, vmax=limit)
        scale_records.append({"quantity": label, "unit": unit, "source_mask_count": int(mask.sum()),
                              "visible_source_mask_count": int(visible.sum()), "shared_observed_model_limits": [low, high],
                              "residual_limits": [-limit, limit], "limits_rule": "actual extrema within declared view; no percentile clipping"})
        for column, (values, title) in enumerate(((observed, "Observed"), (fitted, "Projected fit"), (residual, "Fit − observed"))):
            ax = axes[row, column]
            picture = ax.pcolormesh(e, n, np.ma.array(values, mask=~mask), shading="nearest", rasterized=True,
                                   cmap=sequential if column < 2 else diverging,
                                   norm=normal if column < 2 else residual_normal)
            ax.set_facecolor("#e1e4e8")
            ax.plot(ce, cn, "+", color="white" if column < 2 else "black", markersize=7, markeredgewidth=.9)
            ax.set(xlim=extent[:2], ylim=extent[2:], aspect="equal", xlabel="East offset (arcsec)",
                   ylabel="North offset (arcsec)", title=f"{label}\n{title}")
            figure.colorbar(picture, ax=ax, shrink=.78, label=unit, pad=.025)
    figure.suptitle(f"{name}: central projection detail — declared display window", fontsize=15)
    figure.supxlabel("Display choice: projected inner cylinder (R ≤ 0.5 × concentration radius, |z| ≤ 6h) plus one beam margin.\n"
                       "Scales use within-view extrema; masks and full-frame scores unchanged. Full frame: " + name + "-projection.png", fontsize=9)
    paths = []
    for extension in ("png", "svg", "pdf"):
        path = output / f"{name}-projection-detail.{extension}"
        figure.savefig(path, dpi=160)
        paths.append(path)
    plt.close(figure)
    arrays.close()
    return {"galaxy": name, "results_sha256": sha256(report_path), "arrays_sha256": sha256(arrays_path),
            "figures": [{"file": str(path), "sha256": sha256(path)} for path in paths],
            "view_extent_arcsec": extent, "extent_order": ["east_max", "east_min", "north_min", "north_max"],
            "view_rule": "axis-aligned bounding box of projected R<=0.5*concentration radius, abs(z)<=6h cylinder, plus source major-beam margin",
            "display_radius_fraction_of_concentration": radius_fraction,
            "scales": scale_records, "model_refitted": False, "scores_recomputed": False,
            "full_frame_retained": True}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--results", required=True)
    parser.add_argument("--output", required=True)
    parser.add_argument("--details-only", action="store_true",
                        help="Add/refresh central views while preserving existing full-frame files")
    parser.add_argument("--galaxies", nargs="+", choices=("NGC2403", "NGC3198"),
                        default=["NGC2403", "NGC3198"])
    args = parser.parse_args()
    output = Path(args.output)
    output.mkdir(parents=True, exist_ok=True)
    if args.details_only:
        previous = json.loads((output / "plot-provenance.json").read_text())
        records = previous["plots"]
        for record in records:
            for figure in record["figures"]:
                if sha256(figure["file"]) != figure["sha256"]:
                    raise ValueError("existing full-frame figure identity changed")
        full_source = previous.get("full_plot_script_sha256", previous["script_sha256"])
    else:
        records = [plot_galaxy(Path(args.results) / name, output) for name in args.galaxies]
        full_source = sha256(__file__)
    details = [plot_detail(Path(args.results) / name, output) for name in args.galaxies]
    (output / "plot-provenance.json").write_text(json.dumps({"plots": records, "details": details,
        "script_sha256": sha256(__file__), "full_plot_script_sha256": full_source,
        "detail_plot_script_sha256": sha256(__file__)}, indent=2) + "\n")


if __name__ == "__main__":
    main()
