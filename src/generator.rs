use rand::{Rng, RngExt, SeedableRng, rngs::SmallRng};

use crate::{
    board::{Board, CellRef},
    solver::{self, Solver},
    utils::iter_neighbors,
};

/// Parameters controlling generation.
#[derive(Clone, Debug)]
pub struct GeneratorParams {
    /// Board width in cells.
    pub w: usize,
    /// Board height in cells.
    pub h: usize,
    /// `(row, col)` of the starting cell. The generator guarantees this cell
    /// is not a mine and is the first one revealed, so the solvability
    /// guarantee is built outwards from here.
    pub start: (usize, usize),
    /// RNG seed. The same seed plus the same parameters always produces the
    /// same board. This is also used to seed the Solver's hash.
    pub seed: u64,
    /// Maximum number of iterations before giving up with
    /// [`Error::OutOfAttempts`].
    pub max_iterations: usize,
    /// Maximum number of local shift attempts before resetting the board.
    pub max_local_iterations: usize,
    /// Number of mines. `None` picks a random count on each attempt;
    /// `Some(k)` fixes the count at `k`.
    pub n_mine: Option<usize>,
    /// If `true`, the solver used during generation is told the exact mine
    /// count, which lets it rule out some configurations it otherwise
    /// couldn't. If `false`, the generated board stays solvable even without
    /// telling the player the mine count.
    pub use_n_mines: bool,
}

impl GeneratorParams {
    pub fn new(w: usize, h: usize, start: (usize, usize)) -> Self {
        Self {
            w,
            h,
            start,
            seed: rand::rng().next_u64(),
            max_iterations: 10_000,
            max_local_iterations: 200,
            n_mine: None,
            use_n_mines: true,
        }
    }

    pub fn with_mine_count(mut self, n_mine: usize) -> Self {
        self.n_mine = Some(n_mine);
        self
    }

    pub fn with_seed(mut self, seed: u64) -> Self {
        self.seed = seed;
        self
    }
}

struct Generator<'a> {
    params: &'a GeneratorParams,
    rng: SmallRng,
    board: Board,
    solver: Solver,
    chunk_size: usize,
    reveals: Vec<(usize, usize)>,
    classified_positions: [Vec<(usize, usize)>; 6],
}

#[derive(Debug, Clone)]
pub enum Error {
    Solver(solver::Error),
    OutOfAttempts,
    TooManyMines,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Solver(solver_error) => f.write_fmt(format_args!("{}", solver_error)),
            Error::OutOfAttempts => f.write_str("Ran out of attempts generating board."),
            Error::TooManyMines => {
                f.write_str("Requested board with at least as many mines as cells.")
            }
        }
    }
}

impl<'a> Generator<'a> {
    fn reset(&mut self) {
        self.reveals.clear();
        self.board = loop {
            let n_mines = self
                .params
                .n_mine
                .unwrap_or_else(|| self.rng.random_range(0..self.params.w * self.params.h));

            let mut board =
                Board::random_mines(self.params.w, self.params.h, n_mines, &mut self.rng);

            if board
                .reveal(self.params.start.0, self.params.start.1)
                .is_some()
            {
                break board;
            }
        }
    }

    /// Bucket a cell into one of six classes based on (region, has-mine):
    ///   0/1 = outside the perimeter, no-mine/mine
    ///   2/3 = on the perimeter, no-mine/mine
    ///   4/5 = already revealed, no-mine/mine
    ///
    /// The low bit is always the mine flag, so `cls ^ 1` swaps mine <-> no-mine
    /// within the same region. `shift` relies on this when it puts a swapped
    /// cell back into the right bucket.
    fn classify_position(&self, row: usize, col: usize) -> usize {
        let is_mine = if self.board.at(row, col).mine() { 1 } else { 0 };
        if self.board.at(row, col).known().is_some() {
            return 4;
        };
        let on_perimeter = iter_neighbors(row, col, self.board.w(), self.board.h())
            .any(|(ni, nj)| self.board.at(ni, nj).known().is_some());
        (if on_perimeter { 2 } else { 0 }) + is_mine
    }

    /// Randomly swap board positions within `bbox` for `k` iterations. This
    /// looks at the classified positions and randomly decides which class
    /// to transfer from/to. Returns if successful.
    ///
    /// `pressure` ramps from 0 to 1 over the lifetime of `generate_loop`.
    /// Low pressure keeps swaps local to the perimeter which has a more even
    /// mine distribution. When we're running out of attempts, bias towards
    /// swapping perimeter and outside, which tends to push mines outwards to
    /// improve solvability.
    fn shift(&mut self, k: usize, bbox: [usize; 4], pressure: f32) -> bool {
        let threshold = 1.0 - (1.0 - pressure) * 0.7;

        for positions in &mut self.classified_positions {
            positions.clear();
        }
        for r in bbox[0]..bbox[1] {
            for c in bbox[2]..bbox[3] {
                let cls = self.classify_position(r, c);
                self.classified_positions[cls].push((r, c));
            }
        }

        let mut changed = false;
        for _ in 0..k {
            let p = self.rng.random::<f32>();
            let (a_base, b_base) = if p <= pressure {
                // Perimeter <-> outside.
                (0, 2)
            } else if p <= threshold {
                // Perimeter <-> perimeter.
                (2, 2)
            } else {
                // Perimeter <-> known.
                (2, 4)
            };

            // Always swap non-mines and mines. Otherwise, what are we doing?
            let (a, b) = if self.rng.random::<bool>() {
                (a_base + 1, b_base)
            } else {
                (a_base, b_base + 1)
            };

            if !self.classified_positions[a].is_empty() && !self.classified_positions[b].is_empty()
            {
                let removed_vs = [a, b].map(|cls| {
                    let positions = &mut self.classified_positions[cls];
                    let idx = self.rng.random_range(0..positions.len());
                    let (r, c) = positions.swap_remove(idx);
                    self.board.set_mine(r, c, !self.board.at(r, c).mine());
                    (r, c)
                });

                for (cls, v) in [a ^ 1, b ^ 1].into_iter().zip(removed_vs) {
                    self.classified_positions[cls].push(v);
                }

                changed = true;
            }
        }

        changed
    }
    fn n_mines(&self) -> Option<usize> {
        if self.params.use_n_mines {
            Some(self.board.n_mine())
        } else {
            None
        }
    }
    fn check_moves_valid(&mut self) -> Result<bool, solver::Error> {
        self.board.hide_all();
        if self
            .board
            .reveal(self.params.start.0, self.params.start.1)
            .is_none()
        {
            return Ok(false);
        }
        for &(r, c) in &self.reveals {
            let mut known_solver = self.solver.with_known(self.board.known(), self.n_mines())?;
            if known_solver.can_be_mine(r, c)? {
                return Ok(false);
            }
            self.board
                .reveal(r, c)
                .expect("just checked it's not a mine");
        }
        Ok(true)
    }

    /// Shifts a chunk that overlaps with the perimeter until it contains a
    /// guaranteed non-mine, returning if successful.
    fn shift_random_chunk(&mut self, pressure: f32) -> Result<bool, solver::Error> {
        let old_board = self.board.clone();
        let perimeter_cells = self.board.perimeter().collect::<Vec<_>>();
        let rnd_cell = perimeter_cells[self.rng.random_range(0..perimeter_cells.len())];

        let r1 = self.rng.random_range(
            rnd_cell.row().saturating_sub(self.chunk_size - 1)
                ..=rnd_cell.row().min(self.board.h() - self.chunk_size),
        );
        let c1 = self.rng.random_range(
            rnd_cell.col().saturating_sub(self.chunk_size - 1)
                ..=rnd_cell.col().min(self.board.w() - self.chunk_size),
        );
        let r2 = r1 + self.chunk_size;
        let c2 = c1 + self.chunk_size;

        let in_bbox = |x: CellRef| (r1..r2).contains(&x.row()) && (c1..c2).contains(&x.col());
        debug_assert!(in_bbox(rnd_cell));

        for _attempt in 0..25 {
            if !self.shift(2, [r1, r2, c1, c2], pressure) {
                self.shift(1, [0, self.board.h(), 0, self.board.w()], pressure);
                continue;
            }

            // This is not exact; that happens below.
            if self
                .reveals
                .iter()
                .chain(&[self.params.start])
                .any(|&(r, c)| self.board.at(r, c).mine())
            {
                continue;
            }

            let mut known_solver = self.solver.with_known(self.board.known(), self.n_mines())?;
            let potential_non_mine = self
                .board
                .perimeter()
                .find_map(|x| {
                    if !x.mine() && in_bbox(x) {
                        match known_solver.can_be_mine(x.row(), x.col()) {
                            Ok(true) => None,
                            Ok(false) => Some(Ok(())),
                            Err(e) => Some(Err(e)),
                        }
                    } else {
                        None
                    }
                })
                .transpose()?;

            if let Some(()) = potential_non_mine
                && self.check_moves_valid()?
            {
                return Ok(true);
            }
        }

        self.board = old_board;
        Ok(false)
    }

    fn generate_loop(&mut self) -> Result<bool, solver::Error> {
        let mut attempts = 0usize;
        self.reset();

        for iter in 0..self.params.max_iterations {
            let pressure = iter as f32 / self.params.max_iterations as f32;
            if !self.board.solved() && !self.shift_random_chunk(pressure)? {
                attempts += 1;

                // Reset board entirely if our local/small adjustments failed too much.
                if attempts > self.params.max_local_iterations {
                    attempts = 0;
                    self.reset();
                }

                continue;
            }

            loop {
                if self.board.solved() {
                    return Ok(true);
                }

                let mut known_solver =
                    self.solver.with_known(self.board.known(), self.n_mines())?;

                // Optimization: mines on our board can never be guaranteed safe.
                match known_solver.find_safe_cell_filtering(|idx| !self.board.mines()[idx]) {
                    Ok((r, c)) => {
                        debug_assert!(!known_solver.can_be_mine(r, c).unwrap());
                        self.reveals.push((r, c));
                        self.board.reveal(r, c).expect("should be safe")
                    }
                    Err(solver::Error::SolverFailure(solver::SolverFailure::MustGuess)) => break,
                    Err(e) => return Err(e),
                };
            }

            debug_assert!(self.check_moves_valid().unwrap());
        }

        Ok(false)
    }
}

/// Attempt to generate a board with the given parameters.
pub fn generate(params: GeneratorParams) -> Result<Board, Error> {
    if params.n_mine.is_some_and(|x| x >= params.w * params.h) {
        return Err(Error::TooManyMines);
    }

    let rng = SmallRng::seed_from_u64(params.seed);
    let chunk_size = params.h.min(params.w).min(5);

    let mut generator = Generator {
        params: &params,
        board: Board::empty(params.w, params.h),
        rng,
        chunk_size,
        reveals: vec![],
        solver: Solver::new(params.w, params.h, params.seed),
        classified_positions: [const { Vec::new() }; 6],
    };

    if generator.generate_loop().map_err(Error::Solver)? {
        generator.board.hide_all();
        generator.board.reveal(params.start.0, params.start.1);
        Ok(generator.board)
    } else {
        Err(Error::OutOfAttempts)
    }
}

#[cfg(test)]
mod tests {
    use crate::{generator, solver};

    #[test]
    fn generate_extremes() {
        assert!(
            generator::generate(
                generator::GeneratorParams::new(5, 5, (3, 3))
                    .with_mine_count(24)
                    .with_seed(42)
            )
            .unwrap()
            .iter()
            .all(|x| x.mine() || (x.row() == 3 && x.col() == 3))
        );
        assert!(
            generator::generate(
                generator::GeneratorParams::new(5, 5, (3, 3))
                    .with_mine_count(0)
                    .with_seed(42)
            )
            .unwrap()
            .solved()
        );
    }

    #[test]
    fn too_many_mines() {
        assert!(matches!(
            generator::generate(
                generator::GeneratorParams::new(5, 5, (3, 3))
                    .with_seed(123)
                    .with_mine_count(25)
            ),
            Err(generator::Error::TooManyMines)
        ))
    }

    #[test]
    fn generate_small() {
        let generated_board =
            generator::generate(generator::GeneratorParams::new(5, 5, (3, 3)).with_seed(123))
                .unwrap();
        println!("generated (random # mines):\n{}", generated_board);

        let generated_board = generator::generate(
            generator::GeneratorParams::new(5, 5, (3, 3))
                .with_seed(123)
                .with_mine_count(15),
        )
        .unwrap();
        println!("generated (15 mines):\n{}", generated_board);
    }

    // This takes a while and should only be done with optimization.
    #[test]
    #[ignore]
    fn generate_solvable() {
        let generated_boards = (0..30)
            .map(|seed| {
                generator::generate(
                    generator::GeneratorParams::new(16, 16, (4, 4))
                        .with_mine_count(90 + seed)
                        .with_seed(seed as u64),
                )
                .unwrap()
            })
            .collect::<Vec<_>>();

        for (i, i_board) in generated_boards.iter().enumerate() {
            println!("generated board:\n{}", i_board);
            for j_board in &generated_boards[0..i] {
                if j_board.mines() == i_board.mines() {
                    panic!("generated same board twice!");
                }
            }

            let mut solver = solver::Solver::new(i_board.w(), i_board.h(), 123);
            let mut i_board_cpy = i_board.clone();
            let mut n_step = 0;
            while !i_board_cpy.solved() {
                let mut known = solver
                    .with_known(i_board_cpy.known(), Some(i_board_cpy.n_mine()))
                    .unwrap();
                let (r, c) = known.find_safe_cell().unwrap();
                assert!(i_board_cpy.reveal(r, c).expect("should be safe") > 0);
                n_step += 1;
            }
            println!("solved after {n_step} steps");
        }
    }
}
