mod base;
mod checker;
mod csp_solver;

/// Minesweeper solver instance.
pub struct Solver {
    inner: base::Solver,
    utils: base::SolverUtils,
    checker: checker::CheckerData,
}

/// This is returned once known numbers have been set.
pub struct SolverWithKnown<'a> {
    solver: &'a mut Solver,
}

pub use base::Failure as SolverFailure;

pub use crate::solver::base::counters::print as print_dbg_counters;

/// Minesweeper solver error.
#[derive(Debug, Clone)]
pub enum Error {
    InvalidGridSize,
    RowColumnOutOfRange,
    SolverFailure(SolverFailure),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::InvalidGridSize => {
                f.write_str("Invalid size of new grid, expected width * height")
            }
            Error::RowColumnOutOfRange => f.write_str("Row or column is out of range"),
            Error::SolverFailure(SolverFailure::Empty) => {
                f.write_str("No solution; grid is completely blank")
            }
            Error::SolverFailure(SolverFailure::MustGuess) => {
                f.write_str("No cell is guaranteed to be safe")
            }
            Error::SolverFailure(SolverFailure::Solved) => {
                f.write_str("The board is already solved")
            }
            Error::SolverFailure(SolverFailure::Unsolvable) => {
                f.write_str("The board is unsolvable")
            }
        }
    }
}

impl Solver {
    pub fn new(w: usize, h: usize, seed: u64) -> Self {
        let inner = base::Solver::new(w, h, seed);
        let utils = base::SolverUtils::new(&inner);
        let checker = checker::CheckerData::new(&inner);
        Self {
            inner,
            utils,
            checker,
        }
    }

    pub fn with_known(
        &mut self,
        grid: &[impl Into<Option<usize>> + Clone],
        n_mines: Option<usize>,
    ) -> Result<SolverWithKnown<'_>, Error> {
        if grid.len() != self.inner.w * self.inner.h {
            Err(Error::InvalidGridSize)
        } else {
            self.checker_state().update(grid, n_mines);
            Ok(SolverWithKnown { solver: self })
        }
    }

    pub fn size(&self) -> (usize, usize) {
        (self.inner.w, self.inner.h)
    }

    fn checker_state(&mut self) -> checker::CheckerState<'_> {
        checker::CheckerState::new(&self.inner, &mut self.checker, &mut self.utils)
    }
}

impl<'a> SolverWithKnown<'a> {
    pub fn solver(&self) -> &'_ Solver {
        self.solver
    }

    /// Finds a guaranteed safe cell indexed (row, col) or returns a solver
    /// failure.
    pub fn find_safe_cell(&mut self) -> Result<(usize, usize), Error> {
        self.solver
            .checker_state()
            .find_safe_cell(|_| true)
            .map_err(Error::SolverFailure)
    }

    /// Finds a safe cell if you already know some cells are mines / want to
    /// filter out potential safe cells. Note this might return a cell for which
    /// the filter function returns false if it is safe; the function is merely
    /// a guide.
    pub fn find_safe_cell_filtering(
        &mut self,
        filter_cells: impl FnMut(usize) -> bool,
    ) -> Result<(usize, usize), Error> {
        self.solver
            .checker_state()
            .find_safe_cell(filter_cells)
            .map_err(Error::SolverFailure)
    }

    /// Returns the probability that the cell at (row, col) is a mine. If this
    /// solver has a fixed number of mines, mines are assumed to be randomly
    /// distributed across the unknown cells in a way that is consistent with
    /// the known cells. If there is no fixed number of mines, and the cell may
    /// be a mine, this returns None.
    pub fn mine_probability(&mut self, row: usize, col: usize) -> Result<Option<f32>, Error> {
        let pos = self.get_pos(row, col)?;
        self.solver
            .checker_state()
            .mine_probability(pos)
            .map_err(Error::SolverFailure)
    }

    pub fn can_be_mine(&mut self, row: usize, col: usize) -> Result<bool, Error> {
        let pos = self.get_pos(row, col)?;
        Ok(self.solver.checker_state().can_be_mine(pos))
    }

    fn get_pos(&self, row: usize, col: usize) -> Result<usize, Error> {
        let &base::Solver { w, h, .. } = &self.solver.inner;
        if row >= h || col >= w {
            Err(Error::RowColumnOutOfRange)
        } else {
            Ok(row * w + col)
        }
    }
}

// These tests are written by Claude.
// #[cfg(test)]
// mod tests {
//     use super::*;
//     use crate::utils::{board_from_str, board_to_str};
//     use rand::prelude::*;

//     /// Parse a board string into (grid, width, height).
//     fn parse_board(s: &str) -> (Vec<Option<usize>>, usize, usize) {
//         let result = board_from_str(s).unwrap();
//         (result.to_known().collect(), result.w, result.h)
//     }

//     /// Build a solver from a board string, with optional mine count.
//     fn solver_from_str(s: &str, n_mine: Option<usize>, seed: u64) -> (Solver,
// Vec<Option<usize>>) {         let (grid, w, h) = parse_board(s);
//         let solver = Solver::new(w, h, n_mine, seed);
//         (solver, grid)
//     }

//     /// Count unknown cells in a grid.
//     fn count_unknowns(grid: &[Option<usize>]) -> usize {
//         grid.iter().filter(|c| c.is_none()).count()
//     }

//     // -------------------------------------------------------
//     // Construction & size
//     // -------------------------------------------------------

//     #[test]
//     fn new_solver_has_correct_size() {
//         let s = Solver::new(8, 5, Some(10), 0);
//         assert_eq!(s.size(), (8, 5));
//     }

//     #[test]
//     fn size_matches_board_string() {
//         let board = "\
//             1 1 1\n\
//             1 # 1\n\
//             1 1 1";
//         let (grid, w, h) = parse_board(board);
//         let s = Solver::new(w, h, Some(1), 0);
//         assert_eq!(s.size(), (w, h));
//         assert_eq!(grid.len(), w * h);
//     }

//     // -------------------------------------------------------
//     // board_to_str / board_from_str round-trip
//     // -------------------------------------------------------

//     #[test]
//     fn board_str_roundtrip() {
//         let board = "\
//             0 1 #\n\
//             0 2 #\n\
//             0 1 #";
//         let (grid, w, _) = parse_board(board);
//         let back = board_to_str(&grid, w, None);
//         assert_eq!(back, board);
//     }

//     #[test]
//     fn board_str_roundtrip_random() {
//         let mut rng = SmallRng::seed_from_u64(42);
//         for _ in 0..20 {
//             let w = rng.random_range(2..=10);
//             let h = rng.random_range(2..=10);
//             let grid: Vec<Option<usize>> = (0..w * h)
//                 .map(|_| {
//                     if rng.random_bool(0.3) {
//                         None
//                     } else {
//                         Some(rng.random_range(0..=8))
//                     }
//                 })
//                 .collect();
//             let s = board_to_str(&grid, w, None);
//             let (grid2, w2, _) = parse_board(&s);
//             assert_eq!(w, w2);
//             assert_eq!(grid, grid2);
//         }
//     }

//     // -------------------------------------------------------
//     // with_known — error cases
//     // -------------------------------------------------------

//     #[test]
//     fn with_known_wrong_size_is_error() {
//         let mut s = Solver::new(3, 3, Some(1), 0);
//         let too_short: Vec<Option<usize>> = vec![None; 8]; // need 9
//         assert!(matches!(
//             s.with_known(&too_short),
//             Err(Error::InvalidGridSize)
//         ));
//     }

//     #[test]
//     fn with_known_too_long_is_error() {
//         let mut s = Solver::new(3, 3, Some(1), 0);
//         let too_long: Vec<Option<usize>> = vec![None; 10];
//         assert!(matches!(
//             s.with_known(&too_long),
//             Err(Error::InvalidGridSize)
//         ));
//     }

//     #[test]
//     fn with_known_correct_size_succeeds() {
//         let mut s = Solver::new(3, 3, Some(1), 0);
//         let grid: Vec<Option<usize>> = vec![None; 9];
//         assert!(s.with_known(&grid).is_ok());
//     }

//     // -------------------------------------------------------
//     // mine_probability — out of range
//     // -------------------------------------------------------

//     #[test]
//     fn mine_probability_out_of_range() {
//         let board = "\
//             0 0 0\n\
//             0 1 #\n\
//             0 1 #";
//         let (mut solver, grid) = solver_from_str(board, Some(1), 0);
//         let mut sk = solver.with_known(&grid).unwrap();
//         assert!(matches!(
//             sk.mine_probability(10, 0),
//             Err(Error::RowColumnOutOfRange)
//         ));
//         assert!(matches!(
//             sk.mine_probability(0, 10),
//             Err(Error::RowColumnOutOfRange)
//         ));
//     }

//     // -------------------------------------------------------
//     // Trivially solvable boards
//     // -------------------------------------------------------

//     #[test]
//     fn single_unknown_is_found() {
//         // Only one unknown cell — it must be the mine.
//         // The safe cell finder should not return that cell.
//         let board = "\
//             0 1 1\n\
//             0 1 #\n\
//             0 1 1";
//         let (mut solver, grid) = solver_from_str(board, Some(1), 0);
//         let mut sk = solver.with_known(&grid).unwrap();
//         // The only unknown is (1,2). With 1 mine, probability should be 1.0.
//         let p = sk.mine_probability(1, 2).unwrap().unwrap();
//         assert!(
//             (p - 1.0).abs() < 1e-6,
//             "Only unknown cell with 1 mine should have probability 1.0, got
// {p}"         );
//     }

//     #[test]
//     fn obvious_safe_cell() {
//         // Two unknowns, one mine. The "1" cell constrains which is the mine.
//         let board = "\
//             0 1 #\n\
//             0 1 #\n\
//             0 0 0";
//         let (mut solver, grid) = solver_from_str(board, Some(1), 0);
//         let mut sk = solver.with_known(&grid).unwrap();
//         let (r, c) = sk.find_safe_cell().unwrap();
//         // (2,2) is zero, so the safe cell must be one of the unknowns.
//         // The "1" at (0,1) already touches one unknown (0,2).
//         // The "1" at (1,1) touches both unknowns.
//         // So (1,2) could be safe. Either way the returned cell must not be a
// mine.         assert!(
//             grid[r * 3 + c].is_none(),
//             "find_safe_cell should return an unknown cell, got known at
// ({r},{c})"         );
//     }

//     #[test]
//     fn fully_revealed_board_is_solved() {
//         let board = "\
//             0 0 0\n\
//             0 0 0\n\
//             0 0 0";
//         let (mut solver, grid) = solver_from_str(board, Some(0), 0);
//         let mut sk = solver.with_known(&grid).unwrap();
//         let result = sk.find_safe_cell();
//         assert!(
//             matches!(result,
// Err(Error::SolverFailure(SolverFailure::Solved))),             "Fully solved
// board should return Solved, got {result:?}"         );
//     }

//     // -------------------------------------------------------
//     // Corner / edge mine configurations
//     // -------------------------------------------------------

//     #[test]
//     fn corner_mine_deduction() {
//         // A "1" in the corner with one unknown neighbor — that neighbor is
// the mine.         let board = "\
//             1 #\n\
//             # #";
//         let (mut solver, grid) = solver_from_str(board, Some(1), 0);
//         let mut sk = solver.with_known(&grid).unwrap();

//         // (0,1) is the only neighbor of the "1" at (0,0), so prob = 1.0
//         // Wait — (1,0) and (1,1) are also neighbors. With 1 mine total the
//         // constraint is: exactly one of {(0,1),(1,0),(1,1)} is a mine.
//         // All three are equally constrained by a single "1", so prob = 1/3
// each.         let p01 = sk.mine_probability(0, 1).unwrap().unwrap();
//         let p10 = sk.mine_probability(1, 0).unwrap().unwrap();
//         let p11 = sk.mine_probability(1, 1).unwrap().unwrap();
//         let total = p01 + p10 + p11;
//         assert!(
//             (total - 1.0).abs() < 1e-4,
//             "Probabilities should sum to 1.0 (1 mine among 3 unknowns), got
// {total}"         );
//     }

//     #[test]
//     fn edge_1_2_pattern() {
//         // Classic 1-2 pattern on a border.
//         let board = "\
//             # # # # #\n\
//             1 2 1 2 1\n\
//             0 0 0 0 0";
//         let (mut solver, grid) = solver_from_str(board, Some(3), 0);
//         let mut sk = solver.with_known(&grid).unwrap();

//         // (0,0) is only adjacent to "1" at (1,0) and "2" at (1,1).
//         // In valid configurations the corners (0,0) and (0,4) should be
// safe.         assert_eq!(
//             sk.mine_probability(0, 0).unwrap().unwrap(),
//             1.0,
//             "Corner cell in 1-2-2-2-1 pattern should be mine"
//         );
//         assert_eq!(
//             sk.mine_probability(0, 1).unwrap().unwrap(),
//             0.0,
//             "Cell in 1-2-2-2-1 pattern should be safe"
//         );
//     }

//     // -------------------------------------------------------
//     // Probability sanity checks
//     // -------------------------------------------------------

//     #[test]
//     fn probabilities_sum_correctly() {
//         // On a partially-revealed board, mine probabilities over unknowns
//         // should sum to the number of remaining mines.
//         let board = "\
//             # # # #\n\
//             # 3 2 #\n\
//             # 2 1 1\n\
//             # 1 0 0";
//         let (grid, w, h) = parse_board(board);
//         let n_mine = 4;
//         let mut solver = Solver::new(w, h, Some(n_mine), 0);
//         let mut sk = solver.with_known(&grid).unwrap();

//         let mut prob_sum = 0.0f64;
//         let mut n_unknown = 0;
//         for r in 0..h {
//             for c in 0..w {
//                 if grid[r * w + c].is_none() {
//                     prob_sum += sk.mine_probability(r, c).unwrap().unwrap()
// as f64;                     n_unknown += 1;
//                 }
//             }
//         }
//         assert!(
//             (prob_sum - n_mine as f64).abs() < 0.1,
//             "Sum of mine probs ({prob_sum:.3}) should ≈ {n_mine} ({n_unknown}
// unknowns)"         );
//     }

//     #[test]
//     fn probability_bounds() {
//         let board = "\
//             # # #\n\
//             # 2 #\n\
//             # # #";
//         let (mut solver, grid) = solver_from_str(board, Some(2), 0);
//         let mut sk = solver.with_known(&grid).unwrap();

//         for r in 0..3 {
//             for c in 0..3 {
//                 if grid[r * 3 + c].is_none() {
//                     let p = sk.mine_probability(r, c).unwrap().unwrap();
//                     assert!(
//                         (0.0..=1.0).contains(&p),
//                         "Probability at ({r},{c}) = {p}, expected in [0,1]"
//                     );
//                 }
//             }
//         }
//     }

//     // -------------------------------------------------------
//     // Symmetry: symmetric boards should give symmetric probs
//     // -------------------------------------------------------

//     #[test]
//     fn symmetric_board_gives_symmetric_probs() {
//         let board = "\
//             # # #\n\
//             # 4 #\n\
//             # # #";
//         let (mut solver, grid) = solver_from_str(board, Some(4), 0);
//         let mut sk = solver.with_known(&grid).unwrap();

//         let corners = [(0, 0), (0, 2), (2, 0), (2, 2)];
//         let edges = [(0, 1), (1, 0), (1, 2), (2, 1)];

//         let corner_probs: Vec<f32> = corners
//             .iter()
//             .map(|&(r, c)| sk.mine_probability(r, c).unwrap().unwrap())
//             .collect();
//         let edge_probs: Vec<f32> = edges
//             .iter()
//             .map(|&(r, c)| sk.mine_probability(r, c).unwrap().unwrap())
//             .collect();

//         // All corners should be equal.
//         for p in &corner_probs {
//             assert!(
//                 (p - corner_probs[0]).abs() < 1e-6,
//                 "Corner probs should be equal: {corner_probs:?}"
//             );
//         }
//         // All edges should be equal.
//         for p in &edge_probs {
//             assert!(
//                 (p - edge_probs[0]).abs() < 1e-6,
//                 "Edge probs should be equal: {edge_probs:?}"
//             );
//         }
//     }

//     // -------------------------------------------------------
//     // Deterministic: same seed → same result
//     // -------------------------------------------------------

//     #[test]
//     fn deterministic_across_runs() {
//         let board = "\
//             # # #\n\
//             # 3 #\n\
//             # # #";
//         for seed in [0, 1, 999] {
//             let (mut s1, grid) = solver_from_str(board, Some(3), seed);
//             let (mut s2, grid2) = solver_from_str(board, Some(3), seed);

//             let r1 = s1.with_known(&grid).unwrap().find_safe_cell();
//             let r2 = s2.with_known(&grid2).unwrap().find_safe_cell();
//             assert_eq!(
//                 format!("{r1:?}"),
//                 format!("{r2:?}"),
//                 "Same seed {seed} should give same result"
//             );
//         }
//     }

//     // -------------------------------------------------------
//     // Blank (all unknown) board
//     // -------------------------------------------------------

//     #[test]
//     fn all_unknown_board() {
//         let mut solver = Solver::new(3, 3, Some(1), 0);
//         let grid: Vec<Option<usize>> = vec![None; 9];
//         let mut sk = solver.with_known(&grid).unwrap();
//         let result = sk.find_safe_cell();
//         // With no information, solver should fail (Empty or MustGuess).
//         assert!(
//             result.is_err(),
//             "All-unknown board should not find a safe cell"
//         );
//     }

//     // -------------------------------------------------------
//     // Random board generation: verify probability consistency
//     // -------------------------------------------------------

//     #[test]
//     fn random_partial_boards_probs_in_range() {
//         let mut rng = SmallRng::seed_from_u64(777);
//         for _ in 0..30 {
//             let w = rng.random_range(3..=6);
//             let h = rng.random_range(3..=6);
//             let n_mine = rng.random_range(1..=(w * h / 3).max(1));

//             // Create a random partial board: some known, some unknown.
//             let grid: Vec<Option<usize>> = (0..w * h)
//                 .map(|_| {
//                     if rng.random_bool(0.4) {
//                         Some(rng.random_range(0..=8))
//                     } else {
//                         None
//                     }
//                 })
//                 .collect();

//             if count_unknowns(&grid) == 0 {
//                 continue;
//             }

//             let mut solver = Solver::new(w, h, Some(n_mine), rng.random());
//             let Ok(mut sk) = solver.with_known(&grid) else {
//                 continue;
//             };

//             // Every probability should be in [0, 1].
//             for r in 0..h {
//                 for c in 0..w {
//                     if grid[r * w + c].is_none() {
//                         match sk.mine_probability(r, c) {
//                             Ok(Some(p)) => assert!(
//                                 (0.0..=1.0).contains(&p),
//                                 "Prob {p} out of range at ({r},{c}) on
// {h}x{w} board"                             ),
//                             Ok(None) => {} // no fixed mine count path
//                             Err(_) => {}   // solver may legitimately fail on
// random boards                         }
//                     }
//                 }
//             }
//         }
//     }

//     // -------------------------------------------------------
//     // find_safe_cell result is always an unknown cell
//     // -------------------------------------------------------

//     #[test]
//     fn safe_cell_is_always_unknown() {
//         let boards = [
//             "\
//                 0 1 #\n\
//                 0 1 #\n\
//                 0 0 0",
//             "\
//                 # 2 #\n\
//                 1 # 1\n\
//                 0 0 0",
//             "\
//                 0 0 0 0\n\
//                 1 1 1 0\n\
//                 # # 1 0\n\
//                 # # 1 0",
//         ];

//         for (i, board) in boards.iter().enumerate() {
//             let (grid, w, h) = parse_board(board);
//             let n_mine = count_unknowns(&grid);
//             // Use fewer mines so there's something safe to find.
//             let n_mine_safe = n_mine.saturating_sub(1).max(1);
//             let mut solver = Solver::new(w, h, Some(n_mine_safe), 0);
//             match solver.with_known(&grid).unwrap().find_safe_cell() {
//                 Ok((r, c)) => {
//                     assert!(
//                         grid[r * w + c].is_none(),
//                         "Board {i}: safe cell ({r},{c}) should be unknown"
//                     );
//                 }
//                 Err(_) => {} // acceptable if solver can't determine safety
//             }
//         }
//     }

//     // -------------------------------------------------------
//     // Error display
//     // -------------------------------------------------------

//     #[test]
//     fn error_display_messages() {
//         let cases = [
//             (Error::InvalidGridSize, "Invalid size"),
//             (Error::RowColumnOutOfRange, "out of range"),
//             (
//                 Error::SolverFailure(SolverFailure::Empty),
//                 "completely blank",
//             ),
//             (Error::SolverFailure(SolverFailure::MustGuess), "guaranteed"),
//             (
//                 Error::SolverFailure(SolverFailure::Solved),
//                 "already solved",
//             ),
//             (
//                 Error::SolverFailure(SolverFailure::Unsolvable),
//                 "unsolvable",
//             ),
//         ];
//         for (err, substring) in cases {
//             let msg = err.to_string();
//             assert!(
//                 msg.to_lowercase().contains(&substring.to_lowercase()),
//                 "Expected '{substring}' in error message '{msg}'"
//             );
//         }
//     }

//     // -------------------------------------------------------
//     // No fixed mine count → None probability for ambiguous cells
//     // -------------------------------------------------------

//     #[test]
//     fn no_mine_count_returns_none_for_ambiguous() {
//         let board = "\
//             # # #\n\
//             # 1 #\n\
//             # # #";
//         let (mut solver, grid) = solver_from_str(board, None, 0);
//         let mut sk = solver.with_known(&grid).unwrap();

//         // Without a fixed mine count, ambiguous cells should return
// Ok(None).         // Cells that are definitively safe or mine may still
// return Some.         for r in 0..3 {
//             for c in 0..3 {
//                 if grid[r * 3 + c].is_none() {
//                     match sk.mine_probability(r, c) {
//                         Ok(None) => {} // expected for ambiguous
//                         Ok(Some(p)) => assert!(
//                             p == 0.0 || p == 1.0,
//                             "Without mine count, non-None prob should be 0 or
// 1, got {p}"                         ),
//                         Err(_) => {} // also acceptable
//                     }
//                 }
//             }
//         }
//     }

//     // -------------------------------------------------------
//     // Known cell probability should be 0
//     // -------------------------------------------------------

//     #[test]
//     fn known_cell_has_zero_mine_probability() {
//         let board = "\
//             0 1 #\n\
//             0 1 #\n\
//             0 0 0";
//         let (mut solver, grid) = solver_from_str(board, Some(1), 0);
//         let mut sk = solver.with_known(&grid).unwrap();

//         // (0,0) is known "0" — asking its probability should give 0 (or
// error).         match sk.mine_probability(0, 0) {
//             Ok(Some(p)) => assert!(
//                 p < 1e-6,
//                 "Known cell should have ~0 mine probability, got {p}"
//             ),
//             Ok(None) | Err(_) => {} // also fine
//         }
//     }

//     // -------------------------------------------------------
//     // Exact known probability values
//     // -------------------------------------------------------

//     /// Assert all unknowns in `board` have the expected mine probabilities.
//     /// `expected` is keyed by (row, col).
//     fn assert_probs(board: &str, n_mine: Option<usize>, expected: &[((usize,
// usize), f32)]) {         let (mut solver, grid) = solver_from_str(board,
// n_mine, 0);         let mut sk = solver.with_known(&grid).unwrap();
//         for &((r, c), want) in expected {
//             let got = sk
//                 .mine_probability(r, c)
//                 .expect("mine_probability should not error")
//                 .expect("expected Some(p) with fixed mine count");
//             assert!(
//                 (got - want).abs() < 1e-4,
//                 "Prob at ({r},{c}): expected {want}, got {got}"
//             );
//         }
//     }

//     #[test]
//     fn one_two_one_pattern_exact_probs() {
//         // The classic 1-2-1: middle unknown is safe, edges are mines.
//         // (No row of zeros below — that would contradict the "1"s.)
//         let board = "\
//             1 2 1\n\
//             # # #";
//         assert_probs(
//             board,
//             Some(2),
//             &[((1, 0), 1.0), ((1, 1), 0.0), ((1, 2), 1.0)],
//         );
//     }

//     #[test]
//     fn edge_one_one_pattern_half() {
//         // Two "1"s with two unknowns and 1 mine — each is 50/50.
//         let board = "\
//             1 1\n\
//             # #";
//         assert_probs(board, Some(1), &[((1, 0), 0.5), ((1, 1), 0.5)]);
//     }

//     #[test]
//     fn uniform_distribution_around_center() {
//         // 8 unknowns surround a "2"; 2 mines total → each cell 2/8 = 0.25.
//         let board = "\
//             # # #\n\
//             # 2 #\n\
//             # # #";
//         let probs: Vec<((usize, usize), f32)> = [
//             (0, 0),
//             (0, 1),
//             (0, 2),
//             (1, 0),
//             (1, 2),
//             (2, 0),
//             (2, 1),
//             (2, 2),
//         ]
//         .iter()
//         .map(|&p| (p, 0.25))
//         .collect();
//         assert_probs(board, Some(2), &probs);
//     }

//     #[test]
//     fn corner_one_three_neighbors_third() {
//         // Corner "1" with three unknown neighbors → each 1/3.
//         let board = "\
//             1 #\n\
//             # #";
//         let third = 1.0_f32 / 3.0;
//         assert_probs(
//             board,
//             Some(1),
//             &[((0, 1), third), ((1, 0), third), ((1, 1), third)],
//         );
//     }

//     #[test]
//     fn forced_mine_count_match() {
//         // The "2"s on the ends each see 2 unknowns above (both mines);
//         // the "3" in the middle sees all 3 unknowns above (all mines).
//         let board = "\
//             # # #\n\
//             2 3 2";
//         assert_probs(
//             board,
//             Some(3),
//             &[((0, 0), 1.0), ((0, 1), 1.0), ((0, 2), 1.0)],
//         );
//     }

//     // -------------------------------------------------------
//     // All mines on the boundary → outside cells are safe
//     // -------------------------------------------------------

//     /// Board where the constraint frontier (cells adjacent to known numbers)
//     /// fully accounts for all mines. The far-away unknowns are
// unconstrained.     /// (No row of zeros — that would force the "1"'s
// neighbors to be safe.)     const BOUNDARY_BOARD: &str = "\
//         1 # # # #\n\
//         # # # # #";

//     /// In BOUNDARY_BOARD, these unknowns touch the lone "1" at (0,0).
//     fn boundary_cells() -> [(usize, usize); 3] {
//         [(0, 1), (1, 0), (1, 1)]
//     }

//     /// Unknowns with no known-number neighbor.
//     fn outside_cells() -> Vec<(usize, usize)> {
//         vec![(0, 2), (0, 3), (0, 4), (1, 2), (1, 3), (1, 4)]
//     }

//     #[test]
//     fn boundary_mines_fixed_count_picks_outside() {
//         // 1 mine total, "1" constraint → the mine must be on the boundary.
//         // Outside cells are guaranteed safe.
//         let (mut solver, grid) = solver_from_str(BOUNDARY_BOARD, Some(1), 0);
//         let mut sk = solver.with_known(&grid).unwrap();

//         // Boundary cells each have prob 1/3; outside cells have prob 0.
//         let third = 1.0_f32 / 3.0;
//         for (r, c) in boundary_cells() {
//             let p = sk.mine_probability(r, c).unwrap().unwrap();
//             assert!(
//                 (p - third).abs() < 1e-4,
//                 "Boundary cell ({r},{c}) prob {p}, expected ~1/3"
//             );
//         }
//         for (r, c) in outside_cells() {
//             let p = sk.mine_probability(r, c).unwrap().unwrap();
//             assert!(
//                 p < 1e-6,
//                 "Outside cell ({r},{c}) should be safe, got prob {p}"
//             );
//         }

//         // find_safe_cell must pick an outside cell.
//         let (r, c) = sk.find_safe_cell().expect("should find a safe cell");
//         assert!(
//             outside_cells().contains(&(r, c)),
//             "find_safe_cell returned ({r},{c}); expected an outside cell"
//         );
//     }

//     #[test]
//     fn boundary_mines_no_count_must_guess() {
//         // Without a fixed mine count, outside cells could each independently
//         // be mines, so the solver has nothing guaranteed safe.
//         let (mut solver, grid) = solver_from_str(BOUNDARY_BOARD, None, 0);
//         let mut sk = solver.with_known(&grid).unwrap();

//         let result = sk.find_safe_cell();
//         assert!(
//             matches!(result,
// Err(Error::SolverFailure(SolverFailure::MustGuess))),             "Expected
// MustGuess, got {result:?}"         );

//         // Outside cells should also report unknown probability.
//         for (r, c) in outside_cells() {
//             let p = sk.mine_probability(r, c).unwrap();
//             assert!(
//                 p.is_none(),
//                 "Without mine count, outside ({r},{c}) prob should be None,
// got {p:?}"             );
//         }
//     }
// }
