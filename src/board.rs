//! Fairly dumb utilities for manipulating minesweeper boards. These are
//! relatively functionality-free wrappers which make things feel nicer, but
//! are actually absolutely useless.

use std::fmt::Display;

use rand::{Rng, RngExt};

use crate::utils::{self, board_from_str, is_adj, position_to_row_col};

#[derive(Debug, Clone)]
pub struct Board {
    known: Vec<Option<usize>>,
    mines: Vec<bool>,
    known_count: usize,
    n_mine: usize,
    w: usize,
}

impl Display for Board {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&utils::board_to_str(&self.known, self.w, Some(&self.mines)))
    }
}

#[derive(Copy, Clone, Debug)]
pub struct CellRef<'a> {
    board: &'a Board,
    pos: usize,
}

impl<'a> CellRef<'a> {
    pub fn known(&self) -> Option<usize> {
        self.board.known[self.pos]
    }
    pub fn mine(&self) -> bool {
        self.board.mines[self.pos]
    }
    pub fn index(&self) -> usize {
        self.pos
    }
    pub fn row(&self) -> usize {
        self.pos / self.board.w
    }
    pub fn col(&self) -> usize {
        self.pos % self.board.w
    }
    pub fn is_adjacent(&self, other: &CellRef) -> bool {
        is_adj(self.pos, other.pos, self.board.w)
    }
    pub fn neighbors(&self) -> impl Iterator<Item = CellRef<'a>> {
        utils::iter_neighbors(self.row(), self.col(), self.board.w, self.board.h()).map(
            |(ni, nj)| CellRef {
                board: self.board,
                pos: ni * self.board.w + nj,
            },
        )
    }
}

/// A `CellRef` doubles as an iterator over the board starting at its own
/// position, stepping through every following cell in row-major order. This
/// is what powers [`Board::iter`], which hands back a `CellRef` at position 0.
impl<'a> Iterator for CellRef<'a> {
    type Item = CellRef<'a>;
    fn next(&mut self) -> Option<Self::Item> {
        if self.pos >= self.board.len() {
            None
        } else {
            self.pos += 1;
            Some(CellRef {
                board: self.board,
                pos: self.pos - 1,
            })
        }
    }
}

impl std::str::FromStr for Board {
    type Err = &'static str;

    /// Converts a string to a board without regard for efficiency. This
    /// fully validates that the board is a real game state.
    fn from_str(value: &str) -> Result<Self, &'static str> {
        let result = board_from_str(value)?;
        let mut board = Self::from_mines(result.w, result.h, result.to_mines());

        for (idx, known) in result.to_known().enumerate() {
            if known.is_some() {
                board.reveal(idx / result.w, idx % result.w);
            }
        }

        if board
            .known()
            .iter()
            .zip(result.to_known())
            .all(|(&a, b)| a == b)
        {
            Ok(board)
        } else {
            Err("Invalid board state: inconsistent known mine counts")
        }
    }
}

impl Board {
    pub fn from_mines(w: usize, h: usize, mines: impl Iterator<Item = bool>) -> Self {
        let mines_vec = mines.collect::<Vec<_>>();
        let n_mine = mines_vec.iter().filter(|x| **x).count();

        Self {
            known: vec![None; w * h],
            mines: mines_vec,
            known_count: 0,
            n_mine,
            w,
        }
    }

    pub fn empty(w: usize, h: usize) -> Self {
        Self::from_mines(w, h, (0..w * h).map(|_| false))
    }

    /// Generate a board with `n_mines` randomly placed.
    pub fn random_mines(w: usize, h: usize, n_mines: usize, rng: &mut impl Rng) -> Self {
        let mut remaining_mines = n_mines;
        let board = Self::from_mines(
            w,
            h,
            (0..w * h).map(|i| {
                let is_mine = rng.random_bool(remaining_mines as f64 / (w * h - i) as f64);
                if is_mine {
                    remaining_mines -= 1;
                }
                is_mine
            }),
        );
        assert!(board.n_mine == n_mines);
        board
    }

    /// Iterates through unknown cells next to known cells.
    pub fn perimeter(&self) -> impl Iterator<Item = CellRef<'_>> {
        self.iter()
            .filter(|cell| cell.known().is_none() && cell.neighbors().any(|x| x.known().is_some()))
    }

    /// Sets whether there is a mine at (row,col), returning if the board
    /// changed. If the cell was revealed, it becomes unrevealed, and existing
    /// known counts are updated.
    pub fn set_mine(&mut self, row: usize, col: usize, is_mine: bool) -> bool {
        let i = row * self.w + col;
        let mine = &mut self.mines[i];
        if is_mine == *mine {
            return false;
        }
        if self.known[i].is_some() {
            self.known[i] = None;
            self.known_count -= 1;
        }
        let difference = if is_mine { 1 } else { -1 };
        *mine = is_mine;
        self.n_mine = self.n_mine.wrapping_add_signed(difference);
        for (ni, nj) in utils::iter_neighbors(row, col, self.w, self.h()) {
            if let Some(k) = &mut self.known[ni * self.w + nj] {
                *k = k.wrapping_add_signed(difference);
            }
        }
        true
    }

    /// Reveal the cell at (row, col). If it isn't a mine, returns the number of
    /// safe cells which were revealed.
    pub fn reveal(&mut self, row: usize, col: usize) -> Option<usize> {
        let orig_i = row * self.w + col;
        if self.mines[orig_i] {
            return None;
        } else if self.known[orig_i].is_some() {
            return Some(0);
        }

        let mut dfs = vec![row * self.w + col];
        let mut revealed = 0;
        while let Some(pos) = dfs.pop() {
            let mut n_mine = 0;
            let old_dfs_len = dfs.len();
            for neighbor in self.at_index(pos).neighbors() {
                if neighbor.mine() {
                    n_mine += 1;
                } else if neighbor.known().is_none() {
                    dfs.push(neighbor.pos);
                }
            }

            self.known[pos] = Some(n_mine);
            revealed += 1;

            if n_mine == 0 {
                // Throw something there for now to mark as visited.
                for &other_pos in &dfs[old_dfs_len..] {
                    self.known[other_pos] = Some(0);
                }
            } else {
                dfs.truncate(old_dfs_len);
            }
        }

        self.known_count += revealed;
        Some(revealed)
    }

    /// Reveal a random non-mine cell, returning None if the board is solved.
    pub fn reveal_random_non_mine(&mut self, rng: &mut impl Rng) -> Option<usize> {
        let to_reveal = self.len() - self.n_mine - self.known_count;
        if to_reveal == 0 {
            None
        } else {
            let idx = rng.random_range(0..to_reveal);
            let (idx, _) = self
                .mines
                .iter()
                .zip(&self.known)
                .enumerate()
                .filter(|(_, (is_mine, known))| !*is_mine && known.is_none())
                .nth(idx)
                .expect("cell to reveal should exist");
            let (row, col) = position_to_row_col(idx, self.w);
            self.reveal(row, col)
        }
    }

    pub fn reveal_all(&mut self) -> usize {
        let mut num_revealed = 0;
        for i in 0..self.len() {
            if self.known[i].is_none() && !self.mines[i] {
                let mines = self.at_index(i).neighbors().filter(|x| x.mine()).count();
                self.known[i] = Some(mines);
                num_revealed += 1;
            }
        }
        self.known_count += num_revealed;
        num_revealed
    }

    pub fn hide_all(&mut self) {
        self.known.fill(None);
        self.known_count = 0;
    }

    pub fn iter(&self) -> CellRef<'_> {
        CellRef {
            board: self,
            pos: 0,
        }
    }

    pub fn at_index(&self, pos: usize) -> CellRef<'_> {
        CellRef { board: self, pos }
    }
    pub fn at(&self, row: usize, col: usize) -> CellRef<'_> {
        if col >= self.w {
            panic!("Column {} out of range", col);
        }
        let pos = row * self.w + col;
        if pos >= self.len() {
            panic!("Row {} out of range", row);
        }
        self.at_index(pos)
    }

    pub fn solved(&self) -> bool {
        self.known_count == self.len() - self.n_mine
    }
    pub fn known_count(&self) -> usize {
        self.known_count
    }
    pub fn known(&self) -> &[Option<usize>] {
        &self.known
    }
    pub fn mines(&self) -> &[bool] {
        &self.mines
    }
    pub fn n_mine(&self) -> usize {
        self.n_mine
    }
    pub fn w(&self) -> usize {
        self.w
    }
    pub fn h(&self) -> usize {
        self.known.len() / self.w
    }
    pub fn len(&self) -> usize {
        self.known.len()
    }
}
