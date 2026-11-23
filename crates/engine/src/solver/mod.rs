//! Linear system assembly and solution.
//!
//! The MNA system is assembled in triplet form ([`System`]) and handed to a
//! [`LinearSolver`]. [`DenseLu`] (nalgebra, partial pivoting) is the reference
//! implementation; [`SparseLu`] (faer) is the production solver. Both order
//! the columns with COLAMD, equilibrate rows and columns, and refine the
//! solution iteratively (see `pattern`), so each solution component is
//! accurate relative to its own size, not just to the largest one: a
//! response 300 dB below the drive keeps its digits. Integration tests
//! cross-check the two solvers componentwise (Q101, Q102). Solvers run
//! single-threaded so results are bit-for-bit deterministic (Q108).

mod dense;
mod pattern;
mod sparse;

pub use dense::DenseLu;
pub use sparse::SparseLu;

use crate::analysis::SolverKind;
use num_complex::Complex64;
use std::fmt::Debug;
use std::ops::{Add, AddAssign, Div, Mul, Neg, Sub};

/// Matrix element type: `f64` for DC/transient, [`Complex64`] for AC.
pub trait Scalar:
    Copy
    + Debug
    + Default
    + PartialEq
    + Send
    + Sync
    + 'static
    + Add<Output = Self>
    + Sub<Output = Self>
    + Mul<Output = Self>
    + Div<Output = Self>
    + Neg<Output = Self>
    + AddAssign
{
    fn zero() -> Self;
    fn from_real(v: f64) -> Self;
    /// Magnitude.
    fn magnitude(self) -> f64;
    fn is_finite(self) -> bool;
}

impl Scalar for f64 {
    fn zero() -> Self {
        0.0
    }
    fn from_real(v: f64) -> Self {
        v
    }
    fn magnitude(self) -> f64 {
        self.abs()
    }
    fn is_finite(self) -> bool {
        f64::is_finite(self)
    }
}

impl Scalar for Complex64 {
    fn zero() -> Self {
        Complex64::new(0.0, 0.0)
    }
    fn from_real(v: f64) -> Self {
        Complex64::new(v, 0.0)
    }
    fn magnitude(self) -> f64 {
        self.norm()
    }
    fn is_finite(self) -> bool {
        self.re.is_finite() && self.im.is_finite()
    }
}

/// A square linear system `A·x = b` assembled from additive contributions.
/// Rows/columns given as `None` refer to ground and are dropped, which is how
/// device stamps eliminate the reference node.
#[derive(Debug, Clone)]
pub struct System<T> {
    size: usize,
    entries: Vec<(usize, usize, T)>,
    rhs: Vec<T>,
}

impl<T: Scalar> System<T> {
    pub fn new(size: usize) -> Self {
        Self {
            size,
            entries: Vec::new(),
            rhs: vec![T::zero(); size],
        }
    }

    pub fn size(&self) -> usize {
        self.size
    }

    /// Removes all contributions, keeping allocations.
    pub fn clear(&mut self) {
        self.entries.clear();
        self.rhs.iter_mut().for_each(|v| *v = T::zero());
    }

    /// Adds `value` to `A[row][col]`.
    #[inline]
    pub fn add(&mut self, row: Option<usize>, col: Option<usize>, value: T) {
        if let (Some(r), Some(c)) = (row, col) {
            debug_assert!(r < self.size && c < self.size);
            self.entries.push((r, c, value));
        }
    }

    /// Adds `value` to `b[row]`.
    #[inline]
    pub fn add_rhs(&mut self, row: Option<usize>, value: T) {
        if let Some(r) = row {
            self.rhs[r] += value;
        }
    }

    /// Raw `(row, col, value)` contributions; duplicates are summed.
    pub fn entries(&self) -> &[(usize, usize, T)] {
        &self.entries
    }

    pub fn rhs(&self) -> &[T] {
        &self.rhs
    }

    /// Dense row-major copy of `A` with duplicates summed (tests, debugging).
    pub fn to_dense(&self) -> Vec<Vec<T>> {
        let mut a = vec![vec![T::zero(); self.size]; self.size];
        for &(r, c, v) in &self.entries {
            a[r][c] += v;
        }
        a
    }
}

/// Why a [`System`] could not be solved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SolveFailure {
    /// A pivot vanished: the matrix is singular to working precision.
    Singular,
    /// The factorization succeeded but the solution overflowed or became NaN
    /// (extreme element values, or non-finite entries).
    NonFinite,
}

/// The system could not be solved. `row` is the unknown whose pivot vanished
/// or whose solution value is not finite (used to name the offending node or
/// branch).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SingularError {
    pub row: usize,
    pub kind: SolveFailure,
}

impl SingularError {
    /// A vanished pivot at unknown `row`.
    pub fn singular(row: usize) -> Self {
        Self {
            row,
            kind: SolveFailure::Singular,
        }
    }

    /// A non-finite solution value at unknown `row`.
    pub fn non_finite(row: usize) -> Self {
        Self {
            row,
            kind: SolveFailure::NonFinite,
        }
    }
}

/// Factors and solves a [`System`].
pub trait LinearSolver<T: Scalar>: Send {
    /// Solves `A·x = b`. Implementations may cache symbolic analysis between
    /// calls with an identical sparsity pattern.
    fn solve(&mut self, system: &System<T>) -> Result<Vec<T>, SingularError>;

    /// [`Self::solve`] into `x`, reusing its allocation (the transient's
    /// Newton iterations solve millions of small systems).
    fn solve_into(&mut self, system: &System<T>, x: &mut Vec<T>) -> Result<(), SingularError> {
        *x = self.solve(system)?;
        Ok(())
    }

    /// Solves `A·y = rhs` with the matrix of the last successful
    /// [`Self::solve`], reusing its factorization (no refinement). `None`
    /// when no factorization is held or `rhs` has the wrong length.
    fn resolve(&mut self, rhs: &[T]) -> Option<Vec<T>> {
        let _ = rhs;
        None
    }
}

/// Systems with at most this many unknowns use the dense solver under
/// [`SolverKind::Auto`].
pub const DENSE_LIMIT: usize = 60;

/// A pivot counts as zero when it is not larger than this fraction of the
/// largest entry in its column of the *equilibrated* matrix (rows and
/// columns scaled so their largest entries are about 1).
///
/// The threshold sits at a few units of rounding: a pivot above it still
/// carries a meaningful leading digit. Equilibration makes the test
/// independent of units and impedance levels, and of a single large
/// conductance dominating a column: two default junctions (2.8 pS) next to
/// a 1 mΩ resistor leave a relative pivot of about 2.8e-15, about 12
/// rounding units, and such circuits solve (their solution there is usually
/// exactly zero). A single junction (1.4 pS) behind 1 mΩ is at about 6 units
/// and counts as singular here; an operating point at which every source is
/// zero is then accepted by inspection instead (see [`crate::newton`]). How
/// precisely the rest of such a node can be resolved is not a pivot
/// question; it is limited when `G + g` is rounded during assembly, which
/// the Newton convergence test accounts for (see [`crate::newton`]).
/// Floating nodes and voltage-source loops are diagnosed from the circuit
/// topology before solving, not from pivots (see `circuit::topology`).
pub(crate) const PIVOT_TOLERANCE: f64 = 8.0 * f64::EPSILON;

/// Whether `pivot` is numerically zero for a column whose largest entry has
/// magnitude `column_scale`. NaN pivots count as vanished.
pub(crate) fn pivot_vanished<T: Scalar>(pivot: T, column_scale: f64) -> bool {
    let threshold = PIVOT_TOLERANCE * column_scale;
    pivot.magnitude().partial_cmp(&threshold) != Some(std::cmp::Ordering::Greater)
}

/// Index of the first non-finite solution component, if any.
pub(crate) fn first_non_finite<T: Scalar>(x: &[T]) -> Option<usize> {
    x.iter().position(|&v| !v.is_finite())
}

/// Chooses the solver implementation for a system of `size` unknowns.
pub fn solver_for<T: Scalar>(kind: SolverKind, size: usize) -> Box<dyn LinearSolver<T>>
where
    DenseLu: LinearSolver<T>,
    SparseLu: LinearSolver<T>,
{
    match kind {
        SolverKind::Dense => Box::new(DenseLu::default()),
        SolverKind::Sparse => Box::new(SparseLu::default()),
        SolverKind::Auto if size <= DENSE_LIMIT => Box::new(DenseLu::default()),
        SolverKind::Auto => Box::new(SparseLu::default()),
    }
}
