//! Register-shaped AIR (algebraic intermediate representation) for the Ananse zkVM.
//!
//! This crate defines [`AnanseAir`], the single [`Air`](p3_air::Air) the STARK
//! prover and verifier evaluate against the trace [`ananse_trace`] produces, enabling
//! direct proving/verifcation of WebAssembly.

#![forbid(unsafe_code)]

mod bus;
mod constraints;
mod error;
mod rom;

use constraints::{
    AUX_WIDTH, CHALLENGE_BITWISE_DENOM, CHALLENGE_BITWISE_FOLD, CHALLENGE_BOUNDARY,
    CHALLENGE_CONTROL_FLOW, CHALLENGE_DATA_DENOM, CHALLENGE_DATA_FOLD, CHALLENGE_DENOM,
    CHALLENGE_FOLD, CHALLENGE_POPCOUNT_DENOM, CHALLENGE_POPCOUNT_FOLD, CHALLENGE_RANGE, bitwise,
    boundary, control, logup, permutation, popcount, range,
};
pub use constraints::{AnanseAir, NUM_CHALLENGES};
pub use error::AirError;
use p3_field::extension::BinomialExtensionField;
use p3_matrix::Matrix;
use p3_matrix::dense::RowMajorMatrix;
pub use rom::{pack_edge, program_data, program_rom};

/// Result of AIR operations.
pub type Result<T> = core::result::Result<T, AirError>;

/// The prime field known as Goldilocks, defined as `F_p` where `p = 2^64 - 2^32 + 1`.
pub type Felt = p3_goldilocks::Goldilocks;

/// The quadratic extension of Goldilocks carrying the permutation-argument
/// challenges and the LogUp and multiset-equality witnesses.
pub type QuadExt = BinomialExtensionField<Felt, 2>;

pub fn build_permutation_trace(
    main: &RowMajorMatrix<Felt>,
    rom: &[Felt],
    data: &[(u32, u32, u32)],
    initial: &[(u64, Felt, Felt)],
    challenges: [QuadExt; NUM_CHALLENGES],
) -> Result<RowMajorMatrix<QuadExt>> {
    let (multiplicity, grand_sum) =
        logup::control_flow_columns(main, rom, challenges[CHALLENGE_CONTROL_FLOW])?;
    let (bus_acc, sorted_acc) = permutation::consistency_columns(
        main,
        challenges[CHALLENGE_DENOM],
        challenges[CHALLENGE_FOLD],
    )?;
    let range = range::columns(main, challenges[CHALLENGE_RANGE])?;
    let boundary = boundary::columns(
        main,
        initial,
        challenges[CHALLENGE_FOLD],
        challenges[CHALLENGE_BOUNDARY],
    )?;
    let bitwise = bitwise::columns(
        main,
        challenges[CHALLENGE_BITWISE_FOLD],
        challenges[CHALLENGE_BITWISE_DENOM],
    )?;
    let popcount = popcount::columns(
        main,
        challenges[CHALLENGE_POPCOUNT_FOLD],
        challenges[CHALLENGE_POPCOUNT_DENOM],
    )?;
    let data = control::columns(
        main,
        data,
        challenges[CHALLENGE_DATA_FOLD],
        challenges[CHALLENGE_DATA_DENOM],
    )?;

    let aux = [multiplicity, grand_sum, bus_acc, sorted_acc]
        .into_iter()
        .chain(range)
        .chain(boundary)
        .chain(bitwise)
        .chain(popcount)
        .chain(data)
        .collect::<Vec<Vec<QuadExt>>>();
    debug_assert_eq!(aux.len(), AUX_WIDTH);
    let values = (0..main.height())
        .flat_map(|i| aux.iter().map(move |column| column[i]))
        .collect();
    Ok(RowMajorMatrix::new(values, AUX_WIDTH))
}
