"""Data-conditioned stellar disks, with explicit projection and missing support.

This is the input side of Manual06's G2/G3 application, not a new dynamics law.
SPARC's inclination-corrected SBdisk fixes column luminosity. The exponential
vertical family and z_d/kpc = .196 (R_d/kpc)**.633 follow Lelli et al. (2016),
section 3.3, doi:10.3847/0004-6256/152/6/157. The scale-height relation is an
empirical input estimate; it does not define native octal addresses or beats.

Radial interpolation, axisymmetry and an optional exponential outer extension
are reconstruction choices. We retain the entire analytic vertical tail. The
unmeasured central region, gas volume density and bulge deprojection are unknown.
Main07 eq:m3-support-measure and eq:m3-map govern this weighted representation;
its positive physical density does not by itself certify native formation.
"""
from dataclasses import dataclass
from fractions import Fraction as Q
import hashlib
import math
from pathlib import Path

from simulations.sparc_inputs import PreparationRow


def _exact(value, name, *, positive=False):
    if type(value) not in (int, Q):
        raise ValueError(name + ' requires exact int/Fraction; bool and float excluded')
    value = Q(value)
    if value < 0 or (positive and value == 0):
        raise ValueError(name + ' must be ' + ('positive' if positive else 'nonnegative'))
    return value


def load_geometries(catalog, galaxies):
    """Read selected Table 1 fields with exact decimal values.

    Table1's catalog coordinates include the assumed distance. Joining to a
    different-distance radial file is rejected by ReconstructedDisk, rather
    than silently rescaling only one of the two sources.
    """
    path = Path(catalog)
    content = path.read_bytes()
    text = content.decode('ascii')
    if '62- 66 F5.2   kpc           Rdisk' not in text or 'Table: Galaxy Sample' not in text:
        raise ValueError('expected SPARC Table1 schema and kpc disk scale')
    wanted = set(galaxies)
    if not wanted or any(type(name) is not str or not name for name in wanted):
        raise ValueError('nonempty galaxy identities required')
    result, records = {}, {}
    digest = hashlib.sha256(content).hexdigest()
    for line_no, line in enumerate(text.splitlines(), 1):
        name = line[:11].strip()
        if name not in wanted:
            continue
        tokens = line.split()
        # The public ASCII data inserts spaces beyond the documented byte
        # widths. Bind its complete 19-column token schema instead of slicing
        # later fields at the inaccurate nominal byte offsets.
        if name in result or len(tokens) != 19 or tokens[0] != name:
            raise ValueError('duplicate or truncated galaxy catalog row')
        source = f'SPARC_Lelli2016c.mrt:{line_no}:sha256:{digest}'
        result[name] = DiskGeometry(name, Q(tokens[2]), Q(tokens[11]), Q(tokens[5]), source)
        records[name] = {'line': line_no, 'original_row': line,
            'distance_error_mpc': Q(tokens[3]),
            'inclination_error_deg': Q(tokens[6]),
            'total_luminosity_1e9_lsun': Q(tokens[7]),
            'luminosity_error_1e9_lsun': Q(tokens[8]),
            'effective_radius_kpc': Q(tokens[9]),
            'hi_mass_1e9_msun': Q(tokens[13]),
            'hi_radius_at_1_msun_pc2_kpc': Q(tokens[14]),
            'quality_flag': int(tokens[17]),
            'rotation_source_references': tokens[18]}
    if result.keys() != wanted:
        raise ValueError('catalog missing selected galaxies')
    return result, {'url': 'https://astroweb.cwru.edu/SPARC/SPARC_Lelli2016c.mrt',
                    'sha256': digest, 'rows': records}


@dataclass(frozen=True)
class DiskGeometry:
    galaxy: str
    distance_mpc: Q
    disk_scale_kpc: Q
    inclination_deg: Q
    source_reference: str

    def __post_init__(self):
        if not isinstance(self.galaxy, str) or not self.galaxy.strip():
            raise ValueError('galaxy identity required')
        for name in ('distance_mpc', 'disk_scale_kpc'):
            object.__setattr__(self, name, _exact(getattr(self, name), name, positive=True))
        inclination = _exact(self.inclination_deg, 'inclination')
        if inclination > 90:
            raise ValueError('inclination outside [0,90] degrees')
        object.__setattr__(self, 'inclination_deg', inclination)
        if not isinstance(self.source_reference, str) or not self.source_reference.strip():
            raise ValueError('geometry source reference required')

    @property
    def estimated_height_kpc(self):
        return .196 * float(self.disk_scale_kpc) ** .633


@dataclass(frozen=True)
class ReconstructedDisk:
    geometry: DiskGeometry
    rows: tuple
    mass_to_light: Q = Q(1, 2)

    def __post_init__(self):
        if type(self.geometry) is not DiskGeometry:
            raise ValueError('typed geometry required')
        self.geometry.__post_init__()
        rows = tuple(self.rows)
        if len(rows) < 2 or any(type(row) is not PreparationRow for row in rows):
            raise ValueError('at least two typed radial preparation rows required')
        last = Q(0)
        for row in rows:
            if row.key.galaxy != self.geometry.galaxy or row.distance_mpc != self.geometry.distance_mpc:
                raise ValueError('geometry galaxy/distance does not match radial source')
            radius = _exact(row.key.radius_kpc, 'radius', positive=True)
            if radius <= last:
                raise ValueError('radial support must be strictly ascending')
            _exact(row.sbdisk_lsun_pc2, 'surface luminosity')
            last = radius
        object.__setattr__(self, 'rows', rows)
        object.__setattr__(self, 'mass_to_light', _exact(self.mass_to_light, 'mass-to-light', positive=True))

    def column_luminosity(self, radius_kpc):
        """Exact linear radial interpolation on tabulated support only."""
        radius = _exact(radius_kpc, 'radius', positive=True)
        if not self.rows[0].key.radius_kpc <= radius <= self.rows[-1].key.radius_kpc:
            raise ValueError('radius outside tabulated support; an extension must be declared separately')
        for left, right in zip(self.rows, self.rows[1:]):
            a, b = left.key.radius_kpc, right.key.radius_kpc
            if a <= radius <= b:
                t = (radius - a) / (b - a)
                return (1-t)*left.sbdisk_lsun_pc2 + t*right.sbdisk_lsun_pc2
        raise ValueError('radial support lookup failed')

    def column_mass(self, radius_kpc):
        """Solar masses / pc^2, not native RCD or support cardinality."""
        return self.mass_to_light * self.column_luminosity(radius_kpc)

    def volume_density(self, radius_kpc, z_kpc, *, height_kpc=None):
        """Solar masses / pc^3, axisymmetric exponential vertical family."""
        height = self.geometry.estimated_height_kpc if height_kpc is None else height_kpc
        if type(height) not in (int, float, Q) or not math.isfinite(float(height)) or height <= 0:
            raise ValueError('finite positive physical height required')
        if type(z_kpc) not in (int, float, Q) or not math.isfinite(float(z_kpc)):
            raise ValueError('finite vertical coordinate required')
        height = float(height)
        return float(self.column_mass(radius_kpc))/(2000*height)*math.exp(-abs(float(z_kpc))/height)

    def project_column(self, radius_kpc):
        """Analytic integral over all z; exactly the tabulated face-on column.

        Integral exp(-|z|/h)/(2h) dz = 1 for every h>0. Consequently this
        projection alone cannot fit h. No finite vertical truncation is hidden.
        """
        return self.column_luminosity(radius_kpc)

    def band_accounting(self, radius_kpc, half_height_kpc):
        """Retained +/-z band plus two analytic tails in solar masses/pc^2."""
        if (type(half_height_kpc) not in (int, float, Q) or
                not math.isfinite(float(half_height_kpc)) or half_height_kpc < 0):
            raise ValueError('finite nonnegative band half-height required')
        total = float(self.column_mass(radius_kpc))
        outside_fraction = math.exp(-float(half_height_kpc)/self.geometry.estimated_height_kpc)
        return {'column_mass': total, 'inside': total*(-math.expm1(-float(half_height_kpc)/self.geometry.estimated_height_kpc)),
                'below': total*outside_fraction/2, 'above': total*outside_fraction/2,
                'unit': 'Msun/pc^2', 'tail_status': 'retained analytic extrapolation'}

    def annular_mass_over_pi(self):
        """Exact mass/pi in Msun over the observed radial annulus, all z.

        The 10^6 converts kpc^2 to pc^2. Linear surface interpolation makes
        2 integral R Sigma(R) dR a polynomial with rational coefficients.
        """
        total = Q(0)
        for left, right in zip(self.rows, self.rows[1:]):
            a, b = left.key.radius_kpc, right.key.radius_kpc
            sa = self.mass_to_light*left.sbdisk_lsun_pc2
            sb = self.mass_to_light*right.sbdisk_lsun_pc2
            slope = (sb-sa)/(b-a)
            intercept = sa-slope*a
            total += 2*(slope*(b**3-a**3)/3 + intercept*(b**2-a**2)/2)
        return total*10**6

    def outer_extrapolated_mass_over_pi(self):
        """Separate optional exponential continuation, not measured outer mass.

        Sigma(R)=Sigma(Rmax) exp(-(R-Rmax)/Rd), R>=Rmax; no outer cutoff.
        A rounded zero endpoint makes this continuation zero, but cannot
        establish zero physical outer mass. See positive_anchor_extension.
        """
        row = self.rows[-1]
        rd = self.geometry.disk_scale_kpc
        return 2*self.mass_to_light*row.sbdisk_lsun_pc2*(rd*row.key.radius_kpc+rd**2)*10**6

    def positive_anchor_extension(self):
        """A separately declared outer estimate from the last positive column.

        This uses the catalog Rd, not an invented boundary cutoff. It retains
        the amplitude/source and exposes disagreement with rounded outer rows.
        It does not overwrite the tabulated density or establish true tails.
        """
        positive = [row for row in self.rows if row.sbdisk_lsun_pc2 > 0]
        if not positive:
            return {'status': 'unavailable: no positive tabulated anchor', 'mass_over_pi_msun': None}
        anchor = positive[-1]
        rd, rmax = float(self.geometry.disk_scale_kpc), float(self.rows[-1].key.radius_kpc)
        sbmax = float(anchor.sbdisk_lsun_pc2)*math.exp(-(rmax-float(anchor.key.radius_kpc))/rd)
        return {'status': 'inferred exponential extension from last positive tabulated column',
                'anchor_radius_kpc': anchor.key.radius_kpc,
                'anchor_luminosity_lsun_pc2': anchor.sbdisk_lsun_pc2,
                'estimated_luminosity_at_rmax_lsun_pc2': sbmax,
                'tabulated_luminosity_at_rmax_lsun_pc2': self.rows[-1].sbdisk_lsun_pc2,
                'mass_over_pi_msun': 2*float(self.mass_to_light)*sbmax*(rd*rmax+rd**2)*10**6,
                'physical_tail_uncertainty': None}

    def report(self):
        height = self.geometry.estimated_height_kpc
        return {
            'galaxy': self.geometry.galaxy,
            'input_geometry': self.geometry,
            'stellar_mass_to_light_msun_lsun': self.mass_to_light,
            'estimated_height_kpc': height,
            'height_source': 'Lelli+2016 section 3.3 empirical height relation; uncertainty unsupplied',
            'radial_domain_kpc': (self.rows[0].key.radius_kpc, self.rows[-1].key.radius_kpc),
            'radial_display': {'minimum_kpc': float(self.rows[0].key.radius_kpc),
                'maximum_kpc': float(self.rows[-1].key.radius_kpc),
                'decades': math.log10(float(self.rows[-1].key.radius_kpc/self.rows[0].key.radius_kpc))},
            'angular_model': 'axisymmetric reconstruction, no measured angular structure supplied',
            'vertical_domain': 'all real z; exponential tails retained analytically',
            'annular_mass_over_pi_msun': self.annular_mass_over_pi(),
            'rounded_endpoint_continuation_mass_over_pi_msun': self.outer_extrapolated_mass_over_pi(),
            'outer_positive_anchor_extension': self.positive_anchor_extension(),
            'tabulated_zero_interpretation': 'zero at published precision, not certified empty physical support',
            'central_mass': None, 'gas_volume_density': None, 'bulge_volume_density': None,
            'formation_histories': None,
            'physical_density_is_native_rcd': False,
            'height_identifiability': 'column projection identical for all positive heights; catalog relation supplies estimate',
            'rows': tuple({'radius_kpc': row.key.radius_kpc,
                'surface_luminosity_lsun_pc2': row.sbdisk_lsun_pc2,
                'column_mass_msun_pc2': self.column_mass(row.key.radius_kpc),
                'midplane_density_msun_pc3': self.volume_density(row.key.radius_kpc, 0),
                'projected_luminosity_lsun_pc2': self.project_column(row.key.radius_kpc),
                'projection_residual_lsun_pc2': self.project_column(row.key.radius_kpc)-row.sbdisk_lsun_pc2,
                'characteristic_height_band': self.band_accounting(row.key.radius_kpc, height)} for row in self.rows),
            'status': 'data-conditioned stellar reconstruction; exact interpolation fit, no native evolution claim',
        }
