use std::{
    collections::{HashMap, hash_map::Entry},
    hash::{BuildHasherDefault, Hasher},
    ops::Range,
};

use crate::{
    solver::{
        base::{CELL_FLAG_NO_MINE, CELL_FLAG_NONE, CheckCell, counters},
        csp_solver::{CSPSolver, CSPSolverResult},
    },
    utils::position_to_row_col,
};

use crate::solver::base::{
    CELL_FLAG_DECIDE, CELL_FLAG_MINE, Cell, Failure, IMPOSSIBLE, Solver, SolverUtils, State,
    StateProbs, adj_index,
};

fn convolve_probs(a: &StateProbs, b: &StateProbs, max_len: usize) -> StateProbs {
    let out_len = (a.len() + b.len()).saturating_sub(1).min(max_len);
    let mut out = Vec::new();

    // Due to max_len, the element at out_len-1 may not be nonzero, even though
    // the probs should never have trailing zeros, so we manually check & resize.
    for i in 0..out_len {
        let j_start = (i + 1).saturating_sub(a.len());
        let j_end = i.min(b.len().saturating_sub(1));
        let mut elem = IMPOSSIBLE;
        for j in j_start..=j_end {
            elem += b[j] * a[i - j];
        }
        if elem != IMPOSSIBLE {
            out.resize(i + 1, 0.0);
            out[i] = elem;
        }
    }

    out
}

fn compare_probs_with_empty(
    num: impl Fn(usize) -> f32,
    den: impl Fn(usize) -> f32,
    num_range: Range<usize>,
    den_range: Range<usize>,
    n_mine: usize,
    n_outside: usize,
) -> f32 {
    let mut total = 0.0;
    let min_state_mines = n_mine.saturating_sub(n_outside);
    for n_state_mines in min_state_mines.max(num_range.start)..num_range.end {
        let mut total_den: f32 = 0.0;
        let mut coeff = 1.0;

        for n_original_mines in n_state_mines..den_range.end {
            total_den += den(n_original_mines) * coeff;
            let n_original_outside = n_mine - n_original_mines;
            coeff *= n_original_outside as f32 / (n_outside - n_original_outside + 1) as f32;
        }

        coeff = 1.0;
        for n_original_mines in (min_state_mines.max(den_range.start)..n_state_mines).rev() {
            let n_original_outside = n_mine - n_original_mines;
            coeff *= (n_outside + 1 - n_original_outside) as f32 / n_original_outside as f32;
            total_den += den(n_original_mines) * coeff;
        }

        total += num(n_state_mines) / total_den;
    }
    total
}

struct CheckStateChoose {
    max_n_mine: usize,
    cache_index: CacheIndex,
    state: State,
    /// Which cells are in the set counted by `num_mines.` This indexes into
    /// the state.
    positions: Vec<usize>,
    idx: usize,
    mine_offset: usize,
    num_mines: usize,
}

/// The "stack frame" for our fake-recursive solver.
enum CheckState {
    /// Each state begins here, where we decide if it needs to be split up
    /// (Parts) or conditioned upon (Choose).
    Init { max_n_mine: usize, state: State },
    /// Split up into disconnected (by known cells) components. In this node,
    /// the probabilities for each component are accumulated in the cache.
    Parts {
        max_n_mine: usize,
        cache_index: CacheIndex,
        parts: Vec<State>,
    },
    /// Iterate over whether there is a mine at the list of positions, when
    /// there are `num_mines` total.
    Choose(CheckStateChoose),
}

#[derive(PartialEq, Eq, Clone)]
struct CacheKey(u64, State);

struct CacheResult {
    probs: StateProbs,
    invalidation_index: usize,
}

#[derive(Clone, Copy, Debug)]
struct CacheIndex(usize);

type CacheMap = HashMap<CacheKey, CacheIndex, BuildHasherDefault<NoHasher>>;

fn state_hash(base: &[u64], s: &State) -> u64 {
    let mut out: u64 = 0;
    for c in s {
        out = out.wrapping_add(base[c.position()].wrapping_mul(c.flag().0 as u64));
    }
    out
}

impl std::hash::Hash for CacheKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        state.write_u64(self.0);
    }
}

#[derive(Default)]
struct NoHasher(u64);

impl Hasher for NoHasher {
    fn write(&mut self, _: &[u8]) {
        unreachable!();
    }
    fn write_u64(&mut self, i: u64) {
        self.0 = i;
    }
    fn finish(&self) -> u64 {
        self.0
    }
}

#[derive(Default)]
struct Stats {
    /// Cells without a known adjacency.
    n_empty: usize,
    /// Empty cells not on the perimeter.
    n_outside: usize,
    /// Any empty cell outside the perimeter, which might be revealed if all the
    /// mines are inferred to be on the perimeter.
    outside_perimeter: Option<usize>,
}

/// Cache and mutable scratchpad for the recursive solver / checker.
pub struct CheckerData {
    cache: CacheMap,
    cache_values: Vec<CacheResult>,

    /// Used to invalidate the cache when new numbers are known (e.g. in the
    /// process of solving, we reveal cells). For an updated cell, the
    /// invalidation index is maxed for it and all its neighbors.
    invalidation_index: Vec<usize>,
    max_invalidation_index: usize,

    // If there is a fixed mine count, it's kept here. Updating this clears the cache, since the
    // cached states implicitly encode this constraint (they are not differentiated by max_n_mine).
    n_mines: Option<usize>,

    cstates: Vec<CheckState>,
    child_probs: Option<CacheIndex>,

    stats: Stats,
    initial_state: State,
}

impl CheckerData {
    pub fn new(solver: &Solver) -> Self {
        Self {
            n_mines: None,
            invalidation_index: vec![0; solver.sz],
            max_invalidation_index: 0,
            cstates: Vec::new(),
            child_probs: None,
            cache_values: vec![],
            cache: CacheMap::with_hasher(BuildHasherDefault::<NoHasher>::default()),
            stats: Stats::default(),
            initial_state: vec![],
        }
    }
}

/// Ugly monolithic struct for the current checker state, holding the immutable
/// common data from `Solver` and the mutable scratchpads in `data` / `utils`.
pub struct CheckerState<'a> {
    pub solver: &'a Solver,
    pub data: &'a mut CheckerData,
    pub utils: &'a mut SolverUtils,
    known_neighbors: Vec<Vec<usize>>,
}

impl<'a> CheckerState<'a> {
    pub fn new(solver: &'a Solver, data: &'a mut CheckerData, utils: &'a mut SolverUtils) -> Self {
        Self {
            known_neighbors: std::mem::take(&mut utils.known_neighbors),
            solver,
            data,
            utils,
        }
    }
}

const DBG: bool = false;

fn print_indented(indent: usize, data: String) {
    let pre = "|  ".repeat(indent);
    let mut maxlen = 0;
    for (i, line) in data.lines().enumerate() {
        let suff = ["| ", (if i == 0 { "" } else { "-  " }), line].concat();
        println!("{pre}{suff}");
        maxlen = maxlen.max(suff.len());
    }
    println!("{pre}{}", "-".repeat(maxlen));
}

fn table_strs(strs: &[&str]) -> String {
    let lines = strs
        .iter()
        .map(|slice| slice.lines().collect::<Vec<_>>())
        .collect::<Vec<_>>();
    let max_line_len = lines
        .iter()
        .map(|part| part.iter().map(|line| line.len()).max().unwrap_or(0))
        .collect::<Vec<_>>();
    let max_lines = lines.iter().map(|x| x.len()).max().unwrap_or(0);
    (0..max_lines)
        .map(|line| {
            lines
                .iter()
                .zip(&max_line_len)
                .map(|(part, max_len)| -> String {
                    let line_str = part.get(line).map_or("", |v| v);
                    line_str.to_string() + &" ".repeat(max_len - line_str.len())
                })
                .collect::<Vec<_>>()
                .join(" | ")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// If the state reports impossible.
fn is_impossible(probs: &[f32]) -> bool {
    probs.iter().all(|&x| x == IMPOSSIBLE)
}

impl<'a> Drop for CheckerState<'a> {
    fn drop(&mut self) {
        self.utils.known_neighbors = std::mem::take(&mut self.known_neighbors);
    }
}

impl<'a> CheckerState<'a> {
    /// Recompute stats and invalidate cache based on new known state. This is
    /// called during each solve.
    pub fn update(
        &mut self,
        new_known: &[impl Into<Option<usize>> + Clone],
        n_mines: Option<usize>,
    ) {
        if n_mines != self.data.n_mines {
            self.data.cache.clear();
            self.data.cache_values.clear();
            self.data.n_mines = n_mines;
        }

        self.data.max_invalidation_index += 1;

        for i in 0..self.solver.sz {
            let value = new_known[i].clone().into();
            if value != self.utils.old_known[i] {
                self.data.invalidation_index[i] = self.data.max_invalidation_index;
                for &y in &self.solver.neighbors[i] {
                    self.data.invalidation_index[y] = self.data.max_invalidation_index;
                }
                self.utils.old_known[i] = value;
            }
        }

        let mut state = State::new();
        self.data.stats = Stats::default();
        self.utils.visit_i += 1;

        // Update known_neighbors and stats.
        for i in 0..self.solver.sz {
            self.known_neighbors[i].clear();
            for &j in &self.solver.neighbors[i] {
                if self.utils.old_known[j].is_some() {
                    self.known_neighbors[i].push(j);
                }
            }

            if self.utils.old_known[i].is_none() {
                self.data.stats.n_empty += 1;
            }
        }

        // Find perimeter and add to new initial state.
        for i in 0..self.solver.sz {
            if self.utils.visited[i] == self.utils.visit_i || self.utils.old_known[i].is_none() {
                continue;
            }

            self.utils.dfs.clear();
            self.utils.dfs.push(i);
            self.utils.visited[i] = self.utils.visit_i;

            while let Some(x) = self.utils.dfs.pop() {
                for &y in &self.solver.neighbors[x] {
                    if self.utils.visited[y] != self.utils.visit_i {
                        if self.utils.old_known[y].is_some() {
                            self.utils.dfs.push(y);
                        } else {
                            state.push(Cell::new(CELL_FLAG_DECIDE, y));
                        }
                        self.utils.visited[y] = self.utils.visit_i;
                    }
                }
            }
        }

        for i in 0..self.solver.sz {
            if self.utils.old_known[i].is_none() && self.utils.visited[i] != self.utils.visit_i {
                self.data.stats.outside_perimeter = Some(i);
                self.data.stats.n_outside += 1;
            }
        }

        state.sort_by_key(|cell| cell.position());
        self.data.initial_state = state;
    }

    fn in_check_cell(&self, cell: &CheckCell, x: usize) -> bool {
        adj_index(cell.pos, x, self.solver.w).is_some_and(|adj| (1 << adj) & cell.msk != 0)
    }

    fn assert_probs(&self, state_probs: &StateProbs) {
        debug_assert!(
            state_probs.len() <= self.data.n_mines.unwrap_or(self.solver.sz) + 1
                && state_probs.last().is_none_or(|&x| x != IMPOSSIBLE),
            "invalid probs {:?} (max_n_mine = {:?})",
            state_probs,
            self.data.n_mines
        );
    }

    fn get_child_probs(&self) -> &StateProbs {
        let cache_idx = self
            .data
            .child_probs
            .expect("child probs should have been set by iteration");
        let cache_res = &self.data.cache_values[cache_idx.0];
        self.assert_probs(&cache_res.probs);
        &cache_res.probs
    }

    fn check(&mut self, cstate: CheckState) {
        self.data.child_probs = match cstate {
            CheckState::Init { max_n_mine, state } => self.check_init(state, max_n_mine),
            CheckState::Choose(state) => self.check_choose(state),
            CheckState::Parts {
                max_n_mine,
                cache_index,
                parts,
            } => self.check_parts(cache_index, parts, max_n_mine),
        }
    }

    fn solve_loop(&mut self, initial_state: State) {
        // Always use n_mine, even during mine_probability (which should be -1), so it's
        // consistent across cache hits.
        self.data.cstates.push(CheckState::Init {
            state: initial_state,
            max_n_mine: self.data.n_mines.unwrap_or(self.solver.sz),
        });

        while let Some(cstate) = self.data.cstates.pop() {
            if DBG {
                let indent = self.data.cstates.len();
                let (state_info, states, cache_index) = match &cstate {
                    CheckState::Init { state, max_n_mine } => (
                        format!("init (size {}, max_n_mine {})", state.len(), max_n_mine),
                        vec![state],
                        None,
                    ),
                    CheckState::Choose(CheckStateChoose {
                        cache_index,
                        state,
                        idx,
                        mine_offset,
                        num_mines,
                        positions,
                        max_n_mine,
                    }) => (
                        format!(
                            "choose (size {}, idx {}, mine offset {}, {} mines, {} positions, {max_n_mine} max mines)",
                            state.len(),
                            idx,
                            mine_offset,
                            num_mines,
                            positions.len()
                        ),
                        vec![state],
                        Some(cache_index),
                    ),
                    CheckState::Parts {
                        cache_index,
                        parts,
                        max_n_mine,
                    } => (
                        format!("parts ({} parts, {max_n_mine} max mines)", parts.len()),
                        parts.iter().collect::<Vec<_>>(),
                        Some(cache_index),
                    ),
                };

                let hash = states
                    .iter()
                    .map(|state| format!("{:08x}", state_hash(&self.solver.base, state)))
                    .collect::<Vec<_>>()
                    .join(", ");
                let pos: &[usize] =
                    if let CheckState::Choose(CheckStateChoose { positions, .. }) = &cstate {
                        positions
                    } else {
                        &[]
                    };
                let states_str = states
                    .iter()
                    .map(|state| self.solver.state_str(state, &self.utils.old_known, pos))
                    .collect::<Vec<_>>();
                let states_str =
                    table_strs(&states_str.iter().map(|x| x.as_str()).collect::<Vec<_>>());
                let cache_str = cache_index
                    .map(|idx| idx.0.to_string())
                    .unwrap_or_else(|| "(none)".to_string());

                self.check(cstate);

                let probs = self
                    .data
                    .child_probs
                    .map(|i| {
                        let probs = &self.data.cache_values[i.0].probs;
                        if probs.is_empty() {
                            "impossible".to_string()
                        } else {
                            probs
                                .iter()
                                .enumerate()
                                .map(|(n_mine, prob)| format!("{}: {}", n_mine, prob))
                                .collect::<Vec<_>>()
                                .join(", ")
                        }
                    })
                    .unwrap_or_else(|| "none".to_string());

                print_indented(
                    indent,
                    format!(
                        "Checking {}\n{}\nhash: {}, cache index: {}\n-> {}",
                        state_info, states_str, hash, cache_str, probs
                    ),
                );
            } else {
                self.check(cstate);
            }
        }
    }

    pub fn find_safe_cell(
        &mut self,
        mut filter_fn: impl FnMut(usize) -> bool,
    ) -> Result<(usize, usize), Failure> {
        if self.data.stats.n_empty == self.solver.sz {
            Err(Failure::Empty)
        } else if let Some(n_mine) = self.data.n_mines
            && n_mine > self.data.stats.n_empty
        {
            Err(Failure::Unsolvable)
        } else if self.data.n_mines == Some(self.data.stats.n_empty) || self.data.stats.n_empty == 0
        {
            Err(Failure::Solved)
        } else {
            debug_assert!(
                !self.data.initial_state.is_empty(),
                "initial state should not be empty if stats n empty initialized to positive value"
            );

            let state = self.data.initial_state.clone();

            // Determine if it's inconsistent first.
            self.solve_loop(state.clone());
            let state_probs = self.get_child_probs();
            if state_probs.is_empty() {
                return Err(Failure::Unsolvable);
            }

            // If it's impossible without all the mines on the perimeter, then
            // any cell outside the perimeter is safe.
            if let Some(outside) = self.data.stats.outside_perimeter
                && is_impossible(
                    &state_probs[0..self
                        .data
                        .n_mines
                        .unwrap_or(usize::MAX)
                        .min(state_probs.len())],
                )
            {
                return Ok(position_to_row_col(outside, self.solver.w));
            }

            let min_state_mines = self
                .data
                .n_mines
                .map(|n_mines| n_mines.saturating_sub(self.data.stats.n_outside + 1))
                .unwrap_or(0);

            // Then try flagging each perimeter cell as a mine.
            for (idx, &cell) in state
                .iter()
                .enumerate()
                .filter(|&(_, &cell)| filter_fn(cell.position()))
            {
                let mut new_state = state.clone();
                new_state[idx].set_flag(CELL_FLAG_MINE);
                self.solve_loop(new_state);

                let probs = self.get_child_probs();
                if min_state_mines >= probs.len()
                    || is_impossible(
                        &probs[min_state_mines
                            ..self.data.n_mines.unwrap_or(usize::MAX).min(probs.len())],
                    )
                {
                    let ret = position_to_row_col(cell.position(), self.solver.w);
                    return Ok(ret);
                }
            }

            Err(Failure::MustGuess)
        }
    }

    fn clear_state_idx(&mut self, state: &State) {
        for cell in state {
            self.utils.tmp_cell_idx[cell.position()] = None;
            self.utils.tmp_cell_flag[cell.position()] = CELL_FLAG_NONE;
        }
    }

    fn split_into_parts(&mut self, state: &State) -> Option<Vec<State>> {
        self.utils.dfs.clear();
        self.utils.tmp_part.clear();
        let first_visit_i = self.utils.visit_i;
        let mut maybe_parts: Option<Vec<State>> = None;

        for &cell in state {
            if cell.flag() != CELL_FLAG_DECIDE
                || self.utils.visited[cell.position()] > first_visit_i
            {
                continue;
            }

            // Only decides or knowns are pushed here. We push knowns adjacent to decides
            // since, for the part to be self-contained, we need potential mines (decides)
            // next to all knowns in our state.
            self.utils.dfs.push(cell.position());
            self.utils.visit_i += 1;
            self.utils.visited[cell.position()] = self.utils.visit_i;

            if !self.utils.tmp_part.is_empty() {
                maybe_parts = Some(vec![std::mem::take(&mut self.utils.tmp_part)]);
            }

            self.utils.tmp_part.push(cell);
            while let Some(x) = self.utils.dfs.pop() {
                for &y in &self.known_neighbors[x] {
                    if self.utils.visited[y] != self.utils.visit_i {
                        self.utils.visited[y] = self.utils.visit_i;
                        for &z in &self.solver.neighbors[y] {
                            let flag = self.utils.tmp_cell_flag[z];
                            if flag != CELL_FLAG_NONE && self.utils.visited[z] != self.utils.visit_i
                            {
                                if flag == CELL_FLAG_DECIDE {
                                    self.utils.dfs.push(z);
                                }
                                self.utils.tmp_part.push(Cell::new(flag, z));
                                self.utils.visited[z] = self.utils.visit_i;
                            }
                        }
                    }
                }
            }

            if let Some(parts) = &mut maybe_parts {
                parts.push(std::mem::take(&mut self.utils.tmp_part));
            }
        }

        if let Some(parts) = &mut maybe_parts {
            for part in parts {
                part.sort_by_key(|cell| cell.position());
            }
        }

        maybe_parts
    }

    fn check_init(&mut self, state: State, max_n_mine: usize) -> Option<CacheIndex> {
        let hash = state_hash(&self.solver.base, &state);
        let maybe_entry = self.data.cache.entry(CacheKey(hash, state));

        let (cache_index, do_push, entry) = match maybe_entry {
            Entry::Occupied(occupied) => {
                let &CacheIndex(cache_index) = occupied.get();
                // We should always make progress.
                let cache_result = &mut self.data.cache_values[cache_index];
                // Check if this cache hit is invalid. This works since the state
                // plus the adjacent knows contain all we need to decide if the mine
                // placement is valid for the original board.
                let valid = self.data.max_invalidation_index == cache_result.invalidation_index
                    || occupied.key().1.iter().all(|c| {
                        cache_result.invalidation_index
                            >= self.data.invalidation_index[c.position()]
                    });
                if valid {
                    counters::CACHE_HIT.add();
                    cache_result.invalidation_index = self.data.max_invalidation_index;
                    return Some(CacheIndex(cache_index));
                }
                (CacheIndex(cache_index), false, occupied)
            }
            Entry::Vacant(vacant) => {
                let idx = CacheIndex(self.data.cache_values.len());
                let entry = vacant.insert_entry(idx);
                (idx, true, entry)
            }
        };

        counters::STATE_COUNT.add();

        let mut new_state = entry.key().1.clone();
        for (idx, cell) in new_state.iter().enumerate() {
            self.utils.tmp_cell_idx[cell.position()] = Some(idx);
            self.utils.tmp_cell_flag[cell.position()] = cell.flag();
        }

        let csp_solver = CSPSolver::new(
            self.solver,
            self.utils,
            &mut new_state,
            &self.known_neighbors,
            max_n_mine,
        );
        let solution = csp_solver.solve();

        if DBG {
            let best = match solution {
                CSPSolverResult::Found { cell, mine_offset } => Some((Some(cell), mine_offset)),
                CSPSolverResult::Solved { mine_offset } => Some((None, mine_offset)),
                _ => None,
            };
            print_indented(
                self.data.cstates.len(),
                format!(
                    "CSP solver result ({}):\n{}",
                    best.map(|x| format!("mine offset {}, best {:?}", x.1, x.0))
                        .unwrap_or_else(|| "unsolvable".to_string()),
                    self.solver
                        .state_str(&new_state, &self.utils.old_known, &[])
                ),
            );
        }

        debug_assert!(if let CSPSolverResult::Found { mine_offset, .. }
        | CSPSolverResult::Solved { mine_offset } = &solution
        {
            *mine_offset <= self.data.n_mines.unwrap_or(self.solver.sz)
        } else {
            true
        });

        let (probs, finalized) = match solution {
            CSPSolverResult::Found {
                cell: best,
                mine_offset,
            } => {
                debug_assert!(
                    (0..=best.msk.count_ones().min((max_n_mine - mine_offset) as u32))
                        .contains(&(best.count as u32))
                );

                // This needs tmp_cell_idx, so clear afterwards.
                // let maybe_parts = self.split_into_parts(&new_state);
                let maybe_parts = self.split_into_parts(&new_state);

                // State was modified by CSP solver, but set of cells did not change.
                self.clear_state_idx(&new_state);

                if let Some(mut parts) = maybe_parts {
                    // We init the return probabilities as being fully likely to contain
                    // the minimum number of mines. As we get data from each component,
                    // we convolve the results.
                    let mut probs = vec![0.0; mine_offset + 1];
                    probs[mine_offset] = 1.0;

                    let part = parts.pop().expect("checked len > 1");
                    self.data.cstates.push(CheckState::Parts {
                        cache_index,
                        parts,
                        max_n_mine,
                    });
                    self.data.cstates.push(CheckState::Init {
                        state: part,
                        max_n_mine,
                    });

                    (probs, false)
                } else {
                    let positions = new_state
                        .iter()
                        .enumerate()
                        .filter_map(|(idx, cell)| {
                            if self.in_check_cell(&best, cell.position()) {
                                Some(idx)
                            } else {
                                None
                            }
                        })
                        .collect::<Vec<_>>();
                    debug_assert!(positions.len() >= best.count as usize);

                    self.data.cstates.push(CheckState::Choose(CheckStateChoose {
                        idx: 0,
                        cache_index,
                        state: new_state,
                        positions,
                        mine_offset,
                        num_mines: best.count as usize,
                        max_n_mine,
                    }));

                    (vec![], false)
                }
            }
            CSPSolverResult::Solved { mine_offset } => {
                self.clear_state_idx(&new_state);
                // If the CSP solver succeeded but ended up resolving all the cells
                // (hence no cell to condition on), it decided that there must be
                // exactly `mine_offset` mines.
                let mut probs = vec![IMPOSSIBLE; mine_offset + 1];
                probs[mine_offset] = 1.0;
                (probs, true)
            }
            CSPSolverResult::Unsolvable => {
                self.clear_state_idx(&new_state);
                (vec![], true)
            }
        };

        let cache_result = CacheResult {
            probs,
            invalidation_index: self.data.max_invalidation_index,
        };

        if do_push {
            self.data.cache_values.push(cache_result);
        } else {
            self.data.cache_values[cache_index.0] = cache_result;
        }

        if finalized { Some(cache_index) } else { None }
    }

    fn check_parts(
        &mut self,
        cache_index: CacheIndex,
        mut parts: Vec<State>,
        max_n_mine: usize,
    ) -> Option<CacheIndex> {
        let child_probs = self.get_child_probs();
        let new_probs = convolve_probs(
            child_probs,
            &self.data.cache_values[cache_index.0].probs,
            max_n_mine + 1,
        );

        // We're done with these components if combining them is impossible.
        if new_probs.is_empty() {
            parts.clear();
        }

        self.data.cache_values[cache_index.0].probs = new_probs;
        if let Some(state) = parts.pop() {
            self.data.cstates.push(CheckState::Parts {
                cache_index,
                parts,
                max_n_mine,
            });
            self.data
                .cstates
                .push(CheckState::Init { state, max_n_mine });
            None
        } else {
            Some(cache_index)
        }
    }

    fn check_choose(&mut self, choose: CheckStateChoose) -> Option<CacheIndex> {
        let ways = &self.solver.ways[choose.positions.len()][choose.num_mines];
        let cache_idx = choose.cache_index.0;

        if choose.idx > 0 {
            // (This is only buggy if cache_idx is the child cache index, but that should
            // never happen!)
            debug_assert_ne!(cache_idx, self.data.child_probs.unwrap().0);
            let mut probs = std::mem::take(&mut self.data.cache_values[cache_idx].probs);
            let child_probs = self.get_child_probs();
            if !child_probs.is_empty() {
                let child_mine_offset =
                    choose.mine_offset + ways[choose.idx - 1].count_ones() as usize;
                let probs_view = &child_probs[..child_probs
                    .len()
                    .min((choose.max_n_mine + 1).saturating_sub(child_mine_offset))];
                if let Some(child_prob_len) = probs_view
                    .iter()
                    .enumerate()
                    .rev()
                    .find_map(|(i, &x)| if x != 0.0 { Some(i + 1) } else { None })
                {
                    let len = child_mine_offset + child_prob_len;
                    if probs.len() < len {
                        probs.resize(len, IMPOSSIBLE);
                    }

                    for n_mine in 0..child_prob_len {
                        probs[child_mine_offset + n_mine] += probs_view[n_mine];
                    }
                }
            }
            self.data.cache_values[cache_idx].probs = probs;
        }

        if choose.idx < ways.len() {
            let msk = ways[choose.idx];
            let mut new_state = choose.state.clone();
            for (j, &state_idx) in choose.positions.iter().enumerate() {
                if (msk & (1 << j)) != 0 {
                    new_state[state_idx].set_flag(CELL_FLAG_MINE);
                } else {
                    new_state[state_idx].set_flag(CELL_FLAG_NO_MINE);
                }
            }
            let child_mine_offset = choose.mine_offset + ways[choose.idx].count_ones() as usize;

            self.data.cstates.push(CheckState::Choose(CheckStateChoose {
                idx: choose.idx + 1,
                ..choose
            }));
            self.data.cstates.push(CheckState::Init {
                state: new_state,
                max_n_mine: choose.max_n_mine - child_mine_offset,
            });

            None
        } else {
            Some(choose.cache_index)
        }
    }

    pub fn mine_probability(&mut self, pos: usize) -> Result<Option<f32>, Failure> {
        if self.data.n_mines == Some(0) || self.utils.old_known[pos].is_some() {
            return Ok(Some(IMPOSSIBLE));
        }

        let original_probs_cache_idx = {
            let state = self.data.initial_state.clone();
            self.solve_loop(state);
            if self.get_child_probs().is_empty() {
                return Err(Failure::Unsolvable);
            }
            self.data.child_probs.expect("solve populates child probs")
        };

        let n_outside = self.data.stats.n_outside;

        if let Some(total_mines) = self.data.n_mines {
            let res = if let Some((cell_idx, _)) = self
                .data
                .initial_state
                .iter()
                .enumerate()
                .find(|(_, cell)| cell.position() == pos)
            {
                let mut state = self.data.initial_state.clone();
                state[cell_idx].set_flag(CELL_FLAG_MINE);
                self.solve_loop(state);

                let probs = self.get_child_probs();
                let max_state_mines = total_mines.min(probs.len());
                let original_probs = &self.data.cache_values[original_probs_cache_idx.0].probs;

                compare_probs_with_empty(
                    |n_state_mines| {
                        // -1 since we fixed another mine.
                        probs[n_state_mines - 1]
                    },
                    |n_original_mines| original_probs[n_original_mines],
                    1..max_state_mines + 1,
                    0..original_probs.len(),
                    total_mines,
                    n_outside,
                )
            } else {
                assert!(n_outside > 0);

                let probs = self.get_child_probs();
                compare_probs_with_empty(
                    |n_mines| probs[n_mines] * (total_mines - n_mines) as f32 / n_outside as f32,
                    |n_mines| probs[n_mines],
                    0..probs.len(),
                    0..probs.len(),
                    total_mines,
                    n_outside,
                )
            };
            Ok(Some(res))
        } else {
            Ok(None)
        }
    }

    /// Stripped down version of mine_probability just to know if it's possible
    /// for a cell to be a mine. Called often by the board generator.
    pub fn can_be_mine(&mut self, pos: usize) -> bool {
        if self.data.n_mines == Some(0) || self.utils.old_known[pos].is_some() {
            return false;
        }

        let offset = if let Some((cell_idx, _)) = self
            .data
            .initial_state
            .iter()
            .enumerate()
            .find(|(_, cell)| cell.position() == pos)
        {
            let mut state = self.data.initial_state.clone();
            state[cell_idx].set_flag(CELL_FLAG_MINE);
            self.solve_loop(state);
            1
        } else {
            self.solve_loop(self.data.initial_state.clone());
            0
        };

        let probs = self.get_child_probs();
        let min_state_mines = self
            .data
            .n_mines
            .map_or(0, |x| x.saturating_sub(self.data.stats.n_outside + offset));
        let max_state_mines = self
            .data
            .n_mines
            .map_or(probs.len(), |x| x.min(probs.len()));

        min_state_mines < max_state_mines
            && probs[min_state_mines..max_state_mines]
                .iter()
                .any(|&v| v != IMPOSSIBLE)
    }
}

#[cfg(test)]
mod tests {
    use rand::{RngExt, SeedableRng, rngs::SmallRng};

    #[test]
    fn test_compare_probs_with_empty() {
        let rng = SmallRng::seed_from_u64(123);
        let mut f = rng.random_iter().take(100).collect::<Vec<_>>();
        let g = f.split_off(50);

        let n_mine = 70;
        let n_empty = n_mine - 3;
        let res =
            super::compare_probs_with_empty(|i| f[i], |i| g[i], 3..20, 5..30, n_mine, n_empty);

        let binom = |n, k| -> f64 {
            (n - k + 1..=n).map(|x| x as f64).product::<f64>()
                / (1..=k).map(|x| x as f64).product::<f64>()
        };

        let f_tot: f64 = (3..20)
            .map(|i| f[i] as f64 * binom(n_empty, n_mine - i))
            .sum();
        let g_tot: f64 = (5..30)
            .map(|i| g[i] as f64 * binom(n_empty, n_mine - i))
            .sum();
        assert!(((f_tot / g_tot) - res as f64).abs() < 1e-6);
    }
}
