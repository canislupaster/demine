use demine::{
    board::{self, Board},
    solver::Solver,
    utils::position_to_row_col,
};
use rand::{RngExt, SeedableRng, rngs::SmallRng};

const NUM_TESTS: usize = 500;
const SIZE_LIMIT: usize = 20;

fn single_board(seed: u64, fixed_num_mines: bool) {
    println!("test fixed={fixed_num_mines}, seed = {seed}");

    let mut rng = SmallRng::seed_from_u64(seed);
    let [w, h] = loop {
        let [w, h] = [(); 2].map(|_| rng.random_range(1..8));
        if w * h <= SIZE_LIMIT && h * w > 2 {
            break [w, h];
        }
    };

    loop {
        let n_mine = rng.random_range(1..h * w - 1);
        let mut board = board::Board::random_mines(w, h, n_mine, &mut rng);
        let min_n_known = rng.random_range(1..board.len() - n_mine);
        while board.known_count() < min_n_known {
            assert!(board.reveal_random_non_mine(&mut rng).unwrap() > 0);
        }
        if board.solved() {
            continue;
        }

        println!("found board, solving");
        let mut solver = Solver::new(w, h, seed);

        let mut known_solver = solver
            .with_known(
                board.known(),
                if fixed_num_mines {
                    Some(board.n_mine())
                } else {
                    None
                },
            )
            .unwrap();

        let unknown_cells = board
            .iter()
            .filter(|x| x.known().is_none())
            .map(|x| {
                let mine_prob = known_solver.mine_probability(x.row(), x.col()).unwrap();
                (mine_prob, x.index())
            })
            .collect::<Vec<_>>();

        let mut board_mines = Vec::from(board.mines());
        let mut mine_count = unknown_cells.iter().map(|_| 0).collect::<Vec<_>>();
        let mut total_satisfying = 0;

        println!("checking results");
        for msk in 0..1usize << unknown_cells.len() {
            for (msk_bit, &(_, cell_i)) in unknown_cells.iter().enumerate() {
                board_mines[cell_i] = msk & (1 << msk_bit) != 0;
            }

            let mut new_board = Board::from_mines(w, h, board_mines.iter().copied());
            new_board.reveal_all();
            if (!fixed_num_mines || new_board.n_mine() == n_mine)
                && board
                    .known()
                    .iter()
                    .zip(new_board.known())
                    .all(|(a, b)| match a {
                        None => true,
                        Some(_) => a == b,
                    })
            {
                total_satisfying += 1;
                for (msk_bit, count) in mine_count.iter_mut().enumerate() {
                    if msk & (1 << msk_bit) != 0 {
                        *count += 1;
                    }
                }
            }
        }

        println!("found {} mine arrangements", total_satisfying);
        assert!(total_satisfying > 0);
        for ((mine_prob, cell_i), count) in unknown_cells.into_iter().zip(mine_count) {
            let true_prob = count as f32 / total_satisfying as f32;
            let ok = mine_prob.is_none_or(|x| (x - true_prob).abs() < 1e-4);
            if !ok || (mine_prob.is_none() && fixed_num_mines) {
                dbg!(cell_i, mine_prob, true_prob, count);
                println!("board:\n{}", board);
                panic!("mismatched probabilities");
            }

            let (row, col) = position_to_row_col(cell_i, board.w());
            let reported_can_be_mine = known_solver.can_be_mine(row, col).unwrap();
            assert!((true_prob > 0.0) == reported_can_be_mine);

            if let Some(x) = mine_prob {
                let can_be_mine = x > 0.0;
                assert!(can_be_mine == reported_can_be_mine);
            }
        }

        break;
    }
}

#[test]
#[ignore]
fn multi_board() {
    for test_index in 0..NUM_TESTS {
        single_board(test_index as u64, false);
        single_board(test_index as u64, true);
    }
}
