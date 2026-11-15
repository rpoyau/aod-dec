"""Independent analytic and hostile controls for the optional 3D projection fit."""
import math
import unittest

try:
    import numpy as np
except ImportError:
    np = None

from simulations import sparc_projection_fit as model


@unittest.skipIf(np is None, 'optional NumPy is not installed')
class ProjectionFitTests(unittest.TestCase):
    @staticmethod
    def grid(extent=12., step=.5):
        values = np.arange(-extent, extent + step / 2, step)
        return np.meshgrid(values, values)

    @staticmethod
    def two_hat_columns(radius, a=4., b=8.):
        # Explicit independent piecewise definition, with fixed zero outer node.
        first = np.where(radius <= a, 1 - radius / a, 0.)
        second = np.where(radius <= a, radius / a,
                          np.where(radius <= b, (b-radius)/(b-a), 0.))
        return np.stack((first/(math.pi*a*a/3),
                         second/(2*math.pi*a*a)), axis=-1)

    def test_cylindrical_hat_areas_have_independent_polynomial_answers(self):
        # A triangular radial basis must integrate against 2*pi*r, not dr.
        np.testing.assert_allclose(model.radial_areas([0., 1., 2.]),
                                   [math.pi/3, 2*math.pi], rtol=2e-15)
        # For the [0,2,5] triangle, area=5/2 and centroid radius=7/3.
        np.testing.assert_allclose(model.radial_areas([0., 2., 5.]),
                                   [4*math.pi/3, 35*math.pi/3], rtol=2e-15)
        np.testing.assert_allclose(model.radial_enclosed_fractions([0.,1.,2.],.5),
                                   [.5,1/24],rtol=2e-15)
        np.testing.assert_allclose(model.radial_enclosed_fractions([0.,1.,2.],1.),
                                   [1.,1/3],rtol=2e-15)
        np.testing.assert_allclose(model.radial_enclosed_fractions([0.,1.,2.],100.),
                                   [1.,1.],rtol=2e-15)
        for knots in ([1.,2.,3.], [0.,1.,1.], [0.,math.nan,2.]):
            with self.assertRaises(ValueError):
                model.radial_areas(knots)

    def test_volume_density_normalization_and_vertical_tail(self):
        r = np.linspace(0., 8., 8001)
        h, mass = .7, 1234.
        # Integrating rho(r,0)*2h over annuli accounts for the exact full z tail.
        midplane = model.volume_density(r, 0., [0.,4.,8.], [2.,3.], h, mass)
        integrand = midplane*2*h*2*math.pi*r
        measured = float(np.sum((integrand[1:]+integrand[:-1])*np.diff(r)/2))
        self.assertAlmostEqual(measured/mass, 1., delta=5e-8)
        central = model.volume_density(0., 0., [0.,4.,8.], [2.,3.], h, mass)
        at_height = model.volume_density(0., h, [0.,4.,8.], [2.,3.], h, mass)
        self.assertAlmostEqual(float(at_height/central), math.exp(-1), delta=1e-15)
        self.assertGreater(float(model.volume_density(0., 20*h, [0.,4.,8.], [2.,3.], h, mass)), 0.)
        self.assertEqual(float(model.volume_density(8.01, 0., [0.,4.,8.], [2.,3.], h, mass)), 0.)
        with self.assertRaises(ValueError):
            model.volume_density(0.,0.,[0.,4.,8.],[2.,-3.],h,mass)

    def test_face_on_height_degeneracy_and_exact_column(self):
        east, north = self.grid()
        expected = self.two_hat_columns(np.hypot(east, north))
        images = []
        for height in (.01, 1., 100.):
            actual, current = model.projected_basis(east,north,[0.,4.,8.],
                model.Geometry(0.,0.,90.,0.,height), np.zeros((2,2)),1.,quadrature_order=16)
            np.testing.assert_allclose(actual, expected, atol=1e-15, rtol=1e-13)
            np.testing.assert_allclose(current, 0., atol=0., rtol=0.)
            images.append(actual)
        np.testing.assert_array_equal(images[0], images[2])

    def test_inclined_projection_mass_and_covariance(self):
        east, north = self.grid(extent=24., step=.2)
        cov = np.array([[.25,.1],[.1,.5]])
        h, inc = 1.5, 60.
        image, current = model.projected_basis(east,north,[0.,8.,16.],
            model.Geometry(0.,0.,90.,inc,h),cov,1.,quadrature_order=64)
        full_intensity_max = float(image.max())
        image = image[...,0]  # radial triangle 1-r/8, normalized.
        norm = image.sum()
        self.assertAlmostEqual(float(norm*.2**2),1.,delta=.003)
        # E[R²]=3a²/10; E[z²]=2h². Rotation and convolution add covariance.
        intrinsic = 3*8**2/20
        expected_e = intrinsic + cov[0,0]
        expected_n = intrinsic*.25 + 2*h*h*.75 + cov[1,1]
        self.assertAlmostEqual(float((image*east**2).sum()/norm),expected_e,delta=.035)
        self.assertAlmostEqual(float((image*north**2).sum()/norm),expected_n,delta=.035)
        self.assertAlmostEqual(float((image*east*north).sum()/norm),cov[0,1],delta=.01)
        self.assertLess(float(abs(current).max()),full_intensity_max)

    def test_beam_retains_centroid_and_does_not_wrap_lost_flux(self):
        arr = np.zeros((81,81)); arr[40,40] = 1.
        cov = np.array([[1.,.3],[.3,2.]])
        result = model.beam_convolve(arr,cov)
        north,east = np.indices(arr.shape)-40
        self.assertAlmostEqual(float(result.sum()),1.,delta=1e-12)
        self.assertAlmostEqual(float((result*east).sum()),0.,delta=1e-12)
        self.assertAlmostEqual(float((result*north).sum()),0.,delta=1e-12)
        self.assertAlmostEqual(float((result*east**2).sum()),1.,delta=1e-6)
        self.assertAlmostEqual(float((result*north**2).sum()),2.,delta=1e-6)
        self.assertAlmostEqual(float((result*east*north).sum()),.3,delta=1e-6)
        arr[:] = 0.; arr[0,0]=1.
        edge = model.beam_convolve(arr,cov)
        self.assertGreater(float(edge.sum()),0.)
        self.assertLess(float(edge.sum()),.75)
        self.assertLess(float(abs(edge[-4:,-4:]).max()),1e-14)

    def test_projection_renders_incoming_beam_flux_across_grid_boundary(self):
        full_e,full_n=self.grid(extent=12.,step=.5)
        local_e,local_n=self.grid(extent=2.,step=.5)
        geometry=model.Geometry(0.,0.,90.,0.,1.)
        covariance=np.array([[1.,.2],[.2,1.]])
        full,_=model.projected_basis(full_e,full_n,[0.,4.,8.],geometry,covariance,1.)
        local,_=model.projected_basis(local_e,local_n,[0.,4.,8.],geometry,covariance,1.)
        center=full.shape[0]//2
        half=local.shape[0]//2
        expected=full[center-half:center+half+1,center-half:center+half+1]
        # Material outside a requested window can scatter into it through the
        # beam. Cropping the rendered image first would lose that contribution.
        np.testing.assert_allclose(local,expected,atol=2e-14,rtol=1e-12)

    def test_simplex_and_box_fits_match_independent_constrained_minima(self):
        weights,audit = model.simplex_least_squares(np.diag([2.,1.]),[.2,.8])
        # Differentiate (2*x-.2)^2+(.2-x)^2: optimum x=.12.
        np.testing.assert_allclose(weights,[.12,.88],atol=1e-13,rtol=0.)
        self.assertTrue(audit['converged'])
        weights,_ = model.simplex_least_squares(np.eye(3),[2.,-1.,.5])
        np.testing.assert_allclose(weights,[1.,0.,0.],atol=1e-13,rtol=0.)
        fitted,_ = model.bounded_least_squares(np.eye(3),[-1.,2.,.5],np.zeros(3),np.ones(3))
        np.testing.assert_allclose(fitted,[0.,1.,.5],atol=1e-13,rtol=0.)

    def test_velocity_fit_rejects_zero_observational_weight(self):
        east,north = self.grid(extent=2.,step=1.)
        mask = np.ones_like(east,dtype=bool)
        # Finite negative noise is valid intensity data, but cannot provide the
        # declared positive intensity weights for this velocity-loss choice.
        with self.assertRaises(ValueError):
            model.fit_at_geometry(east,north,-np.ones_like(east),np.full_like(east,30.),
                mask,mask,[0.,4.,8.],model.Geometry(0.,0.,90.,30.,1.),
                np.zeros((2,2)),1.,2.)

    def test_nonfinite_velocity_weight_is_rejected_even_outside_intensity_mask(self):
        east,north = self.grid(extent=2.,step=1.)
        intensity = np.ones_like(east)
        fit_mask = np.ones_like(east,dtype=bool)
        fit_mask[0,0]=False
        velocity_mask = np.ones_like(east,dtype=bool)
        intensity[0,0]=np.nan
        with self.assertRaises(ValueError):
            model.fit_at_geometry(east,north,intensity,np.full_like(east,30.),
                fit_mask,velocity_mask,[0.,4.,8.],model.Geometry(0.,0.,90.,30.,1.),
                np.zeros((2,2)),1.,2.)

    def test_incompatible_observed_support_is_scored_not_dropped(self):
        east,north = self.grid()
        mask = np.hypot(east,north)>9.
        # A compact face-on model ending at R=8 cannot fit observed emission
        # beyond R=9. The fixed mask must not disappear when model support does.
        result=model.fit_at_geometry(east,north,np.ones_like(east),np.full_like(east,30.),
            mask,mask,[0.,4.,8.],model.Geometry(0.,0.,90.,0.,1.),
            np.zeros((2,2)),1.,2.)
        self.assertAlmostEqual(result['intensity_nrmse'],1.,delta=1e-14)
        self.assertAlmostEqual(result['score'],1.,delta=1e-14)
        self.assertEqual(result['velocity_denominator_floor_count'],int(mask.sum()))
        self.assertEqual(result['intensity_sample_count'],int(mask.sum()))
        self.assertEqual(result['velocity_sample_count'],int(mask.sum()))

    def test_known_thin_projection_recovers_forward_values_and_mask_is_respected(self):
        east,north = self.grid()
        inc = math.pi/3
        radius = np.hypot(east,north/math.cos(inc))
        columns = self.two_hat_columns(radius)/math.cos(inc)
        weights = np.array([2.,4.]); speeds = np.array([50.,100.])
        intensity = columns@weights
        angular = np.divide(math.sin(inc)*east,radius,out=np.zeros_like(radius),where=radius>0)
        current = angular*(columns@(weights*speeds))
        mask = intensity>1e-10
        velocity = np.full_like(intensity,np.nan)
        velocity[mask] = 83.+current[mask]/intensity[mask]
        intensity[~mask] = np.nan
        args = (east,north,intensity,velocity,mask,mask,[0.,4.,8.],
                model.Geometry(0.,0.,90.,60.,1e-7),np.zeros((2,2)),1.,6.)
        result = model.fit_at_geometry(*args,quadrature_order=16)
        np.testing.assert_allclose(result['weights'],weights,rtol=1e-7,atol=1e-7)
        np.testing.assert_allclose(result['rotation_coefficients_km_s'],speeds,rtol=1e-6,atol=1e-6)
        self.assertAlmostEqual(result['systemic_velocity_km_s'],83.,delta=1e-7)
        self.assertLess(result['intensity_nrmse'],1e-7)
        self.assertLess(result['velocity_rmse_km_s'],1e-6)
        self.assertEqual(result['intensity_sample_count'],int(mask.sum()))
        # Excluded observations can contain large values, not an imputed zero.
        intensity[~mask]=1e20; velocity[~mask]=-1e20
        other = model.fit_at_geometry(*args,quadrature_order=16)
        np.testing.assert_array_equal(result['weights'],other['weights'])
        np.testing.assert_array_equal(result['velocity'],other['velocity'])
        bad = intensity.copy(); bad.flat[np.flatnonzero(mask)[0]]=np.nan
        with self.assertRaises(ValueError):
            model.fit_at_geometry(east,north,bad,velocity,mask,mask,[0.,4.,8.],
                model.Geometry(0.,0.,90.,60.,1e-7),np.zeros((2,2)),1.,6.)


if __name__ == '__main__':
    unittest.main()
