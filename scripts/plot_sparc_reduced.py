#!/usr/bin/env python3
"""Plot every retained bin from an executed reduced comparison CSV."""
import argparse
import csv
from fractions import Fraction
from pathlib import Path


def plot(source, output):
    import matplotlib
    matplotlib.use('Agg')
    import matplotlib.pyplot as plt
    with source.open(newline='') as stream:
        records = list(csv.DictReader(stream))
    galaxies = sorted({row['galaxy'] for row in records})
    figure, axes = plt.subplots(len(galaxies), 2, figsize=(10, 2.4*len(galaxies)), squeeze=False)
    for galaxy, (curve, residual) in zip(galaxies, axes):
        for case, label, color in (('disabled', 'Frozen G0', '#748391'),
                                   ('spacing', 'Reduced response', '#157a80')):
            rows = sorted((r for r in records if r['galaxy'] == galaxy and r['case'] == case),
                          key=lambda r: Fraction(r['radius_kpc']))
            radius = [float(Fraction(r['radius_kpc'])) for r in rows]
            if case == 'disabled':
                curve.plot(radius, [float(Fraction(r['u_observed'])) for r in rows],
                           '.', color='#172b4d', markersize=3, label='Observed squared speed')
            curve.plot(radius, [float(Fraction(r['u_projected'])) for r in rows],
                       color=color, linewidth=1.3, label=label)
            residual.plot(radius, [100*float(Fraction(r['signed_relative_residual'])) for r in rows],
                          color=color, linewidth=1.2, label=label)
        curve.set_title(galaxy, loc='left', fontsize=11)
        curve.set_ylabel('Squared speed / (km/s)²')
        residual.set_ylabel('Signed relative residual / %')
        residual.axhline(0, color='#acb6bf', linewidth=.7)
        for axis in (curve, residual):
            axis.set_xlabel('Radius / kpc')
            axis.grid(alpha=.18)
    axes[0, 0].legend(fontsize=7)
    axes[0, 1].legend(fontsize=7)
    figure.suptitle('Executed radial reduction — complete retained cohort', fontsize=13)
    figure.tight_layout(rect=(0, 0, 1, .98))
    output.parent.mkdir(parents=True, exist_ok=True)
    figure.savefig(output, dpi=160)
    plt.close(figure)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--input', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    plot(args.input, args.output)
