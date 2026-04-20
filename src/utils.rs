/// Render a board to a whitespace-separated string: numbers for known cells,
/// `#` for unknown, and `X` for a mine when `mines` is provided. Panics if
/// `board.len()` is not a multiple of `width`.
pub fn board_to_str(
    board: &[impl Into<Option<usize>> + Clone],
    width: usize,
    mines: Option<&[bool]>,
) -> String {
    assert!(
        board.len().is_multiple_of(width),
        "Board length not divisible by width"
    );

    board
        .chunks_exact(width)
        .enumerate()
        .map(|(i, row)| {
            row.iter()
                .enumerate()
                .map(|(j, cell)| match cell.clone().into() {
                    None if mines.is_some_and(|mines_arr| mines_arr[i * width + j]) => {
                        "X".to_string()
                    }
                    None => "#".to_string(),
                    Some(num) => num.to_string(),
                })
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// A single cell parsed from the string board format.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BoardFromStrCell {
    /// A number token (`0`..=`8`): the cell is revealed and has this many
    /// adjacent mines.
    Known(usize),
    /// The `X` token: the cell is a mine.
    Mine,
    /// The `#` token: the cell is not yet revealed.
    Unknown,
}

/// Result of parsing a string board.
#[derive(Clone, Debug)]
pub struct BoardFromStrResult {
    /// Row-major cells, length `w * h`.
    pub cells: Vec<BoardFromStrCell>,
    /// Board width.
    pub w: usize,
    /// Board height.
    pub h: usize,
}

impl BoardFromStrResult {
    /// Iterate the known-count of each cell: `Some(n)` for `Known(n)`, `None`
    /// for `Mine` or `Unknown`.
    pub fn to_known(&self) -> impl Iterator<Item = Option<usize>> {
        self.cells.iter().map(|&cell| match cell {
            BoardFromStrCell::Known(x) => Some(x),
            _ => None,
        })
    }

    /// Iterate which cells are mines: `true` for `Mine`, `false` otherwise.
    pub fn to_mines(&self) -> impl Iterator<Item = bool> {
        self.cells
            .iter()
            .map(|&cell| cell == BoardFromStrCell::Mine)
    }
}

/// Parse a whitespace-separated board. Each token is a number (revealed
/// cell), `X` (mine), or `#` (unknown). Rows are separated by newlines and
/// must all be the same length.
pub fn board_from_str(data: &str) -> Result<BoardFromStrResult, &'static str> {
    let line_vecs = data
        .lines()
        .map(|line| {
            line.split_ascii_whitespace()
                .map(|x| -> Result<BoardFromStrCell, &'static str> {
                    if x == "X" {
                        Ok(BoardFromStrCell::Mine)
                    } else if x == "#" {
                        Ok(BoardFromStrCell::Unknown)
                    } else {
                        x.parse::<usize>()
                            .map(BoardFromStrCell::Known)
                            .map_err(|_| "Invalid integer")
                    }
                })
                .collect::<Result<Vec<_>, &'static str>>()
        })
        .collect::<Result<Vec<_>, &'static str>>()?;

    if line_vecs.is_empty() {
        Err("Empty data")
    } else if line_vecs[1..]
        .iter()
        .any(|line_vec| line_vec.len() != line_vecs[0].len())
    {
        Err("Inconsistent row lengths in board")
    } else {
        let width = line_vecs[0].len();
        Ok(BoardFromStrResult {
            h: line_vecs.len(),
            cells: line_vecs.into_iter().flatten().collect(),
            w: width,
        })
    }
}

/// Iterate the up-to-8 grid neighbours of `(row, col)` in a `w` by `h`
/// grid, skipping the cell itself and any positions outside the grid.
pub fn iter_neighbors(
    row: usize,
    col: usize,
    w: usize,
    h: usize,
) -> impl Iterator<Item = (usize, usize)> {
    let i_lo = row.saturating_sub(1);
    let j_lo = col.saturating_sub(1);
    let i_hi = (row + 1).min(h - 1);
    let j_hi = (col + 1).min(w - 1);
    (i_lo..=i_hi).flat_map(move |ni| {
        (j_lo..=j_hi).filter_map(move |nj| {
            if ni == row && nj == col {
                None
            } else {
                Some((ni, nj))
            }
        })
    })
}

/// True if the two flat indices are grid-adjacent (within one row and one
/// column of each other). Note this returns `true` when `a_index == b_index`.
pub fn is_adj(a_index: usize, b_index: usize, board_width: usize) -> bool {
    (a_index / board_width).abs_diff(b_index / board_width) <= 1
        && (a_index % board_width).abs_diff(b_index % board_width) <= 1
}

/// Convert a flat row-major index to `(row, col)` for a grid of the given
/// width.
pub fn position_to_row_col(pos: usize, width: usize) -> (usize, usize) {
    (pos / width, pos % width)
}
