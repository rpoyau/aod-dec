#!/usr/bin/env python3
"""Reproduce SPARC stellar reconstruction and separate inverse diagnostics."""
import argparse
import csv
from dataclasses import asdict, is_dataclass
from fractions import Fraction
import hashlib
import json
from pathlib import Path
import sys

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))
from simulations.sparc_inputs import load_inputs, scale_inventory
from simulations.sparc_comparison import compare
from simulations.sparc_inverse import baseline_projections, inverse_requirements, fit_effective_factors
from simulations.sparc_native_span import extract_native_span
from simulations.sparc_reconstruction import load_geometries, ReconstructedDisk


def encode(value):
    if is_dataclass(value):
        return encode(asdict(value))
    if isinstance(value, Fraction):
        return str(value)
    if isinstance(value, dict):
        return {str(k): encode(v) for k, v in value.items()}
    if isinstance(value, (tuple, list)):
        return [encode(v) for v in value]
    return value


def run(input_directory, catalog):
    data = load_inputs(input_directory)
    galaxies = sorted({key.galaxy for key in data.preparation})
    geometries, catalog_binding = load_geometries(catalog, galaxies)
    reconstructed = tuple(ReconstructedDisk(geometries[galaxy],
        tuple(row for key, row in sorted(data.preparation.items()) if key.galaxy == galaxy)).report()
        for galaxy in galaxies)
    evidence = tuple(f'{Path(source.path).name}:sha256:{source.sha256}' for source in data.sources)
    control = compare(data.targets, baseline_projections(data.preparation, source_evidence=evidence))
    inverse = inverse_requirements(data.preparation, data.targets)
    fitted = fit_effective_factors(data.preparation, data.targets, source_evidence=evidence)
    bindings = {}
    for name in ('sparc_inputs', 'sparc_comparison', 'sparc_inverse', 'sparc_native_span', 'sparc_reconstruction'):
        relative = f'simulations/{name}.py'
        bindings[relative] = hashlib.sha256((ROOT/relative).read_bytes()).hexdigest()
    bindings['scripts/reconstruct_sparc.py'] = hashlib.sha256(Path(__file__).read_bytes()).hexdigest()
    return {
        'format': 'sparc-reconstruction-v1',
        'main_manual_commit': 'ab8359209b8f9b2ef4007403101016e3ea58cfe4',
        'source_files': tuple({'name': Path(s.path).name, 'sha256': s.sha256, 'rows': len(s.rows)} for s in data.sources),
        'implementation_sha256': bindings, 'catalog': catalog_binding,
        'physical_scales': scale_inventory(data), 'stellar_reconstruction': reconstructed,
        'frozen_g0_control': control, 'target_conditioned_requirements': inverse,
        'target_conditioned_scalar_fit': fitted,
        'display': {'g0_mape_percent': f"{float(control['primary_mape_percent']):.12f}",
                    'g0_equal_galaxy_mape_percent': f"{float(control['equal_galaxy_mape_percent']):.12f}",
                    'in_sample_mape_percent': f"{float(fitted['comparison']['primary_mape_percent']):.12f}",
                    'negative_required_contributions': sum(row['required_dbf'] < 0 for row in inverse['rows'])},
        'native_span': extract_native_span(()),
        'stage_status': {
            'field_input': 'stellar density family reconstructed; central, gas, bulge, background and temporal support incomplete',
            'native_window': 'extractor implemented; no galaxy formation histories supplied or manufactured',
            'aod_evolution': 'not run: reconstructed physical weights are not yet bound to native formation/current histories',
            'projection': 'analytic inclination-corrected stellar column projection executed; no evolved galaxy projection',
            'comparison': f'all {len(data.targets)} frozen G0 targets and explicit in-sample inverse diagnostic; no new G2/G3 score',
        },
        'missing_input_binding': {
            'calculus': 'existing AOD inverse-pressure/current response remains unchanged',
            'native_support': 'measured or inferred mass/size and density to retained support/formation histories',
            'phase_hook': 'Manual Appendix C mass/size-to-beta_a requires an acquisition adapter; fixed beta=5 fixture is not used',
            'time_and_currents': 'density snapshot does not uniquely specify directional returns or coupled temporal history',
        },
        'uncertainty': 'Physical height, mass-to-light, distance, inclination and reconstructed profiles have no joint law here; no total confidence bound is asserted.',
    }


def write_report(result, output):
    output.mkdir(parents=True, exist_ok=True)
    (output/'results.json').write_text(json.dumps(encode(result), indent=2, sort_keys=True, allow_nan=False)+'\n')
    with (output/'stellar-density.csv').open('w', newline='') as stream:
        fields = ['galaxy', 'radius_kpc', 'surface_luminosity_lsun_pc2', 'column_mass_msun_pc2',
                  'midplane_density_msun_pc3', 'projected_luminosity_lsun_pc2', 'projection_residual_lsun_pc2']
        writer = csv.DictWriter(stream, fieldnames=fields, lineterminator='\n')
        writer.writeheader()
        for galaxy in result['stellar_reconstruction']:
            for row in galaxy['rows']:
                writer.writerow(encode({'galaxy': galaxy['galaxy'], **{key: row[key] for key in fields[1:]}}))
    with (output/'required-current.csv').open('w', newline='') as stream:
        fields = ['galaxy', 'radius_kpc', 'u_bar', 'u_g0', 'u_observed', 'required_dbf',
                  'required_effective_factor', 'factor_status']
        writer = csv.DictWriter(stream, fieldnames=fields, lineterminator='\n')
        writer.writeheader()
        for row in result['target_conditioned_requirements']['rows']:
            writer.writerow(encode({**row['key'], **{key: row[key] for key in fields[2:]}}))
    baseline = result['frozen_g0_control']
    fitted = result['target_conditioned_scalar_fit']['comparison']
    text = [
        '# SPARC reconstruction result', '',
        f"The {baseline['galaxy_count']}-galaxy, {baseline['bins']}-bin stellar reconstruction and its analytic column projection ran. "
        'The stellar profile is constrained by the input data. It does not supply native return histories. '
        'AOD evolution of these galaxies has not run.', '',
        '| Comparison | Equal-bin square-speed MAPE | Equal-galaxy MAPE |',
        '|---|---:|---:|',
        f"| Frozen G0 control | {float(baseline['primary_mape_percent']):.12f}% | {float(baseline['equal_galaxy_mape_percent']):.12f}% |",
        f"| Target-conditioned scalar diagnostic | {float(fitted['primary_mape_percent']):.12f}% | {float(fitted['equal_galaxy_mape_percent']):.12f}% |", '',
        'The second row fits one effective factor per galaxy to these same targets. It is an inverse diagnostic; '
        'its improvement is not a G2/G3 prediction or evidence that a recovered density produced that current.', '',
        'The vertical scale is estimated from catalog disk size using the SPARC empirical relation. '
        'The full exponential vertical tails remain accounted for. Positive heights produce the same column, '
        'so this projected profile alone cannot identify thickness.', '',
        'Native beat separations, windows and scale ranges are computed from certified return histories by '
        '`extract_native_span`. With no galaxy histories, that output remains unavailable. '
        'Physical radii, mass density and cadence are kept as distinct quantities.', '',
        'Exact rational values and all signed inverse requirements are in results.json and required-current.csv. '
        'Unknown central/gas/bulge volume density and unsupplied physical uncertainty are retained as unknown.', '',
    ]
    (output/'REPORT.md').write_text('\n'.join(text))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--input', type=Path, default=ROOT/'data/sparc')
    parser.add_argument('--catalog', type=Path, default=ROOT/'data/sparc/SPARC_Lelli2016c.mrt')
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    result = run(args.input, args.catalog)
    write_report(result, args.output)
    print(json.dumps({'output': str(args.output), 'bins': result['frozen_g0_control']['bins'],
                     'g0_mape_percent': float(result['frozen_g0_control']['primary_mape_percent']),
                     'native_evolution': 'not run'}))


if __name__ == '__main__':
    main()
