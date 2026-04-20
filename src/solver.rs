mod base;
mod checker;
mod set_solver;

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
#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::{board_from_str, board_to_str, iter_neighbors, position_to_row_col};
    use rand::prelude::*;

    // -------------------------------------------------------
    // Helpers
    // -------------------------------------------------------

    type Grid = Vec<Option<usize>>;

    struct TestBoard {
        grid: Grid,
        w: usize,
        h: usize,
    }

    impl TestBoard {
        fn parse(s: &str) -> Self {
            let board = board_from_str(s).unwrap();
            Self {
                grid: board.to_known().collect(),
                w: board.w,
                h: board.h,
            }
        }

        fn solver(&self, seed: u64) -> Solver {
            Solver::new(self.w, self.h, seed)
        }

        fn unknown_count(&self) -> usize {
            self.grid.iter().filter(|cell| cell.is_none()).count()
        }
    }

    fn board(s: &str) -> TestBoard {
        TestBoard::parse(s)
    }

    /// Assert exact mine probabilities at given cells. Panics with a clear
    /// message on mismatch. Requires a fixed mine count (`n_mines =
    /// Some(...)`).
    fn assert_probs(board: &str, n_mines: usize, expected: &[((usize, usize), f32)]) {
        let board = TestBoard::parse(board);
        let mut solver = board.solver(0);
        let mut sk = solver.with_known(&board.grid, Some(n_mines)).unwrap();
        for &((r, c), want) in expected {
            let got = sk
                .mine_probability(r, c)
                .expect("mine_probability returned Err")
                .expect("expected Some(prob) with fixed mine count");
            assert!(
                (got - want).abs() < 1e-4,
                "({r},{c}): expected prob {want}, got {got}"
            );
        }
    }

    // -------------------------------------------------------
    // Construction & size
    // -------------------------------------------------------

    #[test]
    fn new_solver_has_correct_size() {
        let s = Solver::new(8, 5, 0);
        assert_eq!(s.size(), (8, 5));
    }

    #[test]
    fn size_matches_board_dimensions() {
        let board = board("1 1 1\n1 # 1\n1 1 1");
        let solver = board.solver(0);
        assert_eq!(solver.size(), (board.w, board.h));
    }

    // -------------------------------------------------------
    // board_to_str / board_from_str round-trips
    // -------------------------------------------------------

    #[test]
    fn board_str_roundtrip_numbers_and_unknown() {
        let board = "0 1 #\n0 2 #\n0 1 #";
        let parsed = TestBoard::parse(board);
        assert_eq!(board_to_str(&parsed.grid, parsed.w, None), board);
    }

    #[test]
    fn board_str_roundtrip_with_mine_overlay() {
        // "X" in the string means a mine; round-trip via to_known / to_mines.
        let board = "1 X #\n0 1 #";
        let result = board_from_str(board).unwrap();
        let known: Vec<Option<usize>> = result.to_known().collect();
        let mines: Vec<bool> = result.to_mines().collect();
        assert_eq!(board_to_str(&known, result.w, Some(&mines)), board);
    }

    #[test]
    fn board_from_str_parses_mine_cells() {
        use crate::utils::BoardFromStrCell::*;
        let result = board_from_str("1 X #").unwrap();
        assert_eq!(result.cells, vec![Known(1), Mine, Unknown]);
        assert_eq!(result.w, 3);
        assert_eq!(result.h, 1);
    }

    #[test]
    fn board_str_roundtrip_random() {
        let mut rng = SmallRng::seed_from_u64(42);
        for _ in 0..20 {
            let w = rng.random_range(2usize..=10);
            let h = rng.random_range(2usize..=10);
            let grid: Vec<Option<usize>> = (0..w * h)
                .map(|_| {
                    if rng.random_bool(0.3) {
                        None
                    } else {
                        Some(rng.random_range(0usize..=8))
                    }
                })
                .collect();
            let s = board_to_str(&grid, w, None);
            let parsed = TestBoard::parse(&s);
            assert_eq!((w, &grid), (parsed.w, &parsed.grid));
        }
    }

    // -------------------------------------------------------
    // Utils: iter_neighbors, position_to_row_col
    // -------------------------------------------------------

    #[test]
    fn iter_neighbors_corner_gives_three() {
        let ns: Vec<_> = iter_neighbors(0, 0, 5, 5).collect();
        assert_eq!(ns.len(), 3);
        assert!(ns.contains(&(0, 1)));
        assert!(ns.contains(&(1, 0)));
        assert!(ns.contains(&(1, 1)));
    }

    #[test]
    fn iter_neighbors_center_gives_eight() {
        let ns: Vec<_> = iter_neighbors(2, 2, 5, 5).collect();
        assert_eq!(ns.len(), 8);
    }

    #[test]
    fn iter_neighbors_edge_gives_five() {
        let ns: Vec<_> = iter_neighbors(0, 2, 5, 5).collect();
        assert_eq!(ns.len(), 5);
    }

    #[test]
    fn position_to_row_col_correct() {
        assert_eq!(position_to_row_col(0, 5), (0, 0));
        assert_eq!(position_to_row_col(4, 5), (0, 4));
        assert_eq!(position_to_row_col(5, 5), (1, 0));
        assert_eq!(position_to_row_col(7, 3), (2, 1));
    }

    // -------------------------------------------------------
    // with_known — error cases
    // -------------------------------------------------------

    #[test]
    fn with_known_wrong_size_errors() {
        let mut s = Solver::new(3, 3, 0);
        assert!(matches!(
            s.with_known(&vec![None::<usize>; 8], Some(1)),
            Err(Error::InvalidGridSize)
        ));
        assert!(matches!(
            s.with_known(&vec![None::<usize>; 10], Some(1)),
            Err(Error::InvalidGridSize)
        ));
    }

    #[test]
    fn with_known_correct_size_succeeds() {
        let mut s = Solver::new(3, 3, 0);
        assert!(s.with_known(&vec![None::<usize>; 9], Some(1)).is_ok());
    }

    // -------------------------------------------------------
    // Out-of-bounds errors
    // -------------------------------------------------------

    #[test]
    fn mine_probability_out_of_range() {
        let board = board("0 0 0\n0 1 #\n0 1 #");
        let mut solver = board.solver(0);
        let mut sk = solver.with_known(&board.grid, Some(1)).unwrap();
        assert!(matches!(
            sk.mine_probability(10, 0),
            Err(Error::RowColumnOutOfRange)
        ));
        assert!(matches!(
            sk.mine_probability(0, 10),
            Err(Error::RowColumnOutOfRange)
        ));
    }

    #[test]
    fn can_be_mine_out_of_range() {
        let board = board("0 1 #");
        let mut solver = board.solver(0);
        let mut sk = solver.with_known(&board.grid, Some(1)).unwrap();
        assert!(matches!(
            sk.can_be_mine(5, 0),
            Err(Error::RowColumnOutOfRange)
        ));
    }

    // -------------------------------------------------------
    // Error display
    // -------------------------------------------------------

    #[test]
    fn error_display_messages() {
        let cases = [
            (Error::InvalidGridSize, "Invalid size"),
            (Error::RowColumnOutOfRange, "out of range"),
            (
                Error::SolverFailure(SolverFailure::Empty),
                "completely blank",
            ),
            (Error::SolverFailure(SolverFailure::MustGuess), "guaranteed"),
            (
                Error::SolverFailure(SolverFailure::Solved),
                "already solved",
            ),
            (
                Error::SolverFailure(SolverFailure::Unsolvable),
                "unsolvable",
            ),
        ];
        for (err, needle) in cases {
            let msg = err.to_string().to_lowercase();
            assert!(
                msg.contains(&needle.to_lowercase()),
                "Expected '{needle}' in \"{msg}\""
            );
        }
    }

    // -------------------------------------------------------
    // Basic solver behaviour
    // -------------------------------------------------------

    #[test]
    fn fully_revealed_board_is_solved() {
        let board = board("0 0 0\n0 0 0\n0 0 0");
        let mut solver = board.solver(0);
        let result = solver
            .with_known(&board.grid, Some(0))
            .unwrap()
            .find_safe_cell();
        assert!(
            matches!(result, Err(Error::SolverFailure(SolverFailure::Solved))),
            "Expected Solved, got {result:?}"
        );
    }

    #[test]
    fn all_unknown_board_cannot_find_safe_cell() {
        let mut s = Solver::new(3, 3, 0);
        let grid = vec![None::<usize>; 9];
        let result = s.with_known(&grid, Some(1)).unwrap().find_safe_cell();
        assert!(
            result.is_err(),
            "All-unknown board should not find a safe cell"
        );
    }

    #[test]
    fn single_mine_identified_as_prob_one() {
        // (1,2) is the only unknown, and there is 1 mine → prob must be 1.0.
        let board = board("0 1 1\n0 1 #\n0 1 1");
        let mut solver = board.solver(0);
        let p = solver
            .with_known(&board.grid, Some(1))
            .unwrap()
            .mine_probability(1, 2)
            .unwrap()
            .unwrap();
        assert!(
            (p - 1.0).abs() < 1e-6,
            "Sole unknown with 1 mine: expected prob 1.0, got {p}"
        );
    }

    #[test]
    fn safe_cell_is_unknown() {
        // (1,2) is mine, (0,2) is safe. Solver should return (0,2).
        let board = board("0 1 #\n0 1 #\n0 0 0");
        let mut solver = board.solver(0);
        let (r, c) = solver
            .with_known(&board.grid, Some(1))
            .unwrap()
            .find_safe_cell()
            .unwrap();
        assert!(
            board.grid[r * board.w + c].is_none(),
            "Safe cell ({r},{c}) should be unknown"
        );
    }

    #[test]
    fn can_be_mine_consistent_with_probability() {
        // For deterministic cells: can_be_mine should agree with prob 0 / prob 1.
        let board = board("1 2 1\n# # #");
        let mut solver = board.solver(0);
        let mut sk = solver.with_known(&board.grid, Some(2)).unwrap();
        // (1,1) has prob 0 — cannot be a mine.
        assert!(!sk.can_be_mine(1, 1).unwrap(), "(1,1) should not be a mine");
        // (1,0) and (1,2) have prob 1 — must be mines.
        assert!(sk.can_be_mine(1, 0).unwrap(), "(1,0) should be a mine");
        assert!(sk.can_be_mine(1, 2).unwrap(), "(1,2) should be a mine");
    }

    // -------------------------------------------------------
    // Determinism across seeds
    // -------------------------------------------------------

    #[test]
    fn same_seed_gives_same_result() {
        let board = board("# # #\n# 3 #\n# # #");
        for seed in [0u64, 1, 999] {
            let mut s1 = board.solver(seed);
            let mut s2 = board.solver(seed);
            let r1 = s1
                .with_known(&board.grid, Some(3))
                .unwrap()
                .find_safe_cell();
            let r2 = s2
                .with_known(&board.grid, Some(3))
                .unwrap()
                .find_safe_cell();
            assert_eq!(format!("{r1:?}"), format!("{r2:?}"), "seed {seed}");
        }
    }

    // -------------------------------------------------------
    // Exact probability values
    // -------------------------------------------------------

    #[test]
    fn one_two_one_exact_probs() {
        // Constraints: (1,0)+(1,1)=1, (1,0)+(1,1)+(1,2)=2, (1,1)+(1,2)=1
        // → unique solution: (1,0)=mine, (1,1)=safe, (1,2)=mine.
        assert_probs(
            "1 2 1\n# # #",
            2,
            &[((1, 0), 1.0), ((1, 1), 0.0), ((1, 2), 1.0)],
        );
    }

    #[test]
    fn one_one_edge_is_half() {
        // Two unknowns, each constrained once by a "1" → 50/50.
        assert_probs("1 1\n# #", 1, &[((1, 0), 0.5), ((1, 1), 0.5)]);
    }

    #[test]
    fn uniform_ring_around_center() {
        // 8 symmetric unknowns, 2 mines → each exactly 0.25.
        let cells = [
            (0, 0),
            (0, 1),
            (0, 2),
            (1, 0),
            (1, 2),
            (2, 0),
            (2, 1),
            (2, 2),
        ];
        let expected: Vec<_> = cells.iter().map(|&p| (p, 0.25f32)).collect();
        assert_probs("# # #\n# 2 #\n# # #", 2, &expected);
    }

    #[test]
    fn corner_one_three_neighbors_each_third() {
        // "1" in corner; 3 unknown neighbors; 1 mine → each 1/3.
        let third = 1.0f32 / 3.0;
        assert_probs(
            "1 #\n# #",
            1,
            &[((0, 1), third), ((1, 0), third), ((1, 1), third)],
        );
    }

    #[test]
    fn all_forced_mines() {
        // (0,0) adj to "2" seeing only (0,0)&(0,1); "3" seeing all three.
        // Unique solution: all three are mines.
        assert_probs(
            "# # #\n2 3 2",
            3,
            &[((0, 0), 1.0), ((0, 1), 1.0), ((0, 2), 1.0)],
        );
    }

    #[test]
    fn one_two_two_two_one_middle_forced() {
        // Two solutions: mines at {0,2,3} or {1,2,4}. Col 2 is always a mine.
        assert_probs(
            "# # # # #\n1 2 2 2 1",
            3,
            &[
                ((0, 2), 1.0),
                ((0, 0), 0.5),
                ((0, 1), 0.5),
                ((0, 3), 0.5),
                ((0, 4), 0.5),
            ],
        );
    }

    // -------------------------------------------------------
    // Probability invariants
    // -------------------------------------------------------

    #[test]
    fn probabilities_sum_to_mine_count() {
        let board = board("# # # #\n# 3 2 #\n# 2 1 1\n# 1 0 0");
        let mut solver = board.solver(0);
        let n_mines = 4;
        let mut sk = solver.with_known(&board.grid, Some(n_mines)).unwrap();
        let sum: f64 = (0..board.h)
            .flat_map(|r| (0..board.w).map(move |c| (r, c)))
            .filter(|&(r, c)| board.grid[r * board.w + c].is_none())
            .map(|(r, c)| sk.mine_probability(r, c).unwrap().unwrap() as f64)
            .sum();
        assert!(
            (sum - n_mines as f64).abs() < 0.1,
            "Sum of probs {sum:.3} should ≈ {n_mines}"
        );
    }

    #[test]
    fn probability_bounds_always_zero_to_one() {
        let board = board("# # #\n# 2 #\n# # #");
        let mut solver = board.solver(0);
        let mut sk = solver.with_known(&board.grid, Some(2)).unwrap();
        for r in 0..board.h {
            for c in 0..board.w {
                if board.grid[r * board.w + c].is_none() {
                    let p = sk.mine_probability(r, c).unwrap().unwrap();
                    assert!((0.0..=1.0).contains(&p), "({r},{c}) prob {p} out of [0,1]");
                }
            }
        }
    }

    #[test]
    fn symmetric_board_symmetric_probs() {
        let board = board("# # #\n# 4 #\n# # #");
        let mut solver = board.solver(0);
        let mut sk = solver.with_known(&board.grid, Some(4)).unwrap();
        let corner_p = sk.mine_probability(0, 0).unwrap().unwrap();
        let edge_p = sk.mine_probability(0, 1).unwrap().unwrap();
        for (r, c) in [(0, 2), (2, 0), (2, 2)] {
            assert!((sk.mine_probability(r, c).unwrap().unwrap() - corner_p).abs() < 1e-6);
        }
        for (r, c) in [(1, 0), (1, 2), (2, 1)] {
            assert!((sk.mine_probability(r, c).unwrap().unwrap() - edge_p).abs() < 1e-6);
        }
    }

    #[test]
    fn known_cell_cannot_be_mine() {
        let board = board("0 1 #\n0 1 #\n0 0 0");
        let mut solver = board.solver(0);
        let mut sk = solver.with_known(&board.grid, Some(1)).unwrap();
        match sk.mine_probability(0, 0) {
            Ok(Some(p)) => assert!(p < 1e-6, "Known cell prob should be 0, got {p}"),
            Ok(None) | Err(_) => {}
        }
        assert!(
            !sk.can_be_mine(0, 0).unwrap(),
            "Known cell should not can_be_mine"
        );
    }

    // -------------------------------------------------------
    // No fixed mine count → None for ambiguous cells
    // -------------------------------------------------------

    #[test]
    fn no_mine_count_ambiguous_returns_none() {
        let board = board("# # #\n# 1 #\n# # #");
        let mut solver = board.solver(0);
        let mut sk = solver.with_known(&board.grid, None).unwrap();
        for r in 0..board.h {
            for c in 0..board.w {
                if board.grid[r * board.w + c].is_none() {
                    match sk.mine_probability(r, c) {
                        Ok(None) => {}
                        Ok(Some(p)) => assert!(
                            p == 0.0 || p == 1.0,
                            "Without mine count, non-None prob must be 0 or 1, got {p}"
                        ),
                        Err(_) => {}
                    }
                }
            }
        }
    }

    // -------------------------------------------------------
    // All mines on constraint boundary → outside cells safe
    // -------------------------------------------------------

    // Board: one "1" in top-left corner; 1 mine total.
    // Boundary = {(0,1),(1,0),(1,1)}: the three unknowns adjacent to the "1".
    // Outside = {(0,2)..(1,4)}: unknowns with no known neighbor.
    const BOUNDARY_BOARD: &str = "1 # # # #\n# # # # #";
    const BOUNDARY: [(usize, usize); 3] = [(0, 1), (1, 0), (1, 1)];
    const OUTSIDE: [(usize, usize); 6] = [(0, 2), (0, 3), (0, 4), (1, 2), (1, 3), (1, 4)];

    #[test]
    fn boundary_fixed_count_outside_safe() {
        let board = board(BOUNDARY_BOARD);
        let mut solver = board.solver(0);
        let mut sk = solver.with_known(&board.grid, Some(1)).unwrap();

        let third = 1.0f32 / 3.0;
        for (r, c) in BOUNDARY {
            let p = sk.mine_probability(r, c).unwrap().unwrap();
            assert!(
                (p - third).abs() < 1e-4,
                "Boundary ({r},{c}) prob {p}, expected ~1/3"
            );
        }
        for (r, c) in OUTSIDE {
            let p = sk.mine_probability(r, c).unwrap().unwrap();
            assert!(p < 1e-6, "Outside ({r},{c}) should be prob 0, got {p}");
            assert!(
                !sk.can_be_mine(r, c).unwrap(),
                "Outside ({r},{c}) cannot be mine"
            );
        }

        let (r, c) = sk
            .find_safe_cell()
            .expect("should find a safe outside cell");
        assert!(
            OUTSIDE.contains(&(r, c)),
            "find_safe_cell returned ({r},{c}), expected outside"
        );
    }

    #[test]
    fn boundary_no_count_must_guess() {
        let board = board(BOUNDARY_BOARD);
        let mut solver = board.solver(0);
        let mut sk = solver.with_known(&board.grid, None).unwrap();

        let result = sk.find_safe_cell();
        assert!(
            matches!(result, Err(Error::SolverFailure(SolverFailure::MustGuess))),
            "Expected MustGuess without mine count, got {result:?}"
        );
        for (r, c) in OUTSIDE {
            let p = sk.mine_probability(r, c).unwrap();
            assert!(
                p.is_none(),
                "Outside ({r},{c}) prob should be None, got {p:?}"
            );
        }
    }

    // -------------------------------------------------------
    // find_safe_cell_filtering
    // -------------------------------------------------------

    #[test]
    fn filtering_steers_toward_preferred_cells() {
        // Outside cells are all guaranteed safe. Filter to only prefer the
        // rightmost column; solver should pick from there (or anywhere safe).
        let board = board(BOUNDARY_BOARD);
        let mut solver = board.solver(0);
        let mut sk = solver.with_known(&board.grid, Some(1)).unwrap();

        // Filter: prefer column 4 (rightmost).
        let result = sk.find_safe_cell_filtering(|pos| pos % board.w == 4);
        let (r, c) = result.expect("should find a safe cell with filter");
        // The result must still be a safe (unknown) cell.
        assert!(
            board.grid[r * board.w + c].is_none(),
            "Filtered safe cell must be unknown"
        );
    }

    // -------------------------------------------------------
    // Random boards: probabilities stay in [0, 1]
    // -------------------------------------------------------

    #[test]
    fn random_boards_probs_in_range() {
        let mut rng = SmallRng::seed_from_u64(777);
        for _ in 0..30 {
            let w = rng.random_range(3usize..=6);
            let h = rng.random_range(3usize..=6);
            let n_mines = rng.random_range(1usize..=(w * h / 3).max(1));
            let board = TestBoard {
                grid: (0..w * h)
                    .map(|_| {
                        if rng.random_bool(0.4) {
                            Some(rng.random_range(0usize..=8))
                        } else {
                            None
                        }
                    })
                    .collect(),
                w,
                h,
            };
            if board.unknown_count() == 0 {
                continue;
            }
            let mut solver = board.solver(rng.random());
            let Ok(mut sk) = solver.with_known(&board.grid, Some(n_mines)) else {
                continue;
            };
            for r in 0..board.h {
                for c in 0..board.w {
                    if board.grid[r * board.w + c].is_none() {
                        if let Ok(Some(p)) = sk.mine_probability(r, c) {
                            assert!(
                                (0.0..=1.0).contains(&p),
                                "({r},{c}) prob {p} out of [0,1] on {h}×{w} board"
                            );
                        }
                    }
                }
            }
        }
    }
}
