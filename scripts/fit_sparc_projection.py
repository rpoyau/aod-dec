#!/usr/bin/env python3
"""Fit a finite 3D HI family to acquired HALOGAS moment maps; retain evidence."""
from __future__ import annotations
import argparse
from concurrent.futures import ThreadPoolExecutor
from dataclasses import asdict, replace
from fractions import Fraction
import hashlib
import json
import math
from pathlib import Path
import platform
import sys
import threading
import time

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0,str(ROOT))
from simulations.sparc_observations import load_halogas_observations
from simulations.sparc_projection_fit import Geometry, fit_at_geometry, volume_density, radial_areas, radial_enclosed_fractions, projected_basis


def hash_file(path):
    h=hashlib.sha256()
    with Path(path).open('rb') as f:
        for b in iter(lambda:f.read(1024*1024),b''): h.update(b)
    return h.hexdigest()


def summary(fit):
    result={k:v for k,v in fit.items() if k not in ('intensity','velocity')}
    result['geometry']=asdict(result['geometry'])
    for name in ('weights','rotation_coefficients_km_s'):result[name]=result[name].tolist()
    return result


def run_galaxy(name,args):
    import numpy as np
    start=time.monotonic()
    obs=load_halogas_observations(args.data,name,args.context)
    meta=obs.metadata
    stride=args.stride
    selection=(slice(stride//2,None,stride),slice(stride//2,None,stride))
    full_e,full_n=obs.east_arcsec[selection],obs.north_arcsec[selection]
    full_intensity,full_velocity=obs.intensity[selection],obs.velocity_km_s[selection]
    # Computational crop includes every nonzero observed intensity sample.
    # It does not change the loss mask or identify a physical outer boundary.
    emitting=np.isfinite(full_intensity)&(full_intensity!=0)
    iy,ix=np.nonzero(emitting)
    pixel_step=math.sqrt(meta['projected_pixel_area_arcsec2'])*stride
    margin=int(math.ceil(3*meta['beam_major_arcsec']/pixel_step))
    ymin,ymax=max(0,int(iy.min())-margin),min(full_e.shape[0],int(iy.max())+margin+1)
    xmin,xmax=max(0,int(ix.min())-margin),min(full_e.shape[1],int(ix.max())+margin+1)
    crop=(slice(ymin,ymax),slice(xmin,xmax))
    e,n=full_e[crop],full_n[crop]
    intensity,velocity=full_intensity[crop],full_velocity[crop]
    # Source zero-filled mask exteriors are unknown emission support, not
    # measured zero density. Keep all NONZERO finite intensity, including noise.
    fit_mask=np.isfinite(intensity)&(intensity!=0)
    vm=obs.joint_positive_mask[selection][crop]
    distance=float(Fraction(meta['distance_mpc_exact']))
    inc=float(Fraction(meta['inclination_deg_exact']))
    inc_error=float(Fraction(meta['inclination_error_deg_exact']))
    beam=meta['beam_major_arcsec']
    cov=meta['beam_covariance_east_north_arcsec2']
    beam_area=meta['beam_area_arcsec2']
    mass=float(Fraction(meta['catalog_hi_mass_1e9_msun_exact']))*1e9
    # Nearby, optically-thin HI mapping; this is an imported observation
    # conversion, NOT a new AOD dynamics law or coldens-header reinterpretation.
    total_flux=mass/(2.356e5*distance**2)
    seed=Geometry(*meta['initial_center_arcsec'],meta['initial_pa_deg_east_of_north'],inc,beam*.25)
    angle=math.radians(seed.pa_deg)
    major=(e-seed.center_east_arcsec)*math.sin(angle)+(n-seed.center_north_arcsec)*math.cos(angle)
    minor=(e-seed.center_east_arcsec)*math.cos(angle)-(n-seed.center_north_arcsec)*math.sin(angle)
    # Cover every sampled emitting location at the most inclined declared
    # geometry, then retain one beam of explicit finite-model radial padding.
    radii=np.hypot(major[vm],minor[vm]/math.cos(math.radians(inc+inc_error)))
    extent=float(np.max(radii))+beam
    ordering=np.argsort(radii)
    cumulative=np.cumsum(intensity[vm][ordering])
    concentration=float(radii[ordering[np.searchsorted(cumulative,.995*cumulative[-1])]])+beam
    if extent > 1.25*concentration:
        knots=np.r_[np.linspace(0,concentration,args.rings-1),(concentration+extent)/2,extent]
        radial_rule='dense uniform inner knots through 99.5 percent of positive selected flux radius plus one beam; two outer knots through furthest selected sample plus beam; no observations excluded'
    else:
        knots=np.linspace(0,extent,args.rings+1)
        radial_rule='uniform knots through all observed support plus beam; concentration radius already covers at least 80 percent of outer radius'
    history=[]
    cache={}
    output=Path(args.output)/name
    output.mkdir(parents=True,exist_ok=True)
    journal=output/'optimizer-evaluations.jsonl'
    journal.write_text('')
    def evaluate(g,label='search',order=args.quadrature):
        key=tuple(asdict(g).values())+(order,)
        if key not in cache:
            fit=fit_at_geometry(e,n,intensity,velocity,fit_mask,vm,knots,g,cov,beam_area,total_flux,quadrature_order=order)
            cache[key]=fit
            history.append({'evaluation':len(history),'role':label,**summary(fit)})
            with journal.open('a') as stream:
                stream.write(json.dumps(history[-1],allow_nan=False)+'\n')
            if len(history)%20==0:print(name+': '+str(len(history))+' evaluated geometries',flush=True)
        return cache[key]
    print(name+': initial projection',flush=True)
    initial=evaluate(seed,'initial_3d_geometry')
    # Profile the finite declared vertical/inclination family. Retain all runs,
    # including worse fits; selection uses the SAME two source masks and loss.
    profiles=[]
    for inclination in (inc-inc_error,inc,inc+inc_error):
        for height_factor in (.05,.1,.25,.5,1.,2.,4.):
            g=replace(seed,inclination_deg=inclination,height_arcsec=beam*height_factor)
            f=evaluate(g,'height_inclination_profile')
            profiles.append(summary(f))
    best=min(cache.values(),key=lambda f:f['score'])
    print(name+': profile selected, refining centre/orientation/height',flush=True)
    bounds={'center_east_arcsec':(seed.center_east_arcsec-2*beam,seed.center_east_arcsec+2*beam),
            'center_north_arcsec':(seed.center_north_arcsec-2*beam,seed.center_north_arcsec+2*beam),
            'pa_deg':(seed.pa_deg-12,seed.pa_deg+12),
            'inclination_deg':(inc-inc_error,inc+inc_error),
            'height_arcsec':(beam*.025,beam*8)}
    for scale in (1.,.5,.25,.125):
        for repeat in range(2):
            improved=False
            for field,step in (('center_east_arcsec',beam*.5*scale),('center_north_arcsec',beam*.5*scale),
                               ('pa_deg',4*scale),('inclination_deg',2*scale),('height_arcsec',None)):
                current=best['geometry']
                for direction in (-1,1):
                    val=getattr(current,field)
                    proposal=val*math.exp(direction*.7*scale) if step is None else val+direction*step
                    low,high=bounds[field]
                    proposal=max(low,min(high,proposal))
                    f=evaluate(replace(current,**{field:proposal}),'coordinate_refinement')
                    if f['score'] < best['score']-1e-12:
                        best=f
                        improved=True
            if not improved:break
    print(name+': numerical refinement and density export',flush=True)
    # Independent discretization change on the selected state, without fitting
    # new coefficients, measures numerical projection sensitivity directly.
    higher_order=min(128,args.quadrature*2)
    b2,c2=projected_basis(e,n,knots,best['geometry'],cov,beam_area,quadrature_order=higher_order)
    refined_image=b2@best['weights']
    refined_velocity=best['systemic_velocity_km_s']+(c2@(best['weights']*best['rotation_coefficients_km_s']))/np.maximum(refined_image,refined_image.max()*1e-12)
    numerical={'base_quadrature_order':args.quadrature,'check_quadrature_order':higher_order,
        'same_coefficients_no_refit':True,
        'intensity_relative_l2':float(np.linalg.norm((refined_image-best['intensity'])[fit_mask])/np.linalg.norm(best['intensity'][fit_mask])),
        'velocity_rmse_km_s':float(np.sqrt(np.mean((refined_velocity-best['velocity'])[vm]**2)))}
    # Keep a refitted higher-order result separate if quadrature changed enough.
    verified=evaluate(best['geometry'],'higher_order_refit',higher_order)
    # Verify the independent spatial discretization by nested half-stride
    # sampling at the same physical state and coefficients, without refitting.
    if stride%2==0:
        fine_stride=stride//2
        fine_selection=(slice(stride//2+ymin*stride,stride//2+(ymax-1)*stride+1,fine_stride),
                        slice(stride//2+xmin*stride,stride//2+(xmax-1)*stride+1,fine_stride))
        fine_e,fine_n=obs.east_arcsec[fine_selection],obs.north_arcsec[fine_selection]
        fb,fc=projected_basis(fine_e,fine_n,knots,verified['geometry'],cov,beam_area,quadrature_order=higher_order)
        fine_image=(fb@verified['weights'])[::2,::2]
        fine_numerator=(fc@(verified['weights']*verified['rotation_coefficients_km_s']))[::2,::2]
        fine_velocity=verified['systemic_velocity_km_s']+fine_numerator/np.maximum(fine_image,fine_image.max()*1e-12)
        numerical['spatial_refinement']={'coarse_stride':stride,'fine_stride':fine_stride,'same_coefficients_no_refit':True,
            'intensity_relative_l2':float(np.linalg.norm((fine_image-verified['intensity'])[fit_mask])/np.linalg.norm(verified['intensity'][fit_mask])),
            'velocity_rmse_km_s':float(np.sqrt(np.mean((fine_velocity-verified['velocity'])[vm]**2)))}
    else:
        numerical['spatial_refinement']={'status':'not checked; no even-stride nesting'}
    # Full source footprint accounting is distinct from the smaller fit view.
    full_b,_=projected_basis(full_e,full_n,knots,verified['geometry'],cov,beam_area,quadrature_order=higher_order)
    full_model_flux=float((full_b@verified['weights']).sum())*meta['projected_pixel_area_arcsec2']*stride**2/beam_area
    kpc_per_arcsec=distance*1000*math.pi/(180*3600)
    knots_kpc=knots*kpc_per_arcsec
    height_kpc=verified['geometry'].height_arcsec*kpc_per_arcsec
    inner_radius=min(concentration,extent)*kpc_per_arcsec
    radius=np.linspace(0,inner_radius,256)
    z=np.linspace(-6*height_kpc,6*height_kpc,241)
    density=volume_density(radius[None,:],z[:,None],knots_kpc,verified['weights'],height_kpc,mass)
    # A reusable Cartesian sample of the actual 3D inferred field, in addition
    # to its exact parametric representation. Sampling is not a new fit.
    xyz=np.linspace(-inner_radius,inner_radius,65)
    zs=np.linspace(-6*height_kpc,6*height_kpc,49)
    rr=np.hypot(xyz[:,None],xyz[None,:])
    density3d=volume_density(rr[None,:,:],zs[:,None,None],knots_kpc,verified['weights'],height_kpc,mass)
    inner_fraction=float(radial_enclosed_fractions(knots_kpc,float(inner_radius))@verified['weights']/verified['weights'].sum())
    np.savez_compressed(output/'reconstruction.npz',east_arcsec=e,north_arcsec=n,
        observed_intensity=intensity,observed_velocity_km_s=velocity,intensity_fit_mask=fit_mask,
        velocity_fit_mask=vm,initial_intensity=initial['intensity'],initial_velocity_km_s=initial['velocity'],
        fitted_intensity=verified['intensity'],fitted_velocity_km_s=verified['velocity'],
        radial_knots_kpc=knots_kpc,intrinsic_flux_weights=verified['weights'],
        radius_kpc=radius,z_kpc=z,density_msun_kpc3=density,
        cartesian_xy_kpc=xyz,cartesian_z_kpc=zs,cartesian_density_msun_kpc3=density3d)
    modeled_flux=verified['projected_grid_flux_jy_km_s']
    observed_flux=meta['integrated_signed_flux_jy_km_s']
    report={'schema':'aod-observational-3d-inverse-v1','galaxy':name,
        'scope':'inferred finite axisymmetric 3D HI density and circular mean velocity fitted through beam-projected moment maps',
        'observation_metadata':meta,'data_selection':{'spatial_stride_pixels':stride,'sampling':'source pixel centres; no averaging or missing-value imputation',
            'intensity_mask':'all sampled finite nonzero source moment0 values; includes negative noise, excludes zero-filled source-mask exterior',
            'velocity_mask':'sampled finite source moment1 with positive finite source moment0; fixed for every model',
            'source_sampled_full_shape':list(full_e.shape),'fit_grid_shape':list(e.shape),
            'fit_crop_source_sampled_rows':[ymin,ymax],'fit_crop_source_sampled_columns':[xmin,xmax],
            'crop_role':'computational view containing every selected observation plus margin; PSF halo rendered outside view; full source footprint flux separately checked',
            'masked_exterior_is_measured_empty':False,'full_spectral_cube_fitted':False},
        'model':{'radial_basis':'piecewise-linear hats with zero last endpoint, normalized by analytic 2*pi*integral R hat(R) dR',
            'radial_knots_arcsec':knots.tolist(),'radial_knots_kpc':knots_kpc.tolist(),
            'radial_knot_rule':radial_rule,'flux_concentration_fraction':.995,
            'concentration_radius_arcsec':concentration,'concentration_radius_kpc':inner_radius,
            'outlying_emission_membership':'unclassified observed components; retained in loss; outer model density is not an authenticated galactic halo',
            'vertical_profile':'exp(-abs(z)/h)/(2h), all real z; Gauss-Laguerre projection',
            'velocity_model':'basis-weighted circular streaming; fitted LOS mean; no equilibrium or time-transition law',
            'beam':'normalized elliptical Gaussian from source BMAJ/BMIN/BPA; render incoming six-sigma halo before zero-padded convolution and moment division',
            'total_hi_mass_msun':mass,'mass_constraint':'fixed catalog estimate, conditional; uncertainty unsupplied; not exact empirical truth or independent constraint',
            'total_galactic_mass_fitted':False,'stellar_or_bulge_density_fitted':False,
            'mass_flux_conversion':'M_HI/Msun=2.356e5*(D/Mpc)^2*(F/(Jy km/s)); nearby optically-thin mapping',
            'mass_flux_source':'https://egg.astro.cornell.edu/alfalfa/docs/alfalfaprop.pdf',
            'conversion_is_aod_dynamics':False,'geometry_bounds':bounds,
            'optimizer':'two-stage density-intensity and flux-weighted velocity least squares, followed by bounded geometry profile/coordinate search; no joint-global-minimum claim',
            'angle_and_distance_map':'spherical angular offsets mapped by small-angle Euclidean geometry; floating-point pi/trigonometry, not native octal coordinates'},
        'initial_fit':summary(initial),'selected_fit':summary(verified),'geometry_profile':profiles,
        'optimizer_history':history,'optimizer_history_is_physical_time':False,
        'projection_numerical_check':numerical,
        'boundary_accounting':{'intrinsic_total_flux_jy_km_s':total_flux,'source_footprint_flux_jy_km_s':observed_flux,
            'fit_view_model_flux_jy_km_s':modeled_flux,
            'full_sampled_grid_model_flux_jy_km_s':full_model_flux,
            'source_to_catalog_flux_ratio':observed_flux/total_flux,
            'grid_flux_deficit_fraction':1-full_model_flux/total_flux,
            'model_flux_outside_fit_view_inside_source_frame_jy_km_s':full_model_flux-modeled_flux,
            'grid_deficit_interpretation':'combines projection quadrature/pixel sampling and model emission outside grid; not a certified physical lost-mass measurement',
            'display_z_halfheight_kpc':6*height_kpc,'display_z_inside_mass_msun':mass*(1-math.exp(-6)),
            'display_z_above_mass_msun':mass*math.exp(-6)/2,'display_z_below_mass_msun':mass*math.exp(-6)/2,
            'dense_radial_view_radius_kpc':inner_radius,
            'dense_radial_view_mass_all_z_msun':mass*inner_fraction,
            'outside_dense_radial_view_mass_all_z_msun':mass*(1-inner_fraction),
            'dense_radial_and_vertical_view_mass_msun':mass*inner_fraction*(1-math.exp(-6)),
            'outside_dense_radial_and_vertical_view_mass_msun':mass*(1-inner_fraction*(1-math.exp(-6))),
            'radial_and_vertical_outside_masses_overlap_do_not_add':True,
            'radial_model_mass_outside_rmax':0,'real_radial_mass_outside_rmax':'unknown; finite model choice',
            'physical_sweep_or_time_history_inferred':False},
        'metric_definitions':{'score':'intensity NRMSE squared + intensity-weighted velocity RMSE squared / observed half 5th–95th velocity range squared',
            'intensity_nrmse':'sqrt(sum((model-data)^2)/sum(data^2)) on fixed intensity mask',
            'intensity_wape_fraction':'sum(abs(model-data))/sum(abs(data)) on fixed intensity mask; fraction, not percent or MAPE',
            'velocity_rmse_km_s':'unweighted root mean square LOS velocity residual on fixed velocity mask',
            'statistical_chi_square':False,'independent_prediction':False,'sparc_square_speed_mape_recomputed':False},
        'identifiability':'retained finite geometry alternatives are fits, not confidence intervals or proof of unique depth; optimizer is local bounded search',
        'unmodeled':['spiral/asymmetric density','warps','radially varying height','vertical/radial mean motion','line profile dispersion and multiple components','time-resolved moving boundaries','AOD native formation histories'],
        'elapsed_seconds':time.monotonic()-start,'worker_thread':threading.current_thread().name,
        'arrays_sha256':hash_file(output/'reconstruction.npz')}
    (output/'results.json').write_text(json.dumps(report,indent=2,allow_nan=False)+'\n')
    print(name+': done '+json.dumps({'initial_nrmse':initial['intensity_nrmse'],'fitted_nrmse':verified['intensity_nrmse'],
        'velocity_rmse':verified['velocity_rmse_km_s'],'height_kpc':height_kpc,'seconds':report['elapsed_seconds']}),flush=True)
    return report


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--data',required=True)
    parser.add_argument('--context',required=True)
    parser.add_argument('--output',required=True)
    parser.add_argument('--galaxies',nargs='+',choices=('NGC2403','NGC3198'),default=['NGC2403','NGC3198'])
    parser.add_argument('--workers',type=int,default=2)
    parser.add_argument('--stride',type=int,default=4)
    parser.add_argument('--rings',type=int,default=16)
    parser.add_argument('--quadrature',type=int,default=32)
    args=parser.parse_args()
    if not 1<=args.workers<=8 or not 1<=args.stride<=16 or not 4<=args.rings<=64 or not 4<=args.quadrature<=64:
        parser.error('workers1..8, stride1..16, rings4..64, quadrature4..64 required')
    if len(set(args.galaxies))!=len(args.galaxies):parser.error('duplicate galaxy')
    Path(args.output).mkdir(parents=True,exist_ok=True)
    source_paths=[Path(__file__),ROOT/'simulations/sparc_projection_fit.py',ROOT/'simulations/sparc_observations.py']
    sources={str(p.relative_to(ROOT)):hash_file(p) for p in source_paths}
    start=time.monotonic()
    with ThreadPoolExecutor(max_workers=args.workers,thread_name_prefix='projection-fit') as pool:
        results=list(pool.map(lambda name:run_galaxy(name,args),args.galaxies))
    import numpy as np
    if sources != {str(p.relative_to(ROOT)):hash_file(p) for p in source_paths}:
        raise RuntimeError('fit source changed during execution; run is not reproducibly bound')
    run={'scope':'observational 3D HI inverse fit','python':platform.python_version(),'numpy':np.__version__,
         'workers_requested':args.workers,'worker_threads':sorted({r['worker_thread'] for r in results}),
         'backend':'NumPy CPU; independent galaxy fits in threads','gpu_used':False,'source_sha256':sources,
         'elapsed_seconds':time.monotonic()-start,
         'galaxies':[{ 'galaxy':r['galaxy'],'initial':r['initial_fit'],'fitted':r['selected_fit'],
                      'numerical_check':r['projection_numerical_check']} for r in results]}
    (Path(args.output)/'summary.json').write_text(json.dumps(run,indent=2,allow_nan=False)+'\n')


if __name__=='__main__':main()
