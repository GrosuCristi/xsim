//! Assembly pattern, column ordering, equilibration and iterative
//! refinement shared by both LU back ends.
//!
//! Both solvers sum the triplets into compressed-column form, order the
//! columns with COLAMD, scale rows and columns by powers of two so that every
//! row and column of the scaled matrix has its largest entry close to 1,
//! factor the scaled matrix with row (partial) pivoting, and refine the
//! solution against the unscaled assembled matrix.
//!
//! * The fill-reducing order follows the circuit's structure (along a
//!   ladder, for example), which keeps small solution components accurate
//!   relative to themselves. Natural order (all node voltages before all
//!   branch currents) loses about `ε·max|x|` absolutely, i.e. every digit of
//!   a response 300 dB below the drive.
//! * Equilibration makes the pivot test independent of units and impedance
//!   level, see [`super::PIVOT_TOLERANCE`]. Powers of two scale exactly.
//! * Fixed-precision refinement makes the solution componentwise backward
//!   stable (Skeel), so dense and sparse agree to rounding error.

use super::{Scalar, SingularError, System};
use faer::dyn_stack::{MemBuffer, MemStack};
use faer::sparse::SymbolicSparseColMatRef;
use faer::sparse::linalg::colamd;
use std::cmp::Ordering;

/// Most refinement corrections applied to one solve.
const REFINEMENT_STEPS: usize = 3;

/// Refinement stops once the componentwise backward error is this small.
const REFINEMENT_TARGET: f64 = 4.0 * f64::EPSILON;

/// Symbolic analysis of one assembly pattern.
#[derive(Debug, Clone)]
pub(super) struct Pattern {
    pub(super) size: usize,
    /// `(row, col)` of every triplet in assembly order: the cache key.
    coordinates: Vec<(usize, usize)>,
    pub(super) col_ptr: Vec<usize>,
    /// Row indices, sorted within each column, without duplicates.
    pub(super) row_idx: Vec<usize>,
    /// Position in `row_idx`/values of each triplet (duplicates share one).
    slot_of_triplet: Vec<usize>,
    /// Fill-reducing column order: factor column `j` is unknown `col_perm[j]`.
    pub(super) col_perm: Vec<usize>,
    pub(super) col_perm_inv: Vec<usize>,
}

impl Pattern {
    /// Whether `system` was assembled with exactly these coordinates.
    pub(super) fn matches<T: Scalar>(&self, system: &System<T>) -> bool {
        self.size == system.size()
            && self.coordinates.len() == system.entries().len()
            && self
                .coordinates
                .iter()
                .zip(system.entries())
                .all(|(&(r, c), &(row, col, _))| r == row && c == col)
    }

    pub(super) fn analyse<T: Scalar>(system: &System<T>) -> Result<Self, SingularError> {
        let n = system.size();
        let coordinates: Vec<(usize, usize)> =
            system.entries().iter().map(|&(r, c, _)| (r, c)).collect();
        // Stable sort by column then row gives a deterministic CSC layout.
        let mut order: Vec<usize> = (0..coordinates.len()).collect();
        order.sort_by_key(|&k| (coordinates[k].1, coordinates[k].0));

        let mut col_ptr = vec![0; n + 1];
        let mut row_idx = Vec::with_capacity(order.len());
        let mut slot_of_triplet = vec![0; coordinates.len()];
        let mut previous = None;
        for &k in &order {
            let (row, col) = coordinates[k];
            if previous != Some((row, col)) {
                row_idx.push(row);
                col_ptr[col + 1] += 1;
                previous = Some((row, col));
            }
            slot_of_triplet[k] = row_idx.len() - 1;
        }
        for col in 0..n {
            col_ptr[col + 1] += col_ptr[col];
        }

        let mut col_perm = vec![0; n];
        let mut col_perm_inv = vec![0; n];
        if n > 0 {
            let symbolic = SymbolicSparseColMatRef::new_checked(n, n, &col_ptr, None, &row_idx);
            let mut memory = MemBuffer::new(colamd::order_scratch::<usize>(n, n, row_idx.len()));
            colamd::order(
                &mut col_perm,
                &mut col_perm_inv,
                symbolic,
                colamd::Control::default(),
                MemStack::new(&mut memory),
            )
            // COLAMD only fails on index overflow or allocation failure,
            // which a circuit-sized matrix cannot reach; report it as
            // unsolvable.
            .map_err(|_| SingularError::singular(0))?;
        }

        Ok(Self {
            size: n,
            coordinates,
            col_ptr,
            row_idx,
            slot_of_triplet,
            col_perm,
            col_perm_inv,
        })
    }

    /// Range of `row_idx`/values belonging to column `col`.
    pub(super) fn column(&self, col: usize) -> std::ops::Range<usize> {
        self.col_ptr[col]..self.col_ptr[col + 1]
    }
}

/// The numeric values of one assembled system in a [`Pattern`]'s layout.
/// Its buffers are reused from one solve to the next ([`Self::assemble`]).
#[derive(Debug, Clone)]
pub(super) struct Assembled<T> {
    /// Summed matrix entries, unscaled.
    pub(super) values: Vec<T>,
    /// Entries of `R·A·C`.
    pub(super) scaled: Vec<T>,
    /// Power-of-two row scale `R` and column scale `C`.
    pub(super) row_scale: Vec<f64>,
    pub(super) col_scale: Vec<f64>,
    /// Largest magnitude in each column of the scaled matrix.
    pub(super) scaled_column_max: Vec<f64>,
}

impl<T> Default for Assembled<T> {
    fn default() -> Self {
        Self {
            values: Vec::new(),
            scaled: Vec::new(),
            row_scale: Vec::new(),
            col_scale: Vec::new(),
            scaled_column_max: Vec::new(),
        }
    }
}

/// Scratch vectors of the iterative refinement, kept between solves.
#[derive(Debug, Clone)]
pub(super) struct Workspace<T> {
    residual: Vec<T>,
    next_residual: Vec<T>,
    bound: Vec<f64>,
    correction: Vec<T>,
    candidate: Vec<T>,
}

impl<T> Default for Workspace<T> {
    fn default() -> Self {
        Self {
            residual: Vec::new(),
            next_residual: Vec::new(),
            bound: Vec::new(),
            correction: Vec::new(),
            candidate: Vec::new(),
        }
    }
}

/// The power of two nearest to `1/m` (1 for an empty row or column).
fn inverse_power_of_two(m: f64) -> f64 {
    if m > 0.0 && m.is_finite() {
        let exponent = m.log2().round().clamp(-1000.0, 1000.0);
        (-exponent).exp2()
    } else {
        1.0
    }
}

impl<T: Scalar> Assembled<T> {
    /// Sums the triplets of `system` into this storage and equilibrates the
    /// result, reusing the buffers of the previous solve.
    pub(super) fn assemble(&mut self, pattern: &Pattern, system: &System<T>) {
        let n = pattern.size;
        let values = &mut self.values;
        values.clear();
        values.resize(pattern.row_idx.len(), T::zero());
        for (&slot, &(_, _, value)) in pattern.slot_of_triplet.iter().zip(system.entries()) {
            values[slot] += value;
        }
        // The row maxima, then their scales, in place.
        let row_scale = &mut self.row_scale;
        row_scale.clear();
        row_scale.resize(n, 0.0);
        for col in 0..n {
            for k in pattern.column(col) {
                let r = pattern.row_idx[k];
                row_scale[r] = row_scale[r].max(values[k].magnitude());
            }
        }
        for scale in row_scale.iter_mut() {
            *scale = inverse_power_of_two(*scale);
        }
        let col_scale = &mut self.col_scale;
        col_scale.clear();
        col_scale.resize(n, 1.0);
        for (col, scale) in col_scale.iter_mut().enumerate() {
            let m = pattern
                .column(col)
                .map(|k| values[k].magnitude() * row_scale[pattern.row_idx[k]])
                .fold(0.0, f64::max);
            *scale = inverse_power_of_two(m);
        }
        self.scaled.clear();
        self.scaled.extend_from_slice(values);
        self.scaled_column_max.clear();
        self.scaled_column_max.resize(n, 0.0);
        for (col, (&scale, column_max)) in col_scale
            .iter()
            .zip(self.scaled_column_max.iter_mut())
            .enumerate()
        {
            for k in pattern.column(col) {
                let factor = row_scale[pattern.row_idx[k]] * scale;
                self.scaled[k] = values[k] * T::from_real(factor);
                *column_max = f64::max(*column_max, self.scaled[k].magnitude());
            }
        }
    }

    /// `rhs − A·x` into `r` and the componentwise (Oettli–Prager) backward
    /// error `max |rhs − A·x|ᵢ / (|A|·|x| + |rhs|)ᵢ`, with `bound` as
    /// scratch.
    fn residual(
        &self,
        pattern: &Pattern,
        x: &[T],
        rhs: &[T],
        r: &mut Vec<T>,
        bound: &mut Vec<f64>,
    ) -> f64 {
        r.clear();
        r.extend_from_slice(rhs);
        bound.clear();
        bound.extend(rhs.iter().map(|v| v.magnitude()));
        for (col, &xc) in x.iter().enumerate() {
            for k in pattern.column(col) {
                let row = pattern.row_idx[k];
                r[row] = r[row] - self.values[k] * xc;
                bound[row] += self.values[k].magnitude() * xc.magnitude();
            }
        }
        r.iter()
            .zip(bound.iter())
            .map(|(ri, &b)| if b > 0.0 { ri.magnitude() / b } else { 0.0 })
            .fold(
                0.0,
                |acc: f64, v| if v.is_nan() { f64::NAN } else { acc.max(v) },
            )
    }

    /// `C·(R·A·C)⁻¹·R·v`, i.e. `A⁻¹·v`, into `out`, given an in-place solver
    /// of the scaled system.
    pub(super) fn apply_inverse_into(
        &self,
        v: &[T],
        out: &mut Vec<T>,
        solve_scaled: &mut impl FnMut(&mut [T]),
    ) {
        out.clear();
        out.extend(
            v.iter()
                .zip(&self.row_scale)
                .map(|(&vi, &r)| vi * T::from_real(r)),
        );
        solve_scaled(out);
        for (yi, &c) in out.iter_mut().zip(&self.col_scale) {
            *yi = *yi * T::from_real(c);
        }
    }

    /// [`Self::apply_inverse_into`] into a new vector.
    pub(super) fn apply_inverse(&self, v: &[T], solve_scaled: &mut impl FnMut(&mut [T])) -> Vec<T> {
        let mut out = Vec::with_capacity(v.len());
        self.apply_inverse_into(v, &mut out, solve_scaled);
        out
    }

    /// Solves `A·x = rhs` into `x` and refines the solution while its
    /// componentwise backward error keeps improving. A first solution whose
    /// backward error is already at the rounding level is not refined.
    pub(super) fn solve_refined(
        &self,
        pattern: &Pattern,
        rhs: &[T],
        x: &mut Vec<T>,
        work: &mut Workspace<T>,
        mut solve_scaled: impl FnMut(&mut [T]),
    ) {
        self.apply_inverse_into(rhs, x, &mut solve_scaled);
        let Workspace {
            residual,
            next_residual,
            bound,
            correction,
            candidate,
        } = work;
        let mut omega = self.residual(pattern, x, rhs, residual, bound);
        for _ in 0..REFINEMENT_STEPS {
            // NaN also stops here; the caller reports the non-finite value.
            if omega.partial_cmp(&REFINEMENT_TARGET) != Some(Ordering::Greater) {
                break;
            }
            self.apply_inverse_into(residual, correction, &mut solve_scaled);
            candidate.clear();
            candidate.extend(x.iter().zip(correction.iter()).map(|(&a, &b)| a + b));
            let omega_next = self.residual(pattern, candidate, rhs, next_residual, bound);
            if omega_next.partial_cmp(&omega) != Some(Ordering::Less) {
                break;
            }
            std::mem::swap(x, candidate);
            std::mem::swap(residual, next_residual);
            omega = omega_next;
        }
    }
}
