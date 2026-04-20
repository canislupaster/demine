# demine

[![CI](https://img.shields.io/github/actions/workflow/status/canislupaster/demine/ci.yml?branch=main&label=ci&style=for-the-badge)](https://github.com/canislupaster/demine/actions/workflows/ci.yml)
[![Crates.io](https://img.shields.io/crates/v/demine?style=for-the-badge)](https://crates.io/crates/demine)
[![Docs.rs](https://img.shields.io/docsrs/demine?style=for-the-badge)](https://docs.rs/demine)
[![Rust](https://img.shields.io/badge/rust-2024-ed6c2f?style=for-the-badge&logo=rust)](https://www.rust-lang.org/)
[![License](https://img.shields.io/badge/license-MIT%20%7C%20Apache--2.0-blue?style=for-the-badge)](#license)
[![Unsafe](https://img.shields.io/badge/unsafe-forbidden-111111?style=for-the-badge)](#)

A minesweeper solver (and soon, engine). Still WIP, and rest of this doc is Claude.

## Example

```rust
use demine::board::Board;
use demine::solver::Solver;
use rand::SeedableRng;
use rand::rngs::SmallRng;

let (w, h, n_mines) = (8, 8, 10);
let mut rng = SmallRng::seed_from_u64(0);

// Generate a random board and reveal one safe cell to get started.
let mut board = Board::random_mines(w, h, n_mines, &mut rng);
board.reveal_random_non_mine(&mut rng);

// Ask the solver for a guaranteed-safe cell given the current known state.
let mut solver = Solver::new(w, h, 0);
let mut view = solver.with_known(board.known(), Some(n_mines)).unwrap();
let (row, col) = view.find_safe_cell().unwrap();

println!("safe cell: ({row}, {col})");
```

## Crate layout

- [`board`](src/board.rs) — board state, cell references, reveal / mine-set primitives.
- [`generator`](src/generator.rs) — mine placement and board generation.
- [`solver`](src/solver.rs) — solver entry point; deeper logic lives in [`src/solver/`](src/solver/).
- [`utils`](src/utils.rs) — shared helpers (parsing, neighbor iteration, formatting).

## Features

- `use-mimalloc` — swap the global allocator to mimalloc (v3). Off by default;
  enable for benchmarks and release runs where allocator wins matter.

## Profiles

- `release` — max optimizations (fat LTO, single codegen unit).
- `testing` — release-optimized but with `debug_assertions` on; used by the
  `cargo test-all` alias to catch UB / overflow in hot paths.
- `profiling` — release + debug symbols, for perf tools (samply, Instruments).
