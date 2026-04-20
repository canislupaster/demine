use demine::{
    board::{self, Board},
    solver::{Solver, SolverFailure, print_dbg_counters},
    utils::iter_neighbors,
};
use rand::{RngExt, SeedableRng, rngs::SmallRng};

// Brute forcing is 2^n per board.
const BRUTE_FORCE_LIMIT: usize = 14;

/// Brute force that there are at least two mine positions with these knowns.
fn assert_must_guess(board: &Board) {
    let mut outside = vec![false; board.h() * board.w()];
    let mut may_have_mine = vec![true; board.h() * board.w()];
    let mut validate_knowns = Vec::new();
    for row in 0..board.h() {
        for col in 0..board.w() {
            let idx = row * board.w() + col;
            match board.known()[idx] {
                None => {
                    if iter_neighbors(row, col, board.w(), board.h())
                        .all(|(ni, nj)| board.known()[ni * board.w() + nj].is_none())
                    {
                        outside[idx] = true;
                        may_have_mine[idx] = false;
                    }
                }
                Some(k) => {
                    may_have_mine[idx] = false;
                    if k == 0 {
                        for (ni, nj) in iter_neighbors(row, col, board.w(), board.h()) {
                            may_have_mine[ni * board.w() + nj] = false;
                        }
                    } else {
                        validate_knowns.push((row, col, k));
                    }
                }
            }
        }
    }

    if may_have_mine.iter().filter(|&&x| x).count() <= BRUTE_FORCE_LIMIT {
        let idxes = may_have_mine
            .iter()
            .enumerate()
            .filter_map(|(idx, &can_have_mine)| if can_have_mine { Some(idx) } else { None })
            .collect::<Vec<_>>();

        let mut msk = vec![false; board.w() * board.h()];
        let mut ever_a_mine = outside.clone();
        // Assume board isn't solved and there's still potential mines on perimeter.
        let mut remaining_non_mine = ever_a_mine.iter().filter(|&&x| !x).count();

        for mines_msk in 0..1usize << idxes.len() {
            for (i, &idx) in idxes.iter().enumerate() {
                msk[idx] = (1 << i) & mines_msk != 0;
            }

            let valid = validate_knowns.iter().all(|&(row, col, n_mine)| {
                iter_neighbors(row, col, board.w(), board.h())
                    .filter(|(ni, nj)| msk[ni * board.w() + nj])
                    .count()
                    == n_mine
            });

            if valid {
                for (&a, b) in msk.iter().zip(ever_a_mine.iter_mut()) {
                    if !*b {
                        *b = *b || a;
                        remaining_non_mine -= 1;
                        if remaining_non_mine == 0 {
                            return;
                        }
                    }
                }
            }
        }

        panic!("don't have to guess for board:\n{}", board);
    }
}

/// Solve a single random game.
fn single_game(seed: u64, fixed_num_mines: bool) -> (usize, usize) {
    println!("test fixed={fixed_num_mines}, seed={seed}");

    let mut rng = SmallRng::seed_from_u64(seed);
    let [w, h] = [8, 8]; //[(); 2].map(|_| rng.random_range(1..8));
    let n_mine = rng.random_range(0..=h * w);
    let mut solver = Solver::new(w, h, seed);

    let mut board = board::Board::random_mines(w, h, n_mine, &mut rng);
    let mut used_empty = false;
    let (mut num_hints, mut num_steps) = (0usize, 0usize);
    let n_mines = if fixed_num_mines { Some(n_mine) } else { None };

    while !board.solved() {
        // println!("running solver on\n{}", board);
        let mut known_solver = solver
            .with_known(board.known(), n_mines)
            .expect("couldn't get solver with known");

        match known_solver.find_safe_cell() {
            Ok((row, col)) => {
                assert!(board.reveal(row, col).unwrap() > 0);
                num_steps += 1;
            }
            Err(demine::solver::Error::SolverFailure(
                failure @ (SolverFailure::Empty | SolverFailure::MustGuess),
            )) => {
                if failure == SolverFailure::Empty {
                    assert!(!used_empty);
                    used_empty = true;
                } else if cfg!(debug_assertions) {
                    assert_must_guess(&board);
                }

                // Reveal a random cell so the game proceeds.
                board.reveal_random_non_mine(&mut rng);
                num_hints += 1;
            }
            Err(err) => {
                panic!("unexpected error finding safe cell: {err:?}");
            }
        }
    }

    // One hint is needed to get the game going, assuming there are mines.
    (num_hints.saturating_sub(1), num_steps)
}

/// Stress test that takes some time. This should be run with debug_assertions
/// for maximum coverage, and without for profiling.
#[test]
#[ignore]
fn multi_game() {
    for fixed in [false, true] {
        let (mut total_hints, mut total_steps) = (0, 0);
        let n_tests = 1000;
        for seed in 0..n_tests {
            let (hints, steps) = single_game(seed, fixed);
            total_hints += hints;
            total_steps += steps;
        }
        println!(
            "fixed={fixed}, avg num hints: {}, avg num steps: {}",
            total_hints as f64 / n_tests as f64,
            total_steps as f64 / n_tests as f64
        );
        print_dbg_counters();
    }
    println!("finished");
}
