//! Eigenvalue decomposition for general matrices

use crate::{error::*, layout::MatrixLayout, *};
use cauchy::*;
use num_traits::{ToPrimitive, Zero};

/// Wraps `*geev` for general matrices
pub trait Eig_: Scalar {
    /// Calculate Right eigenvalue
    fn eig(
        calc_v: bool,
        l: MatrixLayout,
        a: &mut [Self],
    ) -> Result<(Vec<Self::Complex>, Vec<Self::Complex>)>;
}

macro_rules! impl_eig_complex {
    ($scalar:ty, $ev:path) => {
        impl Eig_ for $scalar {
            fn eig(
                calc_v: bool,
                l: MatrixLayout,
                a: &mut [Self],
            ) -> Result<(Vec<Self::Complex>, Vec<Self::Complex>)> {
                let (n, _) = l.size();
                // LAPACK assumes a column-major input. A row-major input can
                // be interpreted as the transpose of a column-major input. So,
                // for row-major inputs, we we want to solve the following,
                // given the column-major input `A`:
                //
                //   A^T V = V Λ ⟺ V^T A = Λ V^T ⟺ conj(V)^H A = Λ conj(V)^H
                //
                // So, in this case, the right eigenvectors are the conjugates
                // of the left eigenvectors computed with `A`, and the
                // eigenvalues are the eigenvalues computed with `A`.
                let (jobvl, jobvr) = if calc_v {
                    match l {
                        MatrixLayout::C { .. } => (JobEv::Calc, JobEv::Not),
                        MatrixLayout::F { .. } => (JobEv::Not, JobEv::Calc),
                    }
                } else {
                    (JobEv::Not, JobEv::Not)
                };
                let mut eigs: Vec<MaybeUninit<Self>> = unsafe { vec_uninit(n as usize) };
                let mut rwork: Vec<MaybeUninit<Self::Real>> = unsafe { vec_uninit(2 * n as usize) };

                let mut vl: Option<Vec<MaybeUninit<Self>>> =
                    jobvl.then(|| unsafe { vec_uninit((n * n) as usize) });
                let mut vr: Option<Vec<MaybeUninit<Self>>> =
                    jobvr.then(|| unsafe { vec_uninit((n * n) as usize) });

                // calc work size
                let mut info = 0;
                let mut work_size = [Self::zero()];
                unsafe {
                    $ev(
                        jobvl.as_ptr(),
                        jobvr.as_ptr(),
                        &n,
                        AsPtr::as_mut_ptr(a),
                        &n,
                        AsPtr::as_mut_ptr(&mut eigs),
                        AsPtr::as_mut_ptr(vl.as_mut().map(|v| v.as_mut_slice()).unwrap_or(&mut [])),
                        &n,
                        AsPtr::as_mut_ptr(vr.as_mut().map(|v| v.as_mut_slice()).unwrap_or(&mut [])),
                        &n,
                        AsPtr::as_mut_ptr(&mut work_size),
                        &(-1),
                        AsPtr::as_mut_ptr(&mut rwork),
                        &mut info,
                    )
                };
                info.as_lapack_result()?;

                // actal ev
                let lwork = work_size[0].to_usize().unwrap();
                let mut work: Vec<MaybeUninit<Self>> = unsafe { vec_uninit(lwork) };
                let lwork = lwork as i32;
                unsafe {
                    $ev(
                        jobvl.as_ptr(),
                        jobvr.as_ptr(),
                        &n,
                        AsPtr::as_mut_ptr(a),
                        &n,
                        AsPtr::as_mut_ptr(&mut eigs),
                        AsPtr::as_mut_ptr(vl.as_mut().map(|v| v.as_mut_slice()).unwrap_or(&mut [])),
                        &n,
                        AsPtr::as_mut_ptr(vr.as_mut().map(|v| v.as_mut_slice()).unwrap_or(&mut [])),
                        &n,
                        AsPtr::as_mut_ptr(&mut work),
                        &lwork,
                        AsPtr::as_mut_ptr(&mut rwork),
                        &mut info,
                    )
                };
                info.as_lapack_result()?;

                let eigs = unsafe { eigs.assume_init() };
                let vr = unsafe { vr.map(|v| v.assume_init()) };
                let mut vl = unsafe { vl.map(|v| v.assume_init()) };

                // Hermite conjugate
                if jobvl.is_calc() {
                    for c in vl.as_mut().unwrap().iter_mut() {
                        c.im = -c.im;
                    }
                }

                Ok((eigs, vr.or(vl).unwrap_or(Vec::new())))
            }
        }
    };
}

impl_eig_complex!(c64, lapack_sys::zgeev_);
impl_eig_complex!(c32, lapack_sys::cgeev_);

macro_rules! impl_eig_real {
    ($scalar:ty, $ev:path) => {
        impl Eig_ for $scalar {
            fn eig(
                calc_v: bool,
                l: MatrixLayout,
                a: &mut [Self],
            ) -> Result<(Vec<Self::Complex>, Vec<Self::Complex>)> {
                let (n, _) = l.size();
                // LAPACK assumes a column-major input. A row-major input can
                // be interpreted as the transpose of a column-major input. So,
                // for row-major inputs, we we want to solve the following,
                // given the column-major input `A`:
                //
                //   A^T V = V Λ ⟺ V^T A = Λ V^T ⟺ conj(V)^H A = Λ conj(V)^H
                //
                // So, in this case, the right eigenvectors are the conjugates
                // of the left eigenvectors computed with `A`, and the
                // eigenvalues are the eigenvalues computed with `A`.
                //
                // We could conjugate the eigenvalues instead of the
                // eigenvectors, but we have to reconstruct the eigenvectors
                // into new matrices anyway, and by not modifying the
                // eigenvalues, we preserve the nice ordering specified by
                // `sgeev`/`dgeev`.
                let (jobvl, jobvr) = if calc_v {
                    match l {
                        MatrixLayout::C { .. } => (JobEv::Calc, JobEv::Not),
                        MatrixLayout::F { .. } => (JobEv::Not, JobEv::Calc),
                    }
                } else {
                    (JobEv::Not, JobEv::Not)
                };
                let mut eig_re: Vec<MaybeUninit<Self>> = unsafe { vec_uninit(n as usize) };
                let mut eig_im: Vec<MaybeUninit<Self>> = unsafe { vec_uninit(n as usize) };

                let mut vl: Option<Vec<MaybeUninit<Self>>> =
                    jobvl.then(|| unsafe { vec_uninit((n * n) as usize) });
                let mut vr: Option<Vec<MaybeUninit<Self>>> =
                    jobvr.then(|| unsafe { vec_uninit((n * n) as usize) });

                // calc work size
                let mut info = 0;
                let mut work_size: [Self; 1] = [0.0];
                unsafe {
                    $ev(
                        jobvl.as_ptr(),
                        jobvr.as_ptr(),
                        &n,
                        AsPtr::as_mut_ptr(a),
                        &n,
                        AsPtr::as_mut_ptr(&mut eig_re),
                        AsPtr::as_mut_ptr(&mut eig_im),
                        AsPtr::as_mut_ptr(vl.as_mut().map(|v| v.as_mut_slice()).unwrap_or(&mut [])),
                        &n,
                        AsPtr::as_mut_ptr(vr.as_mut().map(|v| v.as_mut_slice()).unwrap_or(&mut [])),
                        &n,
                        AsPtr::as_mut_ptr(&mut work_size),
                        &(-1),
                        &mut info,
                    )
                };
                info.as_lapack_result()?;

                // actual ev
                let lwork = work_size[0].to_usize().unwrap();
                let mut work: Vec<MaybeUninit<Self>> = unsafe { vec_uninit(lwork) };
                let lwork = lwork as i32;
                unsafe {
                    $ev(
                        jobvl.as_ptr(),
                        jobvr.as_ptr(),
                        &n,
                        AsPtr::as_mut_ptr(a),
                        &n,
                        AsPtr::as_mut_ptr(&mut eig_re),
                        AsPtr::as_mut_ptr(&mut eig_im),
                        AsPtr::as_mut_ptr(vl.as_mut().map(|v| v.as_mut_slice()).unwrap_or(&mut [])),
                        &n,
                        AsPtr::as_mut_ptr(vr.as_mut().map(|v| v.as_mut_slice()).unwrap_or(&mut [])),
                        &n,
                        AsPtr::as_mut_ptr(&mut work),
                        &lwork,
                        &mut info,
                    )
                };
                info.as_lapack_result()?;

                let eig_re = unsafe { eig_re.assume_init() };
                let eig_im = unsafe { eig_im.assume_init() };
                let vl = unsafe { vl.map(|v| v.assume_init()) };
                let vr = unsafe { vr.map(|v| v.assume_init()) };

                // reconstruct eigenvalues
                let eigs: Vec<Self::Complex> = eig_re
                    .iter()
                    .zip(eig_im.iter())
                    .map(|(&re, &im)| Self::complex(re, im))
                    .collect();

                if !calc_v {
                    return Ok((eigs, Vec::new()));
                }

                // Reconstruct eigenvectors into complex-array
                // --------------------------------------------
                //
                // From LAPACK API https://software.intel.com/en-us/node/469230
                //
                // - If the j-th eigenvalue is real,
                //   - v(j) = VR(:,j), the j-th column of VR.
                //
                // - If the j-th and (j+1)-st eigenvalues form a complex conjugate pair,
                //   - v(j)   = VR(:,j) + i*VR(:,j+1)
                //   - v(j+1) = VR(:,j) - i*VR(:,j+1).
                //
                // In the C-layout case, we need the conjugates of the left
                // eigenvectors, so the signs should be reversed.

                let n = n as usize;
                let v = vr.or(vl).unwrap();
                let mut eigvecs: Vec<MaybeUninit<Self::Complex>> = unsafe { vec_uninit(n * n) };
                let mut col = 0;
                while col < n {
                    if eig_im[col] == 0. {
                        // The corresponding eigenvalue is real.
                        for row in 0..n {
                            let re = v[row + col * n];
                            eigvecs[row + col * n].write(Self::complex(re, 0.));
                        }
                        col += 1;
                    } else {
                        // This is a complex conjugate pair.
                        assert!(col + 1 < n);
                        for row in 0..n {
                            let re = v[row + col * n];
                            let mut im = v[row + (col + 1) * n];
                            if jobvl.is_calc() {
                                im = -im;
                            }
                            eigvecs[row + col * n].write(Self::complex(re, im));
                            eigvecs[row + (col + 1) * n].write(Self::complex(re, -im));
                        }
                        col += 2;
                    }
                }
                let eigvecs = unsafe { eigvecs.assume_init() };

                Ok((eigs, eigvecs))
            }
        }
    };
}

impl_eig_real!(f64, lapack_sys::dgeev_);
impl_eig_real!(f32, lapack_sys::sgeev_);
