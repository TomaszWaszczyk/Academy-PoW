//! This crate represents a concrete Substrate PoW algorithm.
//!
//! It is multi-pow in the sense that there are multiple supported hashing algorithms.
//! A seal with any of the supported hashing algorithms will be accepted.
//!
//! The purpose of this design is to demonstrate hard and soft forks by adding and removing valid hashing algorithms.
//! While there is no precedent for changing hashing algorithms in the real world yet, it is conceivable that
//! a chain may want to upgrade to a new algorithm when the old one is suspected weak.
//! In any case, the point is that we want to demonstrate hard and soft forks in an understandable way,
//! the multiple hashing algorithms achieves that well.
//!
//! In the future, the hope is that there will be a dedicated difficulty threshold for each hashing algorithm.
//! But currently the Substrate PoW crates are not that flexible.
//! We could solve it by adding a pre-digest that includes information about what hashing algo is being used
//! for the runtime to use later in the difficulty adjustment.

#![cfg_attr(not(feature = "std"), no_std)]

use core::str::FromStr;
#[cfg(feature = "std")]
use std::sync::Arc;

use parity_scale_codec::{Decode, Encode};
#[cfg(feature = "std")]
use sc_consensus_pow::{Error, PowAlgorithm};
#[cfg(feature = "std")]
use sha3::{Digest, Keccak256, Sha3_256};
#[cfg(feature = "std")]
use sp_api::ProvideRuntimeApi;
#[cfg(feature = "std")]
use sp_consensus_pow::DifficultyApi;
#[cfg(feature = "std")]
use sp_consensus_pow::Seal as RawSeal;
use sp_consensus_pow::TotalDifficulty;
use sp_core::{H256, U256};
#[cfg(feature = "std")]
use sp_runtime::generic::BlockId;
#[cfg(feature = "std")]
use sp_runtime::traits::{Block as BlockT, Header as HeaderT};

/// A struct that represents a difficulty threshold.
/// Unlike a normal PoW algorithm this struct has a separate threshold for each hash
#[derive(
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Encode,
    Decode,
    Debug,
    Default,
    scale_info::TypeInfo,
)]
pub struct Threshold {
    pub md5: U256,
    pub sha3: U256,
    pub keccak: U256,
}

// This trait does not seem to be fully baked in the Substrate PoW code
// But we do need some kind of sinsible impl here so the node can import blocks.
// so I will not use it for now.
impl TotalDifficulty for Threshold {
    fn increment(&mut self, other: Threshold) {
        self.md5 += other.md5;
        self.sha3 += other.sha3;
        self.keccak += other.keccak;
    }
}

/// An enum that represents the supported hash types
#[derive(Clone, Copy, PartialEq, Eq, Encode, Decode, Debug)]
pub enum SupportedHashes {
    Md5,
    Sha3,
    Keccak,
}

impl Default for SupportedHashes {
    fn default() -> Self {
        Self::Sha3
    }
}

/// A struct that represents a concrete hash value tagged with what hashing
///  algorithm was used to compute it.
#[derive(Clone, Copy, PartialEq, Eq, Encode, Decode, Debug, Default)]
pub struct MultiHash {
    pub algo: SupportedHashes,
    pub value: H256,
}

/// Determine whether the given hash satisfies the given difficulty.
/// The test is done by multiplying the two together. If the product
/// overflows the bounds of U256, then the product (and thus the hash)
/// was too high.
pub fn simple_hash_meets_difficulty(hash: &H256, difficulty: U256) -> bool {
    let num_hash = U256::from(&hash[..]);
    let (_, overflowed) = num_hash.overflowing_mul(difficulty);

    !overflowed
}

pub fn multi_hash_meets_difficulty(hash: &MultiHash, difficulty: Threshold) -> bool {
    match hash.algo {
        SupportedHashes::Md5 => simple_hash_meets_difficulty(&hash.value, difficulty.md5),
        SupportedHashes::Sha3 => simple_hash_meets_difficulty(&hash.value, difficulty.sha3),
        SupportedHashes::Keccak => simple_hash_meets_difficulty(&hash.value, difficulty.keccak),
    }
}

/// A Seal struct that will be encoded to a Vec<u8> as used as the
/// `RawSeal` type.
#[derive(Clone, PartialEq, Eq, Encode, Decode, Debug)]
pub struct Seal {
    pub work: MultiHash,
    pub difficulty: Threshold,
    pub nonce: U256,
}

/// A not-yet-computed attempt to solve the proof of work. Calling the
/// compute method will compute the hash and return the seal.
#[derive(Clone, PartialEq, Eq, Encode, Decode, Debug)]
pub struct Compute {
    pub difficulty: Threshold,
    pub pre_hash: H256,
    pub nonce: U256,
}

#[cfg(feature = "std")]
impl Compute {
    pub fn compute(self, algo: SupportedHashes) -> Seal {
        let value = match algo {
            SupportedHashes::Md5 => {
                // The md5 is only 16 byte output, so we just concatenate it twice to
                // get an H256
                let bytes = *md5::compute(&self.encode()[..]);
                let mut doubled = [0u8; 32];
                doubled[0..16].copy_from_slice(&bytes[0..16]);
                doubled[16..32].copy_from_slice(&bytes[0..16]);

                H256::from(doubled)
            }
            SupportedHashes::Sha3 => {
                H256::from_slice(Sha3_256::digest(&self.encode()[..]).as_slice())
            }
            SupportedHashes::Keccak => {
                H256::from_slice(Keccak256::digest(&self.encode()[..]).as_slice())
            }
        };

        Seal {
            nonce: self.nonce,
            difficulty: self.difficulty,
            work: MultiHash { algo, value },
        }
    }
}

#[cfg(feature = "std")]
/// A complete PoW Algorithm that uses multiple hashing algorithms.
/// Needs a reference to the client so it can grab the difficulty from the runtime.
pub struct MultiPow<C> {
    client: Arc<C>,
    fork_config: ForkingConfig,
}

#[cfg(feature = "std")]
impl<C> MultiPow<C> {
    pub fn new(client: Arc<C>, fork_config: ForkingConfig) -> Self {
        Self {
            client,
            fork_config,
        }
    }
}

//TODO could maybe derive clone_no_bound
#[cfg(feature = "std")]
impl<C> Clone for MultiPow<C> {
    fn clone(&self) -> Self {
        Self::new(self.client.clone(), self.fork_config)
    }
}

// Here we implement the general PowAlgorithm trait for our concrete algorithm.
#[cfg(feature = "std")]
impl<B: BlockT<Hash = H256>, C> PowAlgorithm<B> for MultiPow<C>
where
    C: ProvideRuntimeApi<B>,
    C::Api: DifficultyApi<B, Threshold>,
    C: sc_client_api::HeaderBackend<B>,
{
    type Difficulty = Threshold;

    fn difficulty(&self, parent: B::Hash) -> Result<Self::Difficulty, Error<B>> {
        let difficulty = self
            .client
            .runtime_api()
            .difficulty(parent)
            .map_err(|err| {
                sc_consensus_pow::Error::Environment(format!(
                    "Fetching difficulty from runtime failed: {:?}",
                    err
                ))
            })?;

        Ok(difficulty)
    }

    fn verify(
        &self,
        parent_id: &BlockId<B>,
        pre_hash: &H256,
        _pre_digest: Option<&[u8]>,
        seal: &RawSeal,
        difficulty: Self::Difficulty,
    ) -> Result<bool, Error<B>> {
        // Try to construct a seal object by decoding the raw seal given
        let seal = match Seal::decode(&mut &seal[..]) {
            Ok(seal) => seal,
            Err(_) => return Ok(false),
        };

        // This is where we handle forks on the verification side.
        // We will still need to handle it in the mining algorithm somewhere.
        // Currently we make the miner configure what algo they mine manually with their cli.
        let _parent_number = match parent_id {
            BlockId::Hash(h) => *self
                .client
                .header(*h)
                .expect("Database should perform lookup successfully")
                .expect("parent header should be present in the db")
                .number(),
            BlockId::Number(n) => *n,
        };

        // When we are ready to do a fork, this is where to do it.
        // Declare a threshold height at which to perform a fork
        // let fork_height: <<B as BlockT>::Header as HeaderT>::Number = 7900u32.into();

        // To begin with we only allow md5 hashes for our pow
        // After the fork height this check is skipped so all the hashes become valid
        // if parent_number > fork_height {
        //     match seal.work.algo {
        //         SupportedHashes::Md5 => {return Ok(false)},
        //         SupportedHashes::Sha3 => (),
        //         SupportedHashes::Keccak => (),
        //     }
        // }

        // See whether the hash meets the difficulty requirement. If not, fail fast.
        if !multi_hash_meets_difficulty(&seal.work, difficulty) {
            return Ok(false);
        }

        // Make sure the provided work actually comes from the correct pre_hash
        let compute = Compute {
            difficulty,
            pre_hash: *pre_hash,
            nonce: seal.nonce,
        };

        if compute.compute(seal.work.algo) != seal {
            return Ok(false);
        }

        Ok(true)
    }
}

#[derive(Copy, Clone, Eq, PartialEq)]
///
pub struct ForkHeights {
    /// The block height to perform the soft fork that adds sha3 and keccak support.
    pub add_sha3_keccak: u32,
    /// The block height to perform the hard fork that removes md5 support.
    pub remove_md5: u32,
    /// The block height to perform the contentious fork where some become sha3- or keccak-maxis.
    pub split_sha3_keccak: u32,
}

/// Various political positions a node could take when the network is forking into
/// keccak maxis and sha3 maxis
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum MaxiPosition {
    /// Allow all blocks, both sha3 and keccak
    NoMaxi,
    /// Only allow sha3 blocks
    Sha3Maxi,
    /// Only allow keccak blocks
    KeccakMaxi,
    /// Only allow a single type of blocks. Which type it is is determined by what algo the node is mining.
    FollowMining,
}

#[derive(Copy, Clone, Eq, PartialEq)]
/// The actual properly typed config after we're done working around all the BS.
pub enum ForkingConfig {
    ///
    Manual,
    ///
    Automatic(ForkHeights, MaxiPosition),
}

impl FromStr for MaxiPosition {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match &s.to_lowercase()[..] {
            "allow-all" | "allowall" | "no-maxi" | "nomaxi" => Self::NoMaxi,
            "sha3-maxi" | "sha3maxi" => Self::Sha3Maxi,
            "keccak-maxi" | "keccakmaxi" => Self::KeccakMaxi,
            _ => Self::FollowMining,
        })
    }
}
