//! Dense LU with partial pivoting (nalgebra). Reference implementation.
//!
//! The matrix is assembled, ordered and equilibrated exactly like the
//! sparse solver's (see [`super::pattern`]), then stored densely with its
//! columns in COLAMD order and factored with row pivoting by nalgebra's
//! Gaussian elimination steps, the arithmetic of `nalgebra::LU`, applied in
//! place. The symbolic part is cached while the assembled coordinates stay
//! the same, and the dense matrix, the pivots and every work vector are
//! reused from one solve to the next.

use super::pattern::{Assembled, Pattern, Workspace};
use super::{LinearSolver, Scalar, SingularError, System, first_non_finite, pivot_vanished};
use nalgebra::linalg::{gauss_step, gauss_step_swap};
use nalgebra::{ComplexField, DMatrix, DVectorViewMut};
use num_complex::Complex64;

#[derive(Debug, Default, Clone)]
pub struct DenseLu {
    pattern: Option<Pattern>,
    real: Factored<f64>,
    complex: Factored<Complex64>,
}

/// The factorization of the last solve and the storage the next one reuses.
#[derive(Debug, Clone)]
struct Factored<T: ComplexField> {
    assembled: Assembled<T>,
    /// The scaled, column-permuted matrix, overwritten by its LU factors.
    lu: DMatrix<T>,
    /// Row exchanged with row `i` at elimination step `i`.
    pivots: Vec<usize>,
    /// Whether the factors belong to a successful solve (re-solves).
    valid: bool,
    work: Workspace<T>,
    /// The solution of the scaled system in factor order.
    scratch: Vec<T>,
}

impl<T: ComplexField> Default for Factored<T> {
    fn default() -> Self {
        Self {
            assembled: Assembled::default(),
            lu: DMatrix::zeros(0, 0),
            pivots: Vec::new(),
            valid: false,
            work: Workspace::default(),
            scratch: Vec::new(),
        }
    }
}

impl LinearSolver<f64> for DenseLu {
    fn solve(&mut self, system: &System<f64>) -> Result<Vec<f64>, SingularError> {
        let mut x = Vec::new();
        self.solve_into(system, &mut x)?;
        Ok(x)
    }

    fn solve_into(&mut self, system: &System<f64>, x: &mut Vec<f64>) -> Result<(), SingularError> {
        let pattern = prepare(&mut self.pattern, system)?;
        solve_dense(pattern, system, &mut self.real, x)
    }

    fn resolve(&mut self, rhs: &[f64]) -> Option<Vec<f64>> {
        resolve_dense(self.pattern.as_ref()?, &mut self.real, rhs)
    }
}

impl LinearSolver<Complex64> for DenseLu {
    fn solve(&mut self, system: &System<Complex64>) -> Result<Vec<Complex64>, SingularError> {
        let mut x = Vec::new();
        self.solve_into(system, &mut x)?;
        Ok(x)
    }

    fn solve_into(
        &mut self,
        system: &System<Complex64>,
        x: &mut Vec<Complex64>,
    ) -> Result<(), SingularError> {
        let pattern = prepare(&mut self.pattern, system)?;
        solve_dense(pattern, system, &mut self.complex, x)
    }

    fn resolve(&mut self, rhs: &[Complex64]) -> Option<Vec<Complex64>> {
        resolve_dense(self.pattern.as_ref()?, &mut self.complex, rhs)
    }
}

/// Makes the cached pattern match `system`.
fn prepare<'a, T: Scalar>(
    cache: &'a mut Option<Pattern>,
    system: &System<T>,
) -> Result<&'a Pattern, SingularError> {
    if !cache.as_ref().is_some_and(|p| p.matches(system)) {
        *cache = Some(Pattern::analyse(system)?);
    }
    cache.as_ref().ok_or(SingularError::singular(0))
}

/// Factors `matrix` in place with row pivoting: the elimination steps of
/// `nalgebra::LU::new`, recording the exchanged rows in `pivots`.
fn factor<T: Scalar + ComplexField>(matrix: &mut DMatrix<T>, pivots: &mut Vec<usize>) {
    pivots.clear();
    for i in 0..matrix.nrows() {
        let piv = matrix.view_range(i.., i).icamax() + i;
        let diag = matrix[(piv, i)];
        if diag.is_zero() {
            // No non-zero entry in this column; the pivot test rejects it.
            pivots.push(i);
            continue;
        }
        if piv == i {
            gauss_step(matrix, diag, i);
        } else {
            matrix.columns_range_mut(..i).swap_rows(i, piv);
            gauss_step_swap(matrix, diag, i, piv);
        }
        pivots.push(piv);
    }
}

/// In-place solve of the scaled, column-permuted system: the row exchanges
/// and triangular solves of `nalgebra::LU::solve_mut`, then the columns back
/// in unknown order.
fn solve_scaled<'a, T: Scalar + ComplexField>(
    pattern: &'a Pattern,
    lu: &'a DMatrix<T>,
    pivots: &'a [usize],
    scratch: &'a mut Vec<T>,
) -> impl FnMut(&mut [T]) + 'a {
    move |v: &mut [T]| {
        scratch.clear();
        scratch.extend_from_slice(v);
        for (i, &piv) in pivots.iter().enumerate() {
            scratch.swap(i, piv);
        }
        let n = scratch.len();
        let mut y = DVectorViewMut::from_slice(scratch, n);
        // Every pivot passed the vanishing test, so neither solve refuses.
        let _ = lu.solve_lower_triangular_with_diag_mut(&mut y, T::one());
        let _ = lu.solve_upper_triangular_mut(&mut y);
        for (j, &yj) in scratch.iter().enumerate() {
            v[pattern.col_perm[j]] = yj;
        }
    }
}

fn solve_dense<T: Scalar + ComplexField>(
    pattern: &Pattern,
    system: &System<T>,
    factored: &mut Factored<T>,
    x: &mut Vec<T>,
) -> Result<(), SingularError> {
    factored.valid = false;
    let n = pattern.size;
    x.clear();
    if n == 0 {
        return Ok(());
    }
    let Factored {
        assembled,
        lu,
        pivots,
        valid,
        work,
        scratch,
    } = factored;
    assembled.assemble(pattern, system);
    if lu.nrows() == n {
        lu.fill(<T as Scalar>::zero());
    } else {
        *lu = DMatrix::zeros(n, n);
    }
    for j in 0..n {
        let col = pattern.col_perm[j];
        for k in pattern.column(col) {
            lu[(pattern.row_idx[k], j)] = assembled.scaled[k];
        }
    }
    factor(lu, pivots);
    if let Some(j) = (0..n)
        .find(|&j| pivot_vanished(lu[(j, j)], assembled.scaled_column_max[pattern.col_perm[j]]))
    {
        return Err(SingularError::singular(pattern.col_perm[j]));
    }
    assembled.solve_refined(
        pattern,
        system.rhs(),
        x,
        work,
        solve_scaled(pattern, lu, pivots, scratch),
    );
    if let Some(row) = first_non_finite(x) {
        return Err(SingularError::non_finite(row));
    }
    *valid = true;
    Ok(())
}

fn resolve_dense<T: Scalar + ComplexField>(
    pattern: &Pattern,
    factored: &mut Factored<T>,
    rhs: &[T],
) -> Option<Vec<T>> {
    let Factored {
        assembled,
        lu,
        pivots,
        valid,
        scratch,
        ..
    } = factored;
    (*valid && rhs.len() == pattern.size)
        .then(|| assembled.apply_inverse(rhs, &mut solve_scaled(pattern, lu, pivots, scratch)))
}
