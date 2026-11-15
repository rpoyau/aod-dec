"""Finite 3D HI inverse reconstruction through an observed projection.

This is a fitted observational state and its forward image, not an AOD temporal
transition law. All floating-point geometry is confined to this application
mapping. The native exact compiler/trace are unchanged. A radial hat basis and
normalized exponential vertical profiles define actual volume densities;
nonnegative coefficients have fixed total HI mass. Optimizer iterations are
not physical time, and projected agreement does not identify unobserved depth.
"""
from __future__ import annotations
from dataclasses import dataclass
import math


def _np():
    try:
        import numpy as np
    except ImportError as exc:
        raise RuntimeError('projected fitting requires optional NumPy') from exc
    return np


def _finite(value, name, *, positive=False):
    if isinstance(value, bool) or not isinstance(value, (int, float)) or not math.isfinite(value):
        raise ValueError(name + ' must be a finite real number')
    if positive and value <= 0:
        raise ValueError(name + ' must be positive')
    return float(value)


@dataclass(frozen=True)
class Geometry:
    center_east_arcsec: float
    center_north_arcsec: float
    pa_deg: float
    inclination_deg: float
    height_arcsec: float

    def __post_init__(self):
        for name in self.__dataclass_fields__:
            _finite(getattr(self, name), name)
        if not 0 <= self.inclination_deg < 89:
            raise ValueError('inclination must lie in [0,89) degrees for this LOS parameterization')
        if self.height_arcsec <= 0:
            raise ValueError('height must be positive')


def radial_areas(knots):
    """Exact analytic angular areas of piecewise-linear hats; last node is zero.

    knots starts at zero; amplitudes are supplied at all except the last knot.
    No modeled radial material exists beyond the last knot. This is a finite
    reconstruction window, never a claim that the real galaxy ends there.
    """
    np = _np()
    r = np.asarray(knots, dtype=float)
    if r.ndim != 1 or len(r) < 3 or not np.isfinite(r).all() or r[0] != 0 or np.any(np.diff(r) <= 0):
        raise ValueError('at least three ascending finite radial knots beginning at zero required')
    area = np.zeros(len(r)-1)
    for j in range(len(area)):
        if j:
            a, b = r[j-1:j+1]
            area[j] += 2*math.pi*((b**3-a**3)/3-a*(b*b-a*a)/2)/(b-a)
        a, b = r[j:j+2]
        area[j] += 2*math.pi*(b*(b*b-a*a)/2-(b**3-a**3)/3)/(b-a)
    return area


def _hat_add(output, radius, weights, knots, current=None, projection=None):
    """Accumulate the two nonzero radial hats per sky ray into a dense basis."""
    np = _np()
    n = len(knots)-1
    index = np.searchsorted(knots, radius, side='right')-1
    valid = (index >= 0) & (index < n)
    pix = np.flatnonzero(valid.ravel())
    low = index.ravel()[pix]
    fraction = (radius.ravel()[pix]-knots[low])/(knots[low+1]-knots[low])
    weight = np.broadcast_to(weights, radius.shape).ravel()[pix]
    left_weight = (1-fraction)*weight
    output[pix, low] += left_weight
    other = low+1 < n
    output[pix[other], low[other]+1] += fraction[other]*weight[other]
    if current is not None:
        projected = projection.ravel()[pix]
        current[pix, low] += left_weight*projected
        current[pix[other], low[other]+1] += fraction[other]*weight[other]*projected[other]


def radial_enclosed_fractions(knots, radius):
    """Analytic fraction of each cylindrical hat inside a declared radial view."""
    np = _np()
    areas = radial_areas(knots)
    r = np.asarray(knots, dtype=float)
    radius = _finite(radius, 'radial view radius')
    if radius < 0:
        raise ValueError('radial view radius must be nonnegative')
    enclosed = np.zeros_like(areas)
    for j in range(len(areas)):
        if j:
            a, b = r[j-1:j+1]
            t = max(a, min(b, radius))
            enclosed[j] += 2*math.pi*((t**3-a**3)/3-a*(t*t-a*a)/2)/(b-a)
        a, b = r[j:j+2]
        t = max(a, min(b, radius))
        enclosed[j] += 2*math.pi*(b*(t*t-a*a)/2-(t**3-a**3)/3)/(b-a)
    return enclosed/areas


def grid_jacobian(east, north):
    np = _np()
    if east.shape != north.shape or east.ndim != 2 or min(east.shape) < 2:
        raise ValueError('matching 2D sky grids with at least two samples per axis required')
    jac = np.array([[np.median(np.diff(east, axis=1)), np.median(np.diff(east, axis=0))],
                    [np.median(np.diff(north, axis=1)), np.median(np.diff(north, axis=0))]])
    if not np.isfinite(jac).all() or abs(np.linalg.det(jac)) < 1e-10:
        raise ValueError('singular/nonfinite sky grid')
    return jac


def beam_convolve(images, covariance_pixels):
    """Zero-padded normalized elliptical Gaussian convolution, no wraparound."""
    np = _np()
    arr = np.asarray(images, dtype=float)
    cov = np.asarray(covariance_pixels, dtype=float)
    if arr.ndim not in (2, 3) or not np.isfinite(arr).all():
        raise ValueError('finite images required')
    if cov.shape != (2, 2) or not np.isfinite(cov).all() or not np.allclose(cov, cov.T) or np.linalg.eigvalsh(cov).min() < 0:
        raise ValueError('positive semidefinite symmetric beam covariance required')
    # A sampled real-space positive kernel avoids negative ringing from a
    # poorly sampled analytic Fourier Gaussian. Padding includes its support.
    radius = max(1, int(math.ceil(6*math.sqrt(float(np.linalg.eigvalsh(cov).max())))))
    if np.linalg.eigvalsh(cov).max() < 1e-20:
        return arr.copy()
    if np.linalg.eigvalsh(cov).min() <= 0:
        raise ValueError('nonzero beam must have positive definite covariance')
    yy, xx = np.mgrid[-radius:radius+1, -radius:radius+1]
    inv = np.linalg.inv(cov)
    kernel = np.exp(-.5*(inv[0,0]*xx*xx+2*inv[0,1]*xx*yy+inv[1,1]*yy*yy))
    kernel /= kernel.sum()
    shape = (arr.shape[0]+kernel.shape[0]-1, arr.shape[1]+kernel.shape[1]-1)
    fk = np.fft.rfft2(kernel, s=shape)
    fa = np.fft.rfft2(arr, s=shape, axes=(0, 1))
    convolved = np.fft.irfft2(fa*(fk[..., None] if arr.ndim == 3 else fk), s=shape, axes=(0, 1))
    return convolved[radius:radius+arr.shape[0], radius:radius+arr.shape[1]]


def projected_basis(east, north, knots, geometry, beam_covariance_arcsec2,
                    beam_area_arcsec2, *, quadrature_order=32):
    """Return intensity and circular-current bases per Jy km/s intrinsic flux.

    x is the disk major axis. At each z, y=(sky_minor-z sin i)/cos i;
    ds=dz/cos i. Gauss-Laguerre integrates each normalized exponential half
    over [0,infinity). The vertical domain is not silently truncated.
    The current basis includes sin(i)*x/R and is convolved BEFORE division
    by the intensity. Model pixels outside the observed footprint are retained
    in the global analytic normalization and reported separately by the caller.
    """
    np = _np()
    if type(geometry) is not Geometry:
        raise ValueError('typed geometry required')
    geometry.__post_init__()
    east, north = np.asarray(east, dtype=float), np.asarray(north, dtype=float)
    if not np.isfinite(east).all() or not np.isfinite(north).all():
        raise ValueError('finite sky coordinates required')
    jac = grid_jacobian(east, north)
    original_shape = east.shape
    inverse = np.linalg.inv(jac)
    cov_pix = inverse @ np.asarray(beam_covariance_arcsec2) @ inverse.T
    if cov_pix.shape != (2, 2) or not np.isfinite(cov_pix).all() or np.linalg.eigvalsh(cov_pix).min() < 0:
        raise ValueError('finite positive semidefinite beam covariance required')
    # Render the PSF halo as well as the requested view. Zero padding the
    # convolution alone would incorrectly discard incoming emission from just
    # outside a cropped field. Keep the exact original coordinates in the view;
    # extrapolation of this local sky chart is confined to the beam halo.
    pad = max(1, int(math.ceil(6*math.sqrt(float(np.linalg.eigvalsh(cov_pix).max())))))
    yy, xx = np.mgrid[-pad:original_shape[0]+pad, -pad:original_shape[1]+pad]
    padded_east = east[0, 0]+jac[0, 0]*xx+jac[0, 1]*yy
    padded_north = north[0, 0]+jac[1, 0]*xx+jac[1, 1]*yy
    padded_east[pad:pad+original_shape[0],pad:pad+original_shape[1]] = east
    padded_north[pad:pad+original_shape[0],pad:pad+original_shape[1]] = north
    east, north = padded_east, padded_north
    area = radial_areas(knots)
    knots = np.asarray(knots, dtype=float)
    beam_area_arcsec2 = _finite(beam_area_arcsec2, 'beam area', positive=True)
    if type(quadrature_order) is not int or not 4 <= quadrature_order <= 128:
        raise ValueError('quadrature order must be an integer in [4,128]')
    angle, inc = math.radians(geometry.pa_deg), math.radians(geometry.inclination_deg)
    e, n = east-geometry.center_east_arcsec, north-geometry.center_north_arcsec
    major = e*math.sin(angle)+n*math.cos(angle)
    minor = e*math.cos(angle)-n*math.sin(angle)
    sini, cosi = math.sin(inc), math.cos(inc)
    basis = np.zeros((east.size, len(area)))
    current = np.zeros_like(basis)
    nodes, weights = np.polynomial.laguerre.laggauss(quadrature_order)
    for node, weight in zip(nodes, weights):
        for sign in (-1, 1):
            z = sign*node*geometry.height_arcsec
            y = (minor-z*sini)/cosi
            r = np.hypot(major, y)
            projection = np.divide(sini*major, r, out=np.zeros_like(r), where=r > 0)
            _hat_add(basis, r, weight/(2*cosi), knots, current, projection)
    normalizers = beam_area_arcsec2/area
    basis *= normalizers
    current *= normalizers
    basis = beam_convolve(basis.reshape(east.shape+(len(area),)), cov_pix)
    current = beam_convolve(current.reshape(east.shape+(len(area),)), cov_pix)
    view = (slice(pad,pad+original_shape[0]),slice(pad,pad+original_shape[1]))
    basis, current = basis[view], current[view]
    # FFT roundoff can introduce machine-size negatives into positive images.
    tolerance = 1e-12*max(float(abs(basis).max()), 1e-30)
    if basis.min() < -tolerance:
        raise ArithmeticError('beam convolution produced substantive negative density')
    basis = np.maximum(basis, 0)
    return basis, current


def simplex_least_squares(matrix, target, total=1.0, *, max_iterations=1000):
    """Active-set convex least squares with x>=0 and sum(x)=total.

    The equality fixes the mass/flux normalization and is never imposed by
    renormalizing a cropped model image. Returns a numerical KKT audit.
    """
    np = _np()
    a, b = np.asarray(matrix, float), np.asarray(target, float)
    total = _finite(total, 'simplex total', positive=True)
    if a.ndim != 2 or b.shape != (a.shape[0],) or not a.shape[1] or not np.isfinite(a).all() or not np.isfinite(b).all():
        raise ValueError('finite aligned least-squares matrix and target required')
    g, rhs = a.T @ a, a.T @ b
    scale = max(float(abs(g).max()), 1e-300)
    g, rhs = g/scale, rhs/scale
    n = a.shape[1]
    x = np.full(n, total/n)
    free = set(range(n))
    converged = False
    for iteration in range(max_iterations):
        active = sorted(free)
        block = g[np.ix_(active, active)]
        system = np.block([[block, np.ones((len(active),1))],
                           [np.ones((1,len(active))), np.zeros((1,1))]])
        solution = np.linalg.lstsq(system, np.r_[rhs[active], total], rcond=1e-12)[0]
        proposal = np.zeros(n)
        proposal[active] = solution[:-1]
        negative = [j for j in active if proposal[j] < -1e-12*total]
        if negative:
            alpha = min(x[j]/(x[j]-proposal[j]) for j in negative)
            x += alpha*(proposal-x)
            for j in active:
                if x[j] <= 1e-12*total and proposal[j] < 0:
                    x[j] = 0
                    free.discard(j)
            if not free:
                raise ArithmeticError('empty simplex active set')
            continue
        x = np.maximum(proposal, 0)
        x *= total/x.sum()  # only roundoff correction on intrinsic coefficients
        gradient = g@x-rhs
        level = float(np.mean(gradient[active]))
        inactive = [j for j in range(n) if j not in free]
        if not inactive or min(gradient[j]-level for j in inactive) >= -1e-10*max(1,total):
            converged = True
            break
        free.add(min(inactive, key=lambda j: gradient[j]))
    gradient = g@x-rhs
    positive = x > 1e-10*total
    level = float(np.mean(gradient[positive]))
    kkt = max(float(np.max(abs(gradient[positive]-level))),
              max(0., -float(np.min(gradient[~positive]-level))) if (~positive).any() else 0.)
    if not converged:
        raise ArithmeticError('simplex fit did not converge')
    return x, {'iterations': iteration+1, 'converged': converged,
               'relative_mass_sum_error': abs(float(x.sum())-total)/total,
               'scaled_kkt_residual': kkt, 'active_coefficients': int(positive.sum())}


def bounded_least_squares(matrix, target, lower, upper, *, max_iterations=5000):
    """Small box-constrained least squares by cyclic exact coordinate updates."""
    np = _np()
    a, b = np.asarray(matrix, float), np.asarray(target, float)
    lo, hi = np.asarray(lower, float), np.asarray(upper, float)
    if a.ndim != 2 or b.shape != (a.shape[0],) or lo.shape != hi.shape or lo.shape != (a.shape[1],):
        raise ValueError('unaligned bounded problem')
    if not all(np.isfinite(v).all() for v in (a,b,lo,hi)) or np.any(lo > hi):
        raise ValueError('finite ordered bounds required')
    g, rhs = a.T@a, a.T@b
    x = np.clip(np.linalg.lstsq(a,b,rcond=1e-10)[0], lo, hi)
    scale = max(float(abs(rhs).max()), 1.)
    for iteration in range(max_iterations):
        for j in range(len(x)):
            if g[j,j] > 1e-24:
                x[j] = np.clip(x[j]+(rhs[j]-g[j]@x)/g[j,j], lo[j], hi[j])
        grad = g@x-rhs
        projected = x-np.clip(x-grad, lo, hi)
        if float(abs(projected).max()) <= 1e-9*scale:
            break
    else:
        raise ArithmeticError('bounded velocity fit did not converge')
    return x, {'iterations':iteration+1, 'projected_gradient_residual':float(abs(projected).max())/scale,
               'at_bound_count':int(np.count_nonzero((abs(x-lo)<1e-7)|(abs(x-hi)<1e-7)))}


def fit_at_geometry(east, north, intensity, velocity, fit_mask, velocity_mask,
                    knots, geometry, beam_covariance, beam_area, total_flux,
                    *, quadrature_order=32):
    np = _np()
    y, v = np.asarray(intensity,float), np.asarray(velocity,float)
    mask, vm = np.asarray(fit_mask,bool), np.asarray(velocity_mask,bool)
    if any(arr.shape != east.shape for arr in (y,v,mask,vm)):
        raise ValueError('all observation maps and masks must align')
    if (not mask.any() or not vm.any() or not np.isfinite(y[mask]).all()
            or not np.isfinite(v[vm]).all() or not np.isfinite(y[vm]).all()):
        raise ValueError('finite selected observations required')
    if not (y[vm] > 0).any():
        raise ValueError('velocity loss requires positive total observed intensity weight')
    basis, current = projected_basis(east,north,knots,geometry,beam_covariance,beam_area,quadrature_order=quadrature_order)
    weights, mass_audit = simplex_least_squares(basis[mask],y[mask],float(total_flux))
    image = basis@weights
    # The fixed source velocity mask is never tightened according to model fit.
    # A tiny denominator floor avoids arithmetic singularity; count its use.
    floor = max(float(image.max())*1e-12,1e-30)
    denominator = np.maximum(image,floor)
    design = current*weights/denominator[...,None]
    observed_mid = float(np.median(v[vm]))
    limit = max(float(np.max(abs(v[vm]-observed_mid)))*2,1.)/max(math.sin(math.radians(geometry.inclination_deg)),.1)
    vmatrix = np.column_stack((np.ones(int(vm.sum())),design[vm]))
    # Flux weighting stabilizes noisy faint outskirts. It is a declared loss,
    # not a statistical inverse covariance or noise estimate.
    velocity_weights = np.sqrt(np.maximum(y[vm],0)/max(float(np.max(y[vm])),1e-30))
    params, velocity_audit = bounded_least_squares(vmatrix*velocity_weights[:,None],(v[vm]-observed_mid)*velocity_weights,
        np.r_[-limit,np.full(len(weights),-limit)],np.r_[limit,np.full(len(weights),limit)])
    systemic = observed_mid+params[0]
    model_velocity = systemic+design@params[1:]
    ir = image[mask]-y[mask]
    vr = model_velocity[vm]-v[vm]
    intensity_nrmse = math.sqrt(float(ir@ir)/max(float(y[mask]@y[mask]),1e-300))
    weighted_velocity_rmse = math.sqrt(float(np.sum(velocity_weights**2*vr**2))/float(np.sum(velocity_weights**2)))
    velocity_scale = max(float(np.percentile(v[vm],95)-np.percentile(v[vm],5))/2,1.)
    score = intensity_nrmse**2+(weighted_velocity_rmse/velocity_scale)**2
    jac = grid_jacobian(east,north)
    pixel_area = abs(float(np.linalg.det(jac)))
    projected_flux = float(image.sum())*pixel_area/beam_area
    return {'geometry':geometry,'weights':weights,'rotation_coefficients_km_s':params[1:],
        'systemic_velocity_km_s':systemic,'intensity':image,'velocity':model_velocity,
        'score':score,'intensity_nrmse':intensity_nrmse,
        'intensity_wape_fraction':float(abs(ir).sum())/max(float(abs(y[mask]).sum()),1e-300),
        'velocity_rmse_km_s':math.sqrt(float(np.mean(vr**2))),
        'velocity_mae_km_s':float(np.mean(abs(vr))),
        'intensity_weighted_velocity_rmse_km_s':weighted_velocity_rmse,
        'velocity_loss_scale_km_s':velocity_scale,
        'intrinsic_total_flux_jy_km_s':float(weights.sum()),
        'projected_grid_flux_jy_km_s':projected_flux,
        'grid_flux_deficit_fraction':1-projected_flux/float(weights.sum()),
        'mass_audit':mass_audit,'velocity_audit':velocity_audit,
        'velocity_denominator_floor_count':int(np.count_nonzero(vm & (image<floor))),
        'intensity_sample_count':int(mask.sum()),'velocity_sample_count':int(vm.sum()),
        'loss_is_statistical_chi_square':False,'quadrature_order':quadrature_order}


def volume_density(radius, z, knots, flux_weights, height, total_mass):
    """Actual inferred mass density in Msun/kpc^3 for kpc coordinates/knots."""
    np = _np()
    area = radial_areas(knots)
    weights = np.asarray(flux_weights,float)
    if weights.shape != area.shape or not np.isfinite(weights).all() or np.any(weights<0) or weights.sum()<=0:
        raise ValueError('nonnegative nonempty mass mixture required')
    height = _finite(height,'height',positive=True)
    total_mass = _finite(total_mass,'total mass',positive=True)
    radius, z = np.broadcast_arrays(np.asarray(radius,float),np.asarray(z,float))
    if not np.isfinite(radius).all() or not np.isfinite(z).all() or np.any(radius<0):
        raise ValueError('finite coordinates and nonnegative radius required')
    column_nodes = total_mass*weights/weights.sum()/area
    column = np.interp(radius,knots,np.r_[column_nodes,0.],left=0,right=0)
    return column*np.exp(-abs(z)/height)/(2*height)
