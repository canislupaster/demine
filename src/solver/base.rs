use std::fmt::Debug;

use rand::{Rng, SeedableRng, rngs::SmallRng};

use crate::utils::iter_neighbors;

pub mod counters {
    use std::sync::atomic::AtomicUsize;

    pub struct DbgCounter {
        name: &'static str,
        counter: AtomicUsize,
    }

    impl DbgCounter {
        const fn new(name: &'static str) -> Self {
            Self {
                name,
                counter: AtomicUsize::new(0),
            }
        }

        #[cfg(debug_assertions)]
        pub fn add(&self) {
            self.counter
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }

        #[cfg(not(debug_assertions))]
        pub const fn add(&self) {}
    }

    pub static STATE_COUNT: DbgCounter = DbgCounter::new("state count");
    pub static CACHE_HIT: DbgCounter = DbgCounter::new("cache hit");
    pub static CSP_ITERATIONS: DbgCounter = DbgCounter::new("csp iterations");

    pub fn print() {
        if cfg!(debug_assertions) {
            println!("counters:");
            for counter in [&STATE_COUNT, &CACHE_HIT, &CSP_ITERATIONS] {
                println!(
                    "{}: {}",
                    counter.name,
                    counter.counter.load(std::sync::atomic::Ordering::SeqCst)
                );
                counter
                    .counter
                    .store(0, std::sync::atomic::Ordering::SeqCst);
            }
        } else {
            println!("counters not recorded; debug assertions disabled")
        }
    }
}

/// A state is a list of constraints for cells on the perimeter of a component
/// and which cells need to be conditioned on (cells flagged as decide).
pub type State = Vec<Cell>;

/// The i-th value is the probability there are that many mines across the
/// decide elements of the state, assuming mines are randomly placed. This
/// should always have no trailing zeros and its length should be capped at
/// n_mines+1.
pub type StateProbs = Vec<f32>;

/// Immutable data used by CSPSolver and the real thing.
#[derive(Debug)]
pub struct Solver {
    pub h: usize,
    pub w: usize,
    pub sz: usize,

    pub neighbors: Vec<Vec<usize>>,
    pub adj_indices: Vec<Vec<(usize, usize)>>,
    /// Precomputed bitmasks for brute-forcing mines set in a CheckCell.
    pub ways: Vec<Vec<Vec<usize>>>,
    pub base: Vec<u64>,
}

/// Mutable scratchpad used by both solvers.
#[derive(Debug, Clone)]
pub struct SolverUtils {
    pub tmp_cell_flag: Vec<CellFlag>,
    pub tmp_cell_idx: Vec<Option<usize>>,
    pub tmp_cell_msk: Vec<usize>,
    pub tmp_cell_count: Vec<i32>,

    /// Highly misleading naming.
    pub dfs: Vec<usize>,
    pub new_dfs: Vec<Cell>,
    pub tmp_part: Vec<Cell>,
    pub visit_i: usize,
    pub visited: Vec<usize>,
    pub known_tmp: Vec<usize>,

    pub old_known: Vec<Option<usize>>,
    pub known_neighbors: Vec<Vec<usize>>,
}

/// This represents a failure to choose a cell to reveal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Failure {
    /// One must guess (exact probabilities are given by `mine_probability`).
    MustGuess,
    /// The board is already solved.
    Solved,
    /// The board is completely empty and has not been started.
    Empty,
    /// The numbers are inconsistent with each other.
    Unsolvable,
}

pub const IMPOSSIBLE: f32 = 0.0;

pub const CELL_POSITION_SHIFT: u32 = 2;
pub const CELL_FLAG_MSK: u32 = (1 << CELL_POSITION_SHIFT) - 1;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CellFlag(pub u32);

// Each cell is either marked as safe, a mine, or unknown (Decide).
pub const CELL_FLAG_NONE: CellFlag = CellFlag(0);
pub const CELL_FLAG_NO_MINE: CellFlag = CellFlag(1);
pub const CELL_FLAG_DECIDE: CellFlag = CellFlag(2);
pub const CELL_FLAG_MINE: CellFlag = CellFlag(3);

#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Cell(u32);

impl Cell {
    pub fn new(flag: CellFlag, position: usize) -> Self {
        Cell(flag.0 | ((position as u32) << CELL_POSITION_SHIFT))
    }
    pub fn flag(self) -> CellFlag {
        CellFlag(self.0 & CELL_FLAG_MSK)
    }
    pub fn set_flag(&mut self, f: CellFlag) {
        self.0 = f.0 | (self.0 & !CELL_FLAG_MSK);
    }
    pub fn position(self) -> usize {
        (self.0 >> CELL_POSITION_SHIFT) as usize
    }
}

impl Debug for Cell {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_fmt(format_args!(
            "Cell {{ position: {}, flag: {} }}",
            self.position(),
            self.flag().0
        ))
    }
}

// Pad each mask on the left/right in addition to the 3 cells in each row of
// neighbors of a cell. This allows us to shift up/down by a row by shifting by
// MSK_STRIDE and then masking out the padding.
const MSK_LPAD: usize = 1;
const MSK_RPAD: usize = 1;

// How many bits are reserved for each row of the adjacency bitmask.
const MSK_STRIDE: usize = MSK_LPAD + MSK_RPAD + 3;

pub fn adj_index(x: usize, y: usize, w: usize) -> Option<usize> {
    let col = (MSK_LPAD + 1 + x % w).wrapping_sub(y % w);
    if col >= MSK_STRIDE {
        return None;
    }
    let row = (MSK_LPAD + 1 + x / w).wrapping_sub(y / w);
    if row >= MSK_STRIDE {
        return None;
    }
    Some(MSK_STRIDE * row + col)
}

pub fn adj_diff(x: usize, y: usize, w: usize) -> isize {
    (MSK_STRIDE as isize) * ((x / w) as isize - (y / w) as isize)
        + ((x % w) as isize - (y % w) as isize)
}

pub fn shift_msk(a: usize, b: isize) -> usize {
    if b >= 0 { a << b } else { a >> -b }
}

/// Maximum number of bits set in a CheckCell.
pub const MAX_NUM_WAYS: usize = 14;

/// `count` mines are contained in `msk` around `pos`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CheckCell {
    pub pos: usize,
    pub msk: usize,
    pub count: i32,
}

impl Solver {
    pub fn new(w: usize, h: usize, seed: u64) -> Self {
        let sz = h * w;
        let mut neighbors = vec![Vec::new(); sz];
        let mut adj_indices = vec![vec![]; sz];
        for i in 0..h {
            for j in 0..w {
                let idx = i * w + j;
                for (ni, nj) in iter_neighbors(i, j, w, h) {
                    let neighbor_idx = ni * w + nj;
                    neighbors[idx].push(neighbor_idx);
                    adj_indices[idx].push((
                        neighbor_idx,
                        adj_index(idx, neighbor_idx, w).expect("neighbors should have adj index"),
                    ));
                }
            }
        }

        let mut rng = SmallRng::seed_from_u64(seed);
        let base: Vec<u64> = (0..sz).map(|_| rng.next_u64()).collect();

        let ways: Vec<Vec<Vec<usize>>> = (0..=MAX_NUM_WAYS)
            .map(|k| {
                let mut msks_by_ones = vec![Vec::new(); k + 1];
                for j in 0..(1usize << k) {
                    msks_by_ones[j.count_ones() as usize].push(j);
                }
                msks_by_ones
            })
            .collect::<Vec<_>>();

        Solver {
            h,
            w,
            sz,
            neighbors,
            adj_indices,
            ways,
            base,
        }
    }

    pub fn for_in_cell(
        &self,
        cell: &CheckCell,
        mut f: impl FnMut(usize) -> Option<()>,
    ) -> Option<()> {
        let mut i: usize = 0;
        let mut msk = cell.msk;
        while msk > 0 {
            let sh = msk.trailing_zeros() as usize;
            msk >>= sh + 1;
            i += sh;
            let off = self.w * (1 + MSK_LPAD) + (1 + MSK_LPAD);
            let di = i / MSK_STRIDE;
            let dj = i % MSK_STRIDE;
            f(cell.pos + off - self.w * di - dj)?;
            i += 1;
        }
        Some(())
    }

    pub fn state_str(
        &self,
        state: &State,
        known: &[Option<usize>],
        state_idxes: &[usize],
    ) -> String {
        let pos_str = |pos| {
            state
                .iter()
                .enumerate()
                .find_map(|(idx, cell)| {
                    if cell.position() == pos {
                        if state_idxes.contains(&idx) {
                            return Some("%");
                        }
                        Some(match cell.flag() {
                            CELL_FLAG_DECIDE => "?",
                            CELL_FLAG_MINE => "X",
                            CELL_FLAG_NO_MINE => "-",
                            _ => panic!("unexpected cell flag {}", cell.flag().0),
                        })
                    } else {
                        None
                    }
                })
                .unwrap_or("#")
        };

        (0..self.h)
            .map(|row| {
                (0..self.w)
                    .map(|col| {
                        let pos = row * self.w + col;
                        match known[pos] {
                            Some(num) => num.to_string(),
                            None => pos_str(pos).to_string(),
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

impl SolverUtils {
    pub fn new(solver: &Solver) -> Self {
        Self {
            old_known: vec![None; solver.sz],
            tmp_cell_idx: vec![None; solver.sz],
            tmp_cell_flag: vec![CELL_FLAG_NONE; solver.sz],
            tmp_cell_msk: vec![0; solver.sz],
            tmp_cell_count: vec![0; solver.sz],
            dfs: Vec::new(),
            new_dfs: Vec::new(),
            tmp_part: vec![],
            visit_i: 0,
            visited: vec![0; solver.sz],
            known_tmp: vec![],
            known_neighbors: vec![Vec::new(); solver.sz],
        }
    }
}
