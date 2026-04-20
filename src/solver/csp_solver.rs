
use crate::solver::base::{
        CELL_FLAG_DECIDE, CELL_FLAG_MINE, CELL_FLAG_NO_MINE, CELL_FLAG_NONE, CellFlag, CheckCell,
        Solver, SolverUtils, State, adj_diff, counters, shift_msk,
    };

const DBG: bool = false;

/// Heuristic solver to reduce brute-forcing. This makes deductions based on
/// the state and finds the best cell to expand (i.e. the one with the fewest
/// choices).
///
/// This is strongly inspired by Simon Tatham's puzzles, which has a similar CSP
/// solver but not as fast/robust brute-forcer.
pub struct CSPSolver<'a, 'b> {
    solver: &'b Solver,
    known_neighbors: &'a Vec<Vec<usize>>,
    utils: &'a mut SolverUtils,
    // Decision cells are marked as Mine/NoMine during deduction.
    state: &'a mut State,
    // The old value used for the current iteration. The cells may have been
    // updated since the sets were formed and the number of free cells may no
    // longer be accurate.
    old_max_n_mine: i32,
    // Decremented when mine_offset is incremented.
    max_n_mine: i32,
    mine_offset: u32,
    min_visit_i: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CSPSolverResult {
    Solved { mine_offset: usize },
    Found { cell: CheckCell, mine_offset: usize },
    Unsolvable,
}

impl<'a, 'b> CSPSolver<'a, 'b> {
    pub fn new(
        solver: &'b Solver,
        utils: &'a mut SolverUtils,
        state: &'a mut State,
        known_neighbors: &'a Vec<Vec<usize>>,
        max_n_mine: usize,
    ) -> Self {
        CSPSolver {
            solver,
            min_visit_i: utils.visit_i,
            utils,
            max_n_mine: max_n_mine as i32,
            old_max_n_mine: max_n_mine as i32,
            state,
            mine_offset: 0,
            known_neighbors,
        }
    }

    fn add_known_cell(&mut self, pos: usize, rec: bool) -> Option<()> {
        if self.utils.visited[pos] == self.utils.visit_i {
            return Some(());
        }
        self.utils.visited[pos] = self.utils.visit_i;

        let known = self.utils.old_known[pos].unwrap() as i32;
        let mut count = known;
        let mut msk = 0usize;

        for &(z, adj_idx) in &self.solver.adj_indices[pos] {
            debug_assert_eq!(
                self.utils.tmp_cell_idx[z].map_or(CELL_FLAG_NONE, |i| self.state[i].flag()),
                self.utils.tmp_cell_flag[z]
            );

            match self.utils.tmp_cell_flag[z] {
                CELL_FLAG_DECIDE => {
                    msk |= 1 << adj_idx;
                }
                CELL_FLAG_MINE => {
                    count -= 1;
                }
                CELL_FLAG_NO_MINE => (),
                CELL_FLAG_NONE => {
                    if self.utils.old_known[z].is_none() {
                        // This was probably split up by parts, say that we don't know anything
                        // about it.
                        self.utils.tmp_cell_msk[pos] = 0;
                        self.utils.tmp_cell_count[pos] = 0;
                        return Some(());
                    }
                    // Recurse if requested. Only push cells which have been pushed before,
                    // otherwise we risk pushing knowns which are nonadjacent to any state cells.
                    // We use a buffer since we need to update msk/count if a neighbor's flag was
                    // changed, but here we only push it back onto the dfs.
                    if rec && self.utils.visited[z] > self.min_visit_i {
                        self.utils.known_tmp.push(z);
                    }
                }
                _ => panic!("unknown cell flag"),
            }
        }

        if !(0i32..=self.max_n_mine.min(msk.count_ones() as i32)).contains(&count) {
            if DBG {
                println!("impossible:");
                dbg!(self.max_n_mine, msk, pos, count);
            }
            return None;
        } else if msk != 0 {
            if DBG {
                dbg!(pos, msk, count);
            }
            self.utils.dfs.push(pos);
        }

        self.utils.tmp_cell_msk[pos] = msk;
        self.utils.tmp_cell_count[pos] = count;

        Some(())
    }

    /// Add or re-add a cell to the queue. For each known cell around this cell:
    /// - decrements tmp_cell_count by the number of adjacent mines in the state
    /// - checks if the number of adjacent decides is valid for the result
    /// - and pushes the cell and its known neighbors onto the queue.
    ///
    /// Note we skip known cells which contain unflagged and unknown
    /// cells as neighbors. This can occur when splitting into components,
    /// e.g. with 1X1 as a minimal example, where X is unknown.
    ///
    /// If rec is true, we also add the decide-flagged neighbors of the known
    /// cells, which is used by push_cell to make sure the next iteration gets
    /// all possibly updated (i.e. changed masks/counts) known cells.
    fn add_cells(&mut self, rec: bool) -> Option<()> {
        self.utils.visit_i += 1;
        self.utils.dfs.clear();
        self.utils.known_tmp.clear();

        while let Some(cell) = self.utils.new_dfs.pop() {
            for &y in &self.known_neighbors[cell.position()] {
                self.add_known_cell(y, rec)?;
            }
        }

        for known_pos in self.utils.known_tmp.drain(..) {
            if self.utils.tmp_cell_msk[known_pos] != 0
                && self.utils.visited[known_pos] != self.utils.visit_i
            {
                self.utils.visited[known_pos] = self.utils.visit_i;
                self.utils.dfs.push(known_pos);
            }
        }

        Some(())
    }

    fn form_sets(
        &mut self,
        pos: usize,
        mut push: impl FnMut(&mut Self, CheckCell) -> Option<()>,
    ) -> Option<()> {
        // Derive the unknown (aka Decide) and known / fixed neighbors.
        let decide_msk = self.utils.tmp_cell_msk[pos];
        let fixed_msk = !decide_msk;
        let x_count = self.utils.tmp_cell_count[pos];

        push(
            self,
            CheckCell {
                pos,
                msk: decide_msk,
                count: x_count,
            },
        )?;

        // Note these pairs/triples are ordered, since they determine
        // the order of set differencing.
        for &y in &self.known_neighbors[pos] {
            if self.utils.visited[y] <= self.min_visit_i || self.utils.tmp_cell_msk[y] == 0 {
                continue;
            }

            let y_shift = adj_diff(pos, y, self.solver.w);
            // These are the unknown neighbors of y, shifted to x's position.
            let y_mask = shift_msk(self.utils.tmp_cell_msk[y], y_shift);
            let y_fixed_mask = !y_mask;
            let y_count = self.utils.tmp_cell_count[y];

            // If all cells outside of y's overlap with x are fixed
            // (i.e. not in decide_msk), then we can safely say that
            // x's counted mines are within y's.
            if (decide_msk & y_fixed_mask) == 0 {
                push(
                    self,
                    CheckCell {
                        pos,
                        msk: y_mask & fixed_msk,
                        count: y_count - x_count,
                    },
                )?;
            }

            for &z in &self.known_neighbors[pos] {
                if self.utils.visited[z] <= self.min_visit_i
                    || z == y
                    || self.utils.tmp_cell_msk[z] == 0
                {
                    continue;
                }

                let z_shift = adj_diff(pos, z, self.solver.w);
                let z_mask = shift_msk(self.utils.tmp_cell_msk[z], z_shift);
                let z_fixed_mask = !z_mask;

                // If y and z cover all of the unknown neighbors of x.
                // Note y/z don't have to be disjoint in the cells
                // decided by x, since mines will be counted by x (a la
                // PIE).
                if (decide_msk & y_fixed_mask & z_fixed_mask) == 0
                    && (fixed_msk & y_mask & z_mask) == 0
                {
                    let z_count = self.utils.tmp_cell_count[z];
                    let msk = y_mask ^ z_mask;
                    push(
                        self,
                        CheckCell {
                            pos,
                            msk: (msk & fixed_msk) | (!msk & decide_msk),
                            count: y_count + z_count - x_count,
                        },
                    )?;
                }
            }
        }

        Some(())
    }

    fn add_state_cells(&mut self) -> Option<()> {
        self.utils.new_dfs.extend(self.state.iter());
        self.add_cells(false)
    }

    /// Main deduction loop. Returns None if inconsistent.
    fn maybe_solve(&mut self) -> Option<Option<CheckCell>> {
        // It might be a poor optimization, but all the relevant per-cell data
        // is kept in scratch buffers so they don't need to be reinitialized.
        // - visit_i is used to track which cells are added in each iteration.
        // - tmp_cell_msk stores the unknown neighbors of each cell.

        if DBG {
            println!(
                "csp\n{}",
                self.solver
                    .state_str(self.state, &self.utils.old_known, &[])
            );
        }

        self.utils.new_dfs.clear();

        // Add each known cell around cells decided by this state.
        self.add_state_cells()?;

        while !self.utils.dfs.is_empty() {
            counters::CSP_ITERATIONS.add();
            self.old_max_n_mine = self.max_n_mine;
            if DBG {
                dbg!(&self.utils.dfs);
            }
            // It isn't pushed in the loop, but this makes things more
            // convenient w.r.t. borrow checking.
            while let Some(x) = self.utils.dfs.pop() {
                self.form_sets(x, |csp, cell| csp.push(cell))?;
            }
            self.add_cells(true)?;
        }

        if DBG {
            println!(
                "csp done with dfs, state:\n{}",
                self.solver
                    .state_str(self.state, &self.utils.old_known, &[])
            );
            dbg!(&self.state);
        }
        self.add_state_cells()?;

        let mut best = None;
        while let Some(x) = self.utils.dfs.pop() {
            self.form_sets(x, |csp, cell| {
                if cell.msk != 0 {
                    debug_assert!(
                        ![0, cell.msk.count_ones() as i32].contains(&cell.count),
                        "should have been found",
                    );
                    let n_ways =
                        csp.solver.ways[cell.msk.count_ones() as usize][cell.count as usize].len();
                    if best.is_none_or(|(best_ways, _)| best_ways > n_ways) {
                        best = Some((n_ways, cell));
                    }
                }
                Some(())
            })?;
        }

        Some(best.map(|x| x.1))
    }

    pub fn solve(mut self) -> CSPSolverResult {
        match self.maybe_solve() {
            Some(Some(cell)) => CSPSolverResult::Found {
                cell,
                mine_offset: self.mine_offset as usize,
            },
            Some(None) => CSPSolverResult::Solved {
                mine_offset: self.mine_offset as usize,
            },
            None => CSPSolverResult::Unsolvable,
        }
    }

    fn set_flag_and_push(&mut self, y: usize, flag: CellFlag) {
        self.utils.tmp_cell_flag[y] = flag;
        let ci = self.utils.tmp_cell_idx[y].unwrap();
        self.state[ci].set_flag(flag);
        self.utils.new_dfs.push(self.state[ci]);
    }

    fn push(&mut self, candidate: CheckCell) -> Option<()> {
        if DBG {
            dbg!(candidate);
        }
        // Number of adjacent decides. If we can't hit `count` using these free
        // cells, then the board is impossible.
        let free = candidate.msk.count_ones() as i32;
        if !(0i32..=free.min(self.old_max_n_mine)).contains(&candidate.count) {
            None
        } else if candidate.msk == 0 {
            // Nothing to update.
            Some(())
        } else if candidate.count == 0 {
            self.solver.for_in_cell(&candidate, |y| {
                // We conflicted with another inference which set this to MINE.
                if self.utils.tmp_cell_flag[y] == CELL_FLAG_MINE {
                    None
                } else if self.utils.tmp_cell_flag[y] == CELL_FLAG_DECIDE {
                    if DBG {
                        dbg!(self.state[self.utils.tmp_cell_idx[y].unwrap()], "no mine");
                    }
                    self.set_flag_and_push(y, CELL_FLAG_NO_MINE);
                    Some(())
                } else {
                    Some(())
                }
            })
        } else if candidate.count == free {
            self.solver.for_in_cell(&candidate, |y| {
                if self.utils.tmp_cell_flag[y] == CELL_FLAG_NO_MINE {
                    if DBG {
                        dbg!(
                            self.state[self.utils.tmp_cell_idx[y].unwrap()],
                            "bad, already marked no mine"
                        );
                    }
                    None
                } else if self.utils.tmp_cell_flag[y] == CELL_FLAG_DECIDE {
                    if DBG {
                        dbg!(self.state[self.utils.tmp_cell_idx[y].unwrap()], "mine");
                    }
                    self.set_flag_and_push(y, CELL_FLAG_MINE);
                    self.mine_offset += 1;
                    self.max_n_mine -= 1;
                    if self.max_n_mine < 0 { None } else { Some(()) }
                } else {
                    Some(())
                }
            })
        } else {
            Some(())
        }
    }
}
