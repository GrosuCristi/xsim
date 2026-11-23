//! Sparse LU (faer, sequential). Production solver for larger circuits.
//!
//! The matrix is assembled, ordered by COLAMD and equilibrated as described
//! in [`super::pattern`], and faer's simplicial (left-looking,
//! partial-pivoting) LU factors the scaled matrix. The symbolic part
//! (triplet-to-column mapping and column ordering) is cached and reused for
//! as long as the assembled triplet coordinates stay the same, which is
//! every Newton iteration and timestep of a run; so are the numeric factor
//! storage, faer's scratch memory and every work vector.
//!
//! The simplicial kernel is used directly instead of faer's high-level
//! `Lu` because its `U` factor is accessible: each pivot is checked against
//! a relative threshold to name the unknown of a singular matrix, and
//! everything runs with [`Par::Seq`] so results are deterministic (Q108).

use super::pattern::{Assembled, Pattern, Workspace};
use super::{LinearSolver, Scalar, SingularError, System, first_non_finite, pivot_vanished};
use faer::dyn_stack::{MemBuffer, MemStack, StackReq};
use faer::perm::PermRef;
use faer::sparse::linalg::LuError;
use faer::sparse::linalg::lu::simplicial::{self, SimplicialLu};
use faer::sparse::{SparseColMatRef, SymbolicSparseColMatRef};
use faer::traits::ComplexField;
use faer::{Conj, MatMut, Par};
use num_complex::Complex64;

#[derive(Debug, Default)]
pub struct SparseLu {
    pattern: Option<Pattern>,
    real: Numeric<f64>,
    complex: Numeric<Complex64>,
}

/// Numeric factor storage and work memory (kept to reuse their
/// allocations) and, after a successful solve, what a re-solve needs.
#[derive(Debug)]
struct Numeric<T: ComplexField> {
    lu: SimplicialLu<usize, T>,
    row_perm: Vec<usize>,
    row_perm_inv: Vec<usize>,
    assembled: Assembled<T>,
    /// Whether the factors belong to a successful solve (re-solves).
    valid: bool,
    work: Workspace<T>,
    /// faer's scratch memory for factoring and for solving.
    factor_memory: Memory,
    solve_memory: Memory,
}

/// faer scratch memory kept between solves (allocated on first use).
#[derive(Default)]
struct Memory(Option<MemBuffer>);

impl std::fmt::Debug for Memory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let bytes = self.0.as_ref().map_or(0, |m| m.len());
        write!(f, "Memory({bytes} bytes)")
    }
}

impl Memory {
    /// The memory, grown to at least `request`.
    fn at_least(&mut self, request: StackReq) -> &mut MemBuffer {
        let size = request.unaligned_bytes_required();
        if self.0.as_ref().is_none_or(|m| m.len() < size) {
            self.0 = Some(MemBuffer::new(request));
        }
        self.0.get_or_insert_with(|| MemBuffer::new(request))
    }
}

impl<T: ComplexField> Default for Numeric<T> {
    fn default() -> Self {
        Self {
            lu: SimplicialLu::new(),
            row_perm: Vec::new(),
            row_perm_inv: Vec::new(),
            assembled: Assembled::default(),
            valid: false,
            work: Workspace::default(),
            factor_memory: Memory::default(),
            solve_memory: Memory::default(),
        }
    }
}

impl LinearSolver<f64> for SparseLu {
    fn solve(&mut self, system: &System<f64>) -> Result<Vec<f64>, SingularError> {
        let mut x = Vec::new();
        self.solve_into(system, &mut x)?;
        Ok(x)
    }

    fn solve_into(&mut self, system: &System<f64>, x: &mut Vec<f64>) -> Result<(), SingularError> {
        self.prepare(system)?;
        let Self { pattern, real, .. } = self;
        solve_sparse(
            pattern.as_ref().ok_or(SingularError::singular(0))?,
            real,
            system,
            x,
        )
    }

    fn resolve(&mut self, rhs: &[f64]) -> Option<Vec<f64>> {
        resolve_sparse(self.pattern.as_ref()?, &mut self.real, rhs)
    }
}

impl LinearSolver<Complex64> for SparseLu {
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
        self.prepare(system)?;
        let Self {
            pattern, complex, ..
        } = self;
        solve_sparse(
            pattern.as_ref().ok_or(SingularError::singular(0))?,
            complex,
            system,
            x,
        )
    }

    fn resolve(&mut self, rhs: &[Complex64]) -> Option<Vec<Complex64>> {
        resolve_sparse(self.pattern.as_ref()?, &mut self.complex, rhs)
    }
}

impl SparseLu {
    /// Makes the cached symbolic analysis match `system`, recomputing it only
    /// when the triplet coordinates changed.
    fn prepare<T: Scalar>(&mut self, system: &System<T>) -> Result<(), SingularError> {
        if !self.pattern.as_ref().is_some_and(|p| p.matches(system)) {
            self.pattern = Some(Pattern::analyse(system)?);
            // Factor storage sized for another pattern must not be reused.
            self.real = Numeric::default();
            self.complex = Numeric::default();
        }
        Ok(())
    }
}

/// In-place solve of the scaled system with the factors in `lu`.
fn solve_scaled<'a, T: Scalar + ComplexField>(
    pattern: &'a Pattern,
    lu: &'a SimplicialLu<usize, T>,
    (row_perm, row_perm_inv): (&'a [usize], &'a [usize]),
    memory: &'a mut Memory,
) -> impl FnMut(&mut [T]) + 'a {
    let n = pattern.size;
    move |v: &mut [T]| {
        let request = simplicial::solve_in_place_scratch::<usize, T>(n, 1, Par::Seq);
        let memory = memory.at_least(request);
        let row_perm = PermRef::new_checked(row_perm, row_perm_inv, n);
        let col_perm = PermRef::new_checked(&pattern.col_perm, &pattern.col_perm_inv, n);
        lu.solve_in_place_with_conj(
            row_perm,
            col_perm,
            Conj::No,
            MatMut::from_column_major_slice_mut(v, n, 1),
            Par::Seq,
            MemStack::new(memory),
        );
    }
}

fn solve_sparse<T: Scalar + ComplexField>(
    pattern: &Pattern,
    numeric: &mut Numeric<T>,
    system: &System<T>,
    x: &mut Vec<T>,
) -> Result<(), SingularError> {
    numeric.valid = false;
    let n = pattern.size;
    x.clear();
    if n == 0 {
        return Ok(());
    }
    let Numeric {
        lu,
        row_perm,
        row_perm_inv,
        assembled,
        valid,
        work,
        factor_memory,
        solve_memory,
    } = numeric;
    assembled.assemble(pattern, system);
    let symbolic =
        SymbolicSparseColMatRef::new_checked(n, n, &pattern.col_ptr, None, &pattern.row_idx);
    let matrix = SparseColMatRef::new(symbolic, &assembled.scaled);
    let col_perm = PermRef::new_checked(&pattern.col_perm, &pattern.col_perm_inv, n);
    row_perm.resize(n, 0);
    row_perm_inv.resize(n, 0);
    let request = simplicial::factorize_simplicial_numeric_lu_scratch::<usize, T>(n, n);
    let memory = factor_memory.at_least(request);
    let singular_at =
        |factor_column: usize| SingularError::singular(pattern.col_perm[factor_column]);
    match simplicial::factorize_simplicial_numeric_lu(
        row_perm,
        row_perm_inv,
        lu,
        matrix,
        col_perm,
        MemStack::new(memory),
    ) {
        Ok(()) => {}
        Err(LuError::SymbolicSingular { index }) => return Err(singular_at(index)),
        // Allocation failure; nothing about the circuit to report.
        Err(LuError::Generic(_)) => return Err(SingularError::singular(0)),
    }
    // A numerically vanishing pivot does not stop faer (it divides by it),
    // so inspect U's diagonal in factor order.
    let u = lu.u_factor_unsorted();
    for j in 0..n {
        let diagonal = u
            .row_idx_of_col_raw(j)
            .iter()
            .zip(u.val_of_col(j))
            .find(|&(&row, _)| row == j)
            .map_or(<T as Scalar>::zero(), |(_, &v)| v);
        if pivot_vanished(diagonal, assembled.scaled_column_max[pattern.col_perm[j]]) {
            return Err(singular_at(j));
        }
    }
    let permutation = (row_perm.as_slice(), row_perm_inv.as_slice());
    assembled.solve_refined(
        pattern,
        system.rhs(),
        x,
        work,
        solve_scaled(pattern, lu, permutation, solve_memory),
    );
    if let Some(row) = first_non_finite(x) {
        return Err(SingularError::non_finite(row));
    }
    *valid = true;
    Ok(())
}

fn resolve_sparse<T: Scalar + ComplexField>(
    pattern: &Pattern,
    numeric: &mut Numeric<T>,
    rhs: &[T],
) -> Option<Vec<T>> {
    let Numeric {
        lu,
        row_perm,
        row_perm_inv,
        assembled,
        valid,
        solve_memory,
        ..
    } = numeric;
    (*valid && rhs.len() == pattern.size).then(|| {
        let permutation = (row_perm.as_slice(), row_perm_inv.as_slice());
        assembled.apply_inverse(
            rhs,
            &mut solve_scaled(pattern, lu, permutation, solve_memory),
        )
    })
}
