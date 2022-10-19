//! Least squares

use crate::{error::*, layout::*, *};
use cauchy::*;
use num_traits::{ToPrimitive, Zero};

/// Result of LeastSquares
pub struct LeastSquaresOwned<A: Scalar> {
    /// singular values
    pub singular_values: Vec<A::Real>,
    /// The rank of the input matrix A
    pub rank: i32,
}

/// Result of LeastSquares
pub struct LeastSquaresRef<'work, A: Scalar> {
    /// singular values
    pub singular_values: &'work [A::Real],
    /// The rank of the input matrix A
    pub rank: i32,
}

pub struct LeastSquaresWork<T: Scalar> {
    pub a_layout: MatrixLayout,
    pub b_layout: MatrixLayout,
    pub singular_values: Vec<MaybeUninit<T::Real>>,
    pub work: Vec<MaybeUninit<T>>,
    pub iwork: Vec<MaybeUninit<i32>>,
    pub rwork: Option<Vec<MaybeUninit<T::Real>>>,
}

pub trait LeastSquaresWorkImpl: Sized {
    type Elem: Scalar;
    fn new(a_layout: MatrixLayout, b_layout: MatrixLayout) -> Result<Self>;
    fn calc(
        &mut self,
        a: &mut [Self::Elem],
        b: &mut [Self::Elem],
    ) -> Result<LeastSquaresRef<Self::Elem>>;
    fn eval(
        self,
        a: &mut [Self::Elem],
        b: &mut [Self::Elem],
    ) -> Result<LeastSquaresOwned<Self::Elem>>;
}

macro_rules! impl_least_squares_work_c {
    ($c:ty, $lsd:path) => {
        impl LeastSquaresWorkImpl for LeastSquaresWork<$c> {
            type Elem = $c;

            fn new(a_layout: MatrixLayout, b_layout: MatrixLayout) -> Result<Self> {
                let (m, n) = a_layout.size();
                let (m_, nrhs) = b_layout.size();
                let k = m.min(n);
                assert!(m_ >= m);

                let rcond = -1.;
                let mut singular_values = vec_uninit(k as usize);
                let mut rank: i32 = 0;

                // eval work size
                let mut info = 0;
                let mut work_size = [Self::Elem::zero()];
                let mut iwork_size = [0];
                let mut rwork = [<Self::Elem as Scalar>::Real::zero()];
                unsafe {
                    $lsd(
                        &m,
                        &n,
                        &nrhs,
                        std::ptr::null_mut(),
                        &m,
                        std::ptr::null_mut(),
                        &m_,
                        AsPtr::as_mut_ptr(&mut singular_values),
                        &rcond,
                        &mut rank,
                        AsPtr::as_mut_ptr(&mut work_size),
                        &(-1),
                        AsPtr::as_mut_ptr(&mut rwork),
                        iwork_size.as_mut_ptr(),
                        &mut info,
                    )
                };
                info.as_lapack_result()?;

                let lwork = work_size[0].to_usize().unwrap();
                let liwork = iwork_size[0].to_usize().unwrap();
                let lrwork = rwork[0].to_usize().unwrap();

                let work = vec_uninit(lwork);
                let iwork = vec_uninit(liwork);
                let rwork = vec_uninit(lrwork);

                Ok(LeastSquaresWork {
                    a_layout,
                    b_layout,
                    work,
                    iwork,
                    rwork: Some(rwork),
                    singular_values,
                })
            }

            fn calc(
                &mut self,
                a: &mut [Self::Elem],
                b: &mut [Self::Elem],
            ) -> Result<LeastSquaresRef<Self::Elem>> {
                let (m, n) = self.a_layout.size();
                let (m_, nrhs) = self.b_layout.size();
                assert!(m_ >= m);

                let lwork = self.work.len().to_i32().unwrap();

                // Transpose if a is C-continuous
                let mut a_t = None;
                let _ = match self.a_layout {
                    MatrixLayout::C { .. } => {
                        let (layout, t) = transpose(self.a_layout, a);
                        a_t = Some(t);
                        layout
                    }
                    MatrixLayout::F { .. } => self.a_layout,
                };

                // Transpose if b is C-continuous
                let mut b_t = None;
                let b_layout = match self.b_layout {
                    MatrixLayout::C { .. } => {
                        let (layout, t) = transpose(self.b_layout, b);
                        b_t = Some(t);
                        layout
                    }
                    MatrixLayout::F { .. } => self.b_layout,
                };

                let rcond: <Self::Elem as Scalar>::Real = -1.;
                let mut rank: i32 = 0;

                let mut info = 0;
                unsafe {
                    $lsd(
                        &m,
                        &n,
                        &nrhs,
                        AsPtr::as_mut_ptr(a_t.as_mut().map(|v| v.as_mut_slice()).unwrap_or(a)),
                        &m,
                        AsPtr::as_mut_ptr(b_t.as_mut().map(|v| v.as_mut_slice()).unwrap_or(b)),
                        &m_,
                        AsPtr::as_mut_ptr(&mut self.singular_values),
                        &rcond,
                        &mut rank,
                        AsPtr::as_mut_ptr(&mut self.work),
                        &lwork,
                        AsPtr::as_mut_ptr(self.rwork.as_mut().unwrap()),
                        AsPtr::as_mut_ptr(&mut self.iwork),
                        &mut info,
                    );
                }
                info.as_lapack_result()?;

                let singular_values = unsafe { self.singular_values.slice_assume_init_ref() };

                // Skip a_t -> a transpose because A has been destroyed
                // Re-transpose b
                if let Some(b_t) = b_t {
                    transpose_over(b_layout, &b_t, b);
                }

                Ok(LeastSquaresRef {
                    singular_values,
                    rank,
                })
            }

            fn eval(
                mut self,
                a: &mut [Self::Elem],
                b: &mut [Self::Elem],
            ) -> Result<LeastSquaresOwned<Self::Elem>> {
                let LeastSquaresRef { rank, .. } = self.calc(a, b)?;
                let singular_values = unsafe { self.singular_values.assume_init() };
                Ok(LeastSquaresOwned {
                    singular_values,
                    rank,
                })
            }
        }
    };
}
impl_least_squares_work_c!(c64, lapack_sys::zgelsd_);
impl_least_squares_work_c!(c32, lapack_sys::cgelsd_);

macro_rules! impl_least_squares_work_r {
    ($c:ty, $lsd:path) => {
        impl LeastSquaresWorkImpl for LeastSquaresWork<$c> {
            type Elem = $c;

            fn new(a_layout: MatrixLayout, b_layout: MatrixLayout) -> Result<Self> {
                let (m, n) = a_layout.size();
                let (m_, nrhs) = b_layout.size();
                let k = m.min(n);
                assert!(m_ >= m);

                let rcond = -1.;
                let mut singular_values = vec_uninit(k as usize);
                let mut rank: i32 = 0;

                // eval work size
                let mut info = 0;
                let mut work_size = [Self::Elem::zero()];
                let mut iwork_size = [0];
                unsafe {
                    $lsd(
                        &m,
                        &n,
                        &nrhs,
                        std::ptr::null_mut(),
                        &m,
                        std::ptr::null_mut(),
                        &m_,
                        AsPtr::as_mut_ptr(&mut singular_values),
                        &rcond,
                        &mut rank,
                        AsPtr::as_mut_ptr(&mut work_size),
                        &(-1),
                        iwork_size.as_mut_ptr(),
                        &mut info,
                    )
                };
                info.as_lapack_result()?;

                let lwork = work_size[0].to_usize().unwrap();
                let liwork = iwork_size[0].to_usize().unwrap();

                let work = vec_uninit(lwork);
                let iwork = vec_uninit(liwork);

                Ok(LeastSquaresWork {
                    a_layout,
                    b_layout,
                    work,
                    iwork,
                    rwork: None,
                    singular_values,
                })
            }

            fn calc(
                &mut self,
                a: &mut [Self::Elem],
                b: &mut [Self::Elem],
            ) -> Result<LeastSquaresRef<Self::Elem>> {
                let (m, n) = self.a_layout.size();
                let (m_, nrhs) = self.b_layout.size();
                assert!(m_ >= m);

                let lwork = self.work.len().to_i32().unwrap();

                // Transpose if a is C-continuous
                let mut a_t = None;
                let _ = match self.a_layout {
                    MatrixLayout::C { .. } => {
                        let (layout, t) = transpose(self.a_layout, a);
                        a_t = Some(t);
                        layout
                    }
                    MatrixLayout::F { .. } => self.a_layout,
                };

                // Transpose if b is C-continuous
                let mut b_t = None;
                let b_layout = match self.b_layout {
                    MatrixLayout::C { .. } => {
                        let (layout, t) = transpose(self.b_layout, b);
                        b_t = Some(t);
                        layout
                    }
                    MatrixLayout::F { .. } => self.b_layout,
                };

                let rcond: <Self::Elem as Scalar>::Real = -1.;
                let mut rank: i32 = 0;

                let mut info = 0;
                unsafe {
                    $lsd(
                        &m,
                        &n,
                        &nrhs,
                        AsPtr::as_mut_ptr(a_t.as_mut().map(|v| v.as_mut_slice()).unwrap_or(a)),
                        &m,
                        AsPtr::as_mut_ptr(b_t.as_mut().map(|v| v.as_mut_slice()).unwrap_or(b)),
                        &m_,
                        AsPtr::as_mut_ptr(&mut self.singular_values),
                        &rcond,
                        &mut rank,
                        AsPtr::as_mut_ptr(&mut self.work),
                        &lwork,
                        AsPtr::as_mut_ptr(&mut self.iwork),
                        &mut info,
                    );
                }
                info.as_lapack_result()?;

                let singular_values = unsafe { self.singular_values.slice_assume_init_ref() };

                // Skip a_t -> a transpose because A has been destroyed
                // Re-transpose b
                if let Some(b_t) = b_t {
                    transpose_over(b_layout, &b_t, b);
                }

                Ok(LeastSquaresRef {
                    singular_values,
                    rank,
                })
            }

            fn eval(
                mut self,
                a: &mut [Self::Elem],
                b: &mut [Self::Elem],
            ) -> Result<LeastSquaresOwned<Self::Elem>> {
                let LeastSquaresRef { rank, .. } = self.calc(a, b)?;
                let singular_values = unsafe { self.singular_values.assume_init() };
                Ok(LeastSquaresOwned {
                    singular_values,
                    rank,
                })
            }
        }
    };
}
impl_least_squares_work_r!(f64, lapack_sys::dgelsd_);
impl_least_squares_work_r!(f32, lapack_sys::sgelsd_);
