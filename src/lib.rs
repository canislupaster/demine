#[cfg(feature = "use-mimalloc")]
mod use_mimalloc {
    use mimalloc::MiMalloc;

    #[global_allocator]
    static GLOBAL_ALLOCATOR: MiMalloc = MiMalloc;
}

pub mod board;
pub mod generator;
pub mod solver;
pub mod utils;
