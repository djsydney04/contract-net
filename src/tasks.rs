//! Exact integer executors. All random draws match `random.Random(int(seed))`.
use anyhow::{Context, Result, bail, ensure};
use num_bigint::{BigInt, BigUint};
use num_traits::{ToPrimitive, Zero};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::random::PythonRandom;

pub const TASK_TYPES: [&str; 5] = [
    "monte_carlo_pi",
    "prime_count",
    "hash_search",
    "sort_checksum",
    "matmul_mod",
];
pub const CHECKSUM_MOD: u128 = (1 << 61) - 1;

pub(crate) fn integer(params: &Value, name: &str) -> Result<BigInt> {
    let value = params
        .get(name)
        .with_context(|| format!("missing parameter {name}"))?;
    let text = match value {
        Value::Number(n) => n.to_string(),
        Value::String(s) => s.trim().to_owned(),
        _ => bail!("{name} must be an integer"),
    };
    text.parse()
        .with_context(|| format!("{name} must be an integer"))
}

pub(crate) fn count(params: &Value, name: &str) -> Result<usize> {
    // Python range(negative) is empty.
    integer(params, name)?
        .max(BigInt::zero())
        .to_usize()
        .with_context(|| format!("{name} exceeds the machine's addressable size"))
}

fn rng(params: &Value) -> Result<PythonRandom> {
    Ok(PythonRandom::new(integer(params, "seed")?.magnitude()))
}

fn zeros<T: Clone + Default>(length: usize) -> Result<Vec<T>> {
    let mut values = Vec::new();
    values
        .try_reserve_exact(length)
        .context("task allocation is too large")?;
    values.resize(length, T::default());
    Ok(values)
}

/// Compute an answer without Python. BigUint keeps results exact beyond u64.
pub fn run_task(task_type: &str, params: &Value) -> Result<BigUint> {
    match task_type {
        "monte_carlo_pi" => {
            let mut random = rng(params)?;
            let mut inside = 0u64;
            for _ in 0..count(params, "samples")? {
                let (x, y) = (random.random(), random.random());
                if x * x + y * y <= 1.0 {
                    inside += 1;
                }
            }
            Ok(inside.into())
        }
        "prime_count" => prime_count(params),
        "hash_search" => hash_search(params),
        "sort_checksum" => {
            let mut random = rng(params)?;
            let mut values = zeros::<u32>(count(params, "n")?)?;
            for value in &mut values {
                *value = random.below_u64(1 << 31) as u32;
            }
            values.sort_unstable();
            let total = values.iter().enumerate().fold(0u128, |sum, (i, &v)| {
                (sum + (i as u128 + 1) * u128::from(v)) % CHECKSUM_MOD
            });
            Ok(total.into())
        }
        "matmul_mod" => matmul_mod(params),
        _ => bail!("unknown task type: {task_type}"),
    }
}

fn prime_count(params: &Value) -> Result<BigUint> {
    let lo = integer(params, "lo")?.max(BigInt::from(2));
    let hi = integer(params, "hi")?;
    if hi <= lo {
        return Ok(BigUint::zero());
    }
    let lo = lo.to_u64().context("prime lower bound exceeds u64")?;
    let hi = hi.to_u64().context("prime upper bound exceeds u64")?;
    let limit = (hi - 1).isqrt();
    let mut base = zeros::<bool>(usize::try_from(limit + 1)?)?;
    base.fill(true);
    base[0] = false;
    if limit >= 1 {
        base[1] = false;
    }
    for p in 2..=limit.isqrt() {
        if base[p as usize] {
            for multiple in (p * p..=limit).step_by(p as usize) {
                base[multiple as usize] = false;
            }
        }
    }
    let primes: Vec<u64> = (2..=limit).filter(|&p| base[p as usize]).collect();
    let mut total = 0u64;
    let mut start = lo;
    while start < hi {
        let stop = start.saturating_add(1_000_000).min(hi);
        let mut candidates = vec![true; (stop - start) as usize];
        for &p in &primes {
            let first = (u128::from(start).div_ceil(u128::from(p)) * u128::from(p))
                .max(u128::from(p) * u128::from(p));
            if first < u128::from(stop) {
                for multiple in (first as u64..stop).step_by(p as usize) {
                    candidates[(multiple - start) as usize] = false;
                }
            }
        }
        total += candidates.into_iter().filter(|&prime| prime).count() as u64;
        start = stop;
    }
    Ok(total.into())
}

pub(crate) fn hash_prefix(seed: &str) -> Sha256 {
    let mut hash = Sha256::new();
    hash.update(seed.as_bytes());
    hash.update(b":");
    hash
}

pub(crate) fn hash_attempt(prefix: &Sha256, nonce: u64) -> u32 {
    // Decimal encoding without allocating a String on every attempt.
    let mut buffer = [0u8; 20];
    let (mut n, mut i) = (nonce, buffer.len());
    loop {
        i -= 1;
        buffer[i] = b'0' + (n % 10) as u8;
        n /= 10;
        if n == 0 {
            break;
        }
    }
    let mut hash = prefix.clone();
    hash.update(&buffer[i..]);
    let digest = hash.finalize();
    u32::from_be_bytes(
        digest[..4]
            .try_into()
            .expect("SHA-256 has at least four bytes"),
    )
}

fn hash_search(params: &Value) -> Result<BigUint> {
    let threshold = integer(params, "threshold")?;
    ensure!(
        threshold > BigInt::zero(),
        "hash threshold must be positive"
    );
    if threshold >= BigInt::from(1u64 << 32) {
        return Ok(BigUint::zero());
    }
    let threshold = threshold.to_u32().context("invalid hash threshold")?;
    let seed = params.get("seed").context("missing parameter seed")?;
    let seed = match seed {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        _ => bail!("hash seed must be a string or number"),
    };
    let prefix = hash_prefix(&seed);
    for nonce in 0..=u64::MAX {
        if hash_attempt(&prefix, nonce) < threshold {
            return Ok(nonce.into());
        }
    }
    bail!("hash nonce exceeds u64")
}

fn matmul_mod(params: &Value) -> Result<BigUint> {
    let n = count(params, "n")?;
    let modulus = integer(params, "mod")?
        .to_biguint()
        .filter(|m| !m.is_zero())
        .context("matrix modulus must be positive")?;
    let mut random = rng(params)?;
    // sum(A @ B) = sum_k(column_sum(A,k) * row_sum(B,k)); O(n²) work, O(n) storage.
    // Reduce every sum to keep the native path within u128 even for u64 moduli.
    if let Some(modulus) = modulus.to_u64() {
        let m = u128::from(modulus);
        let mut columns = zeros::<u128>(n)?;
        for _ in 0..n {
            for column in &mut columns {
                *column = (*column + u128::from(random.below_u64(modulus))) % m;
            }
        }
        let mut total = 0u128;
        for column in columns {
            let mut row = 0u128;
            for _ in 0..n {
                row = (row + u128::from(random.below_u64(modulus))) % m;
            }
            total = (total + (column * row) % m) % m;
        }
        return Ok(total.into());
    }
    let mut columns = zeros::<BigUint>(n)?;
    for _ in 0..n {
        for column in &mut columns {
            *column += random.below_big(&modulus);
        }
    }
    let mut total = BigUint::zero();
    for column in columns {
        let mut row = BigUint::zero();
        for _ in 0..n {
            row += random.below_big(&modulus);
        }
        total += column * row;
    }
    Ok(total % modulus)
}
